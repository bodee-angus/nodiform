//! Pure JavaScript generation, with capability-free QuickJS and a killable worker process.
//! The process boundary is for fault containment; it is not an OS security sandbox.
pub use crate::model::Plan;
use crate::model::{Event, Graph};
use rquickjs::{Context, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

pub const RULE_API_VERSION: &str = "nodiform-rules-v1";
pub const MAX_EVENTS: usize = 100_000;
const MAX_SOURCE_BYTES: usize = 1_048_576;
const MAX_PARAMETER_BYTES: usize = 262_144;
const MAX_REQUEST_BYTES: usize = 2_097_152;
const MAX_EVENT_JSON_BYTES: usize = 16_777_216;
const MAX_RESPONSE_BYTES: usize = 67_108_864;
const WORKER_TIMEOUT: Duration = Duration::from_secs(15);

/// Compile ordinary JavaScript defining function* generate(N, params).
/// No filesystem, network, process, clock, unseeded randomness, or module loader is exposed.
pub fn compile_source(source: &str, parameters: Value, seed: u32) -> Result<Plan, String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err("Rule source exceeds 1 MiB".into());
    }
    let parameters_json = serde_json::to_string(&parameters).map_err(|error| error.to_string())?;
    if parameters_json.len() > MAX_PARAMETER_BYTES {
        return Err("Rule parameters exceed 256 KiB".into());
    }
    let runtime =
        Runtime::new().map_err(|error| format!("Cannot initialise rule runtime: {error}"))?;
    // rquickjs uses QuickJS's default allocator. Custom allocator features would disable this limit.
    runtime.set_memory_limit(64 * 1024 * 1024);
    runtime.set_max_stack_size(512 * 1024);
    let started = Instant::now();
    let mut interrupt_checks = 0u32;
    runtime.set_interrupt_handler(Some(Box::new(move || {
        interrupt_checks += 1;
        interrupt_checks > 10_000 || started.elapsed() > Duration::from_secs(5)
    })));
    let context =
        Context::full(&runtime).map_err(|error| format!("Cannot create rule context: {error}"))?;
    let json = context.with(|ctx| -> Result<String, String> {
        ctx.globals().set("__nodiform_source", source).map_err(|e| e.to_string())?;
        ctx.globals().set("__nodiform_params", parameters_json).map_err(|e| e.to_string())?;
        ctx.globals().set("__nodiform_seed", seed).map_err(|e| e.to_string())?;
        ctx.eval::<String, _>(RUNNER).map_err(|error| {
            if error.is_exception() {
                let exception = ctx.catch();
                let detail = exception.as_object().and_then(|object| {
                    let message = object.get::<_, String>("message").ok()?;
                    let stack = object.get::<_, String>("stack").unwrap_or_default();
                    Some(if stack.is_empty() { message } else { format!("{message}\n{stack}") })
                }).unwrap_or_else(|| format!("{exception:?}"));
                let detail: String = detail.chars().take(4_096).collect();
                format!("Rule error: {detail}. Execution is limited to 64 MiB, a 512 KiB stack, and an instruction/time budget.")
            } else { format!("Rule runtime error: {error}") }
        })
    })?;
    // JavaScript counts UTF-16 code units; enforce the actual UTF-8 byte cap here too.
    if json.len() > MAX_EVENT_JSON_BYTES {
        return Err("Generated event JSON exceeds 16 MiB".into());
    }
    let events: Vec<Event> =
        serde_json::from_str(&json).map_err(|error| format!("Invalid generated event: {error}"))?;
    if events.len() > MAX_EVENTS {
        return Err(format!("At most {MAX_EVENTS} events are allowed"));
    }
    let mut graph = Graph::new(seed);
    let mut total_ticks = 0u64;
    for (index, event) in events.iter().enumerate() {
        graph
            .apply(event)
            .map_err(|error| format!("Event {}: {error}", index + 1))?;
        if let Event::Wait { ticks } = event {
            total_ticks = total_ticks
                .checked_add(u64::from(*ticks))
                .ok_or_else(|| "Total simulation duration overflowed".to_string())?;
        }
    }
    Ok(Plan {
        events,
        total_ticks,
        node_count: graph.nodes.len(),
        edge_count: graph.edges.len(),
    })
}

// User code executes in a separate Function scope and cannot access the runner's counters.
// Each yielded object is serialised immediately, so later mutations cannot rewrite past events.
const RUNNER: &str = r#"
(() => {
    'use strict';
    const source = globalThis.__nodiform_source;
    const parameters = JSON.parse(globalThis.__nodiform_params);
    let state = globalThis.__nodiform_seed >>> 0;
    delete globalThis.__nodiform_source;
    delete globalThis.__nodiform_params;
    delete globalThis.__nodiform_seed;
    const stringify = JSON.stringify;
    const finite = Number.isFinite;
    const join = Function.prototype.call.bind(Array.prototype.join);
    const push = Function.prototype.call.bind(Array.prototype.push);
    const freeze = Object.freeze;
    const define = Object.defineProperty;
    const ErrorType = Error;
    const imul = Math.imul;
    // QuickJS's bare context has no host IO. Also remove its clock and default random source.
    for (const name of ['Date', 'performance', 'crypto', 'process', 'require', 'fetch',
                         'XMLHttpRequest', 'WebSocket', 'setTimeout', 'setInterval',
                         'SharedArrayBuffer', 'Atomics']) {
        define(globalThis, name, { value: undefined, configurable: false, writable: false });
    }
    define(Math, 'random', { value: undefined, configurable: false, writable: false });
    freeze(Math);
    let nextEdge = 0;
    const N = freeze({
        node(id, options = {}) { return { ...options, id }; },
        edge(source, target, options = {}) {
            return { id: `edge:${nextEdge++}`, ...options, source, target };
        },
        batch(nodes = [], edges = []) { return { op: 'batch', nodes, edges }; },
        wait(ticks) { return { op: 'wait', ticks }; },
        setNode(id, options = {}) { return { ...options, op: 'set_node', id }; },
        setEdge(id, options = {}) { return { ...options, op: 'set_edge', id }; },
        random() {
            // Mulberry32, versioned as part of nodiform-rules-v1.
            state = (state + 0x6D2B79F5) >>> 0;
            let t = state;
            t = imul(t ^ (t >>> 15), t | 1);
            t ^= t + imul(t ^ (t >>> 7), t | 61);
            return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
        }
    });
    const factory = new Function('N', 'params', '"use strict";\n' + source +
        '\n;if (typeof generate !== "function") throw new Error("Define function* generate(N, params)");\nreturn generate(N, params);');
    const iterator = factory(N, parameters);
    if (!iterator || typeof iterator.next !== 'function') {
        throw new ErrorType('generate must return a synchronous generator');
    }
    const encoded = [];
    let bytes = 2;
    for (let count = 0; ; count++) {
        const step = iterator.next();
        if (step && typeof step.then === 'function') {
            throw new ErrorType('Async generators are not supported; use function* generate');
        }
        if (!step || typeof step !== 'object') throw new ErrorType('Invalid iterator result');
        if (step.done) break;
        if (count >= 100000) throw new ErrorType('At most 100000 events are allowed');
        const event = stringify(step.value, (_key, value) => {
            if (typeof value === 'number' && !finite(value)) {
                throw new ErrorType('Events cannot contain NaN or Infinity');
            }
            return value;
        });
        if (event === undefined) throw new ErrorType('Every yield must contain an event');
        bytes += event.length + 1;
        if (bytes > 16777216) throw new ErrorType('Generated event JSON exceeds 16 MiB');
        push(encoded, event);
    }
    return '[' + join(encoded, ',') + ']';
})()
"#;

#[derive(Serialize, Deserialize)]
struct WorkerRequest {
    source: String,
    parameters: Value,
    seed: u32,
}

/// A nonblocking handle. Dropping or cancelling it terminates and reaps the worker.
pub struct RuleJob {
    child: Option<Child>,
    result: Receiver<Result<Plan, String>>,
    started: Instant,
    finished: bool,
}

impl RuleJob {
    pub fn start(source: String, parameters: Value, seed: u32) -> Result<Self, String> {
        if source.len() > MAX_SOURCE_BYTES {
            return Err("Rule source exceeds 1 MiB".into());
        }
        let request = serde_json::to_vec(&WorkerRequest {
            source,
            parameters,
            seed,
        })
        .map_err(|error| error.to_string())?;
        if request.len() > MAX_REQUEST_BYTES {
            return Err("Rule request exceeds 2 MiB".into());
        }
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let mut child = Command::new(executable)
            .arg("--rule-worker")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Cannot start rule worker: {error}"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Missing worker input pipe".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Missing worker output pipe".to_string())?;
        // Input writing and output reading both stay off the GUI thread.
        let (sender, result) = mpsc::channel();
        let write_sender = sender.clone();
        std::thread::spawn(move || {
            if let Err(error) = stdin.write_all(&request) {
                let _ = write_sender.send(Err(format!("Cannot send rules to worker: {error}")));
            }
        });
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let read = stdout
                .take((MAX_RESPONSE_BYTES + 1) as u64)
                .read_to_end(&mut bytes);
            let value = match read {
                Err(error) => Err(format!("Cannot read rule worker response: {error}")),
                Ok(_) if bytes.len() > MAX_RESPONSE_BYTES => {
                    Err("Rule worker response exceeds 64 MiB".into())
                }
                Ok(_) => {
                    serde_json::from_slice::<Result<Plan, String>>(&bytes).unwrap_or_else(|error| {
                        Err(format!(
                            "Rule worker stopped without a valid response: {error}"
                        ))
                    })
                }
            };
            let _ = sender.send(value);
        });
        Ok(Self {
            child: Some(child),
            result,
            started: Instant::now(),
            finished: false,
        })
    }

    pub fn poll(&mut self) -> Option<Result<Plan, String>> {
        if self.finished {
            return None;
        }
        match self.result.try_recv() {
            Ok(result) => {
                self.finish();
                Some(result)
            }
            Err(TryRecvError::Disconnected) => {
                self.finish();
                Some(Err("Rule worker disconnected".into()))
            }
            Err(TryRecvError::Empty) if self.started.elapsed() > WORKER_TIMEOUT => {
                self.finish();
                Some(Err(
                    "Rule worker exceeded the 15-second watchdog limit".into()
                ))
            }
            Err(TryRecvError::Empty) => None,
        }
    }

    pub fn cancel(&mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        self.finished = true;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for RuleJob {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Called before GUI initialisation when argv includes --rule-worker.
pub fn worker_main() -> i32 {
    let result = (|| -> Result<Plan, String> {
        let mut request = Vec::new();
        std::io::stdin()
            .take((MAX_REQUEST_BYTES + 1) as u64)
            .read_to_end(&mut request)
            .map_err(|error| format!("Cannot read rule request: {error}"))?;
        if request.len() > MAX_REQUEST_BYTES {
            return Err("Rule request exceeds 2 MiB".into());
        }
        let request: WorkerRequest = serde_json::from_slice(&request)
            .map_err(|error| format!("Invalid rule request: {error}"))?;
        compile_source(&request.source, request.parameters, request.seed)
    })();
    let status = i32::from(result.is_err());
    let mut output = std::io::stdout().lock();
    match serde_json::to_writer(&mut output, &result)
        .and_then(|_| output.flush().map_err(serde_json::Error::io))
    {
        Ok(()) => status,
        Err(_) => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn abc_has_correct_topology_and_insertion_order() {
        let plan = compile_source(
            include_str!("../examples/abc-permutations.js"),
            json!({}),
            42,
        )
        .unwrap();
        assert_eq!(plan.node_count, 15);
        assert_eq!(plan.edge_count, 24);
        let mut graph = Graph::new(42);
        for event in &plan.events {
            graph.apply(event).unwrap();
        }
        assert_eq!(
            graph
                .nodes
                .iter()
                .take(3)
                .map(|n| n.id.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B", "C"]
        );
        let abc = graph
            .nodes
            .iter()
            .position(|node| node.id == "ABC")
            .unwrap();
        let targets: Vec<_> = graph
            .edges
            .iter()
            .filter(|edge| edge.source == abc)
            .map(|edge| graph.nodes[edge.target].id.as_str())
            .collect();
        assert_eq!(targets, vec!["AB", "BC"]);
        assert!(plan.total_ticks > 0);
    }

    #[test]
    fn parameters_control_real_rule_code() {
        let plan = compile_source(include_str!("../examples/abc-permutations.js"),
            json!({"alphabet": "AB", "maxLength": 2, "repeatLetters": true, "ticksPerNode": 7, "finalTicks": 0}), 1).unwrap();
        assert_eq!(plan.node_count, 6);
        assert_eq!(plan.edge_count, 6);
        assert_eq!(plan.total_ticks, 42);
    }

    #[test]
    fn abc_app_controls_and_legacy_aliases_agree() {
        let source = include_str!("../examples/abc-permutations.js");
        let modern = compile_source(
            source,
            json!({"alphabet": "AB", "maxLength": 2,
            "repetitions": true, "order": "reverse"}),
            42,
        )
        .unwrap();
        let legacy = compile_source(
            source,
            json!({"alphabet": "AB", "maxLength": 2,
            "repeatLetters": true, "reverse": true}),
            42,
        )
        .unwrap();
        assert_eq!(modern, legacy);
        assert_eq!(modern.node_count, 6);
        let mut graph = Graph::new(42);
        for event in &modern.events {
            graph.apply(event).unwrap();
        }
        assert_eq!(graph.nodes[0].id, "B");
        assert_eq!(graph.nodes[2].id, "BB");
    }

    #[test]
    fn abc_shuffle_is_seeded_and_preserves_topology() {
        let source = include_str!("../examples/abc-permutations.js");
        let params = json!({"order": "shuffle"});
        let first = compile_source(source, params.clone(), 42).unwrap();
        assert_eq!(first, compile_source(source, params.clone(), 42).unwrap());
        assert_ne!(first, compile_source(source, params, 19).unwrap());
        assert_eq!((first.node_count, first.edge_count), (15, 24));
    }

    #[test]
    fn rules_are_seeded_and_snapshot_events() {
        let source = "function* generate(N) { const n = N.node('a', { radius: N.random() + 1 }); yield N.batch([n]); n.id = 'b'; yield N.batch([n]); yield N.wait(9); }";
        let one = compile_source(source, json!({}), 42).unwrap();
        let two = compile_source(source, json!({}), 42).unwrap();
        assert_eq!(one, two);
        assert_ne!(one, compile_source(source, json!({}), 43).unwrap());
        assert_eq!(one.node_count, 2);
        assert_eq!(one.total_ticks, 9);
    }

    #[test]
    fn illegal_topology_and_negative_weights_are_rejected() {
        assert!(compile_source(
            "function* generate(N) { yield N.batch([], [N.edge('a', 'b')]); }",
            json!({}),
            0
        )
        .is_err());
        assert!(compile_source("function* generate(N) { yield N.batch([N.node('a')], [N.edge('a', 'a', {strength: -1})]); }", json!({}), 0).is_err());
    }

    #[test]
    fn nonfinite_values_are_not_silently_replaced_by_json_null() {
        let source =
            "function* generate(N) { yield N.batch([N.node('a', { position: [Infinity, 0] })]); }";
        assert!(compile_source(source, json!({}), 0)
            .unwrap_err()
            .contains("NaN or Infinity"));
    }

    #[test]
    fn no_clock_host_io_or_unseeded_randomness() {
        let source = r#"function* generate(N) {
            for (const x of [Date, Math.random, globalThis.process, globalThis.fetch,
                             globalThis.require, globalThis.performance]) {
                if (x !== undefined) throw new Error('Unexpected capability');
            }
            yield N.batch([N.node('ok')]);
        }"#;
        assert_eq!(compile_source(source, json!({}), 0).unwrap().node_count, 1);
    }

    #[test]
    fn infinite_loop_is_interrupted() {
        let started = Instant::now();
        let result = compile_source("function* generate(N) { while (true) {} }", json!({}), 0);
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn modular_example_compiles() {
        let plan = compile_source(
            include_str!("../examples/modular-residues.js"),
            json!({"count": 12}),
            9,
        )
        .unwrap();
        assert_eq!(plan.node_count, 12);
        assert!(plan.edge_count > 0);
    }

    #[test]
    fn growing_ring_closes_and_uses_seeded_colours() {
        let source = include_str!("../examples/ring.js");
        let plan = compile_source(source, json!({}), 42).unwrap();
        assert_eq!(
            (plan.node_count, plan.edge_count, plan.total_ticks),
            (80, 80, 960)
        );
        let mut graph = Graph::new(42);
        for event in &plan.events {
            graph.apply(event).unwrap();
        }
        let closing = graph
            .edges
            .iter()
            .find(|edge| edge.id == "closure")
            .unwrap();
        assert_eq!((closing.source, closing.target), (79, 0));
        assert_eq!(graph.component_count(), 1);
        assert_eq!(plan, compile_source(source, json!({}), 42).unwrap());
        assert_ne!(plan, compile_source(source, json!({}), 19).unwrap());
        assert_eq!(
            compile_source(source, json!({"count": 1}), 42)
                .unwrap()
                .edge_count,
            0
        );
        assert_eq!(
            compile_source(source, json!({"count": 2}), 42)
                .unwrap()
                .edge_count,
            1
        );
    }

    #[test]
    fn runtime_rejects_heap_and_stack_exhaustion() {
        assert!(compile_source(
            "function* generate() { yield new ArrayBuffer(1024 * 1024 * 1024); }",
            json!({}),
            0
        )
        .is_err());
        assert!(
            compile_source("function* generate() { yield* generate(); }", json!({}), 0).is_err()
        );
    }

    #[test]
    fn property_events_work() {
        let source = r#"function* generate(N) {
            yield N.batch([N.node('a')], [N.edge('a', 'a', {id: 'loop'})]);
            yield N.setNode('a', {color: '#ff0000', radius: 2});
            yield N.setEdge('loop', {strength: 0, color: '#ffffff'});
        }"#;
        let plan = compile_source(source, json!({}), 0).unwrap();
        let mut graph = Graph::new(0);
        for event in &plan.events {
            graph.apply(event).unwrap();
        }
        assert_eq!(graph.nodes[0].radius, 2.0);
        assert_eq!(graph.edges[0].strength, 0.0);
    }
}
