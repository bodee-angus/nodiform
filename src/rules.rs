//! Pure JavaScript generation, with capability-free QuickJS and a killable worker process.
//! The process boundary is for fault containment; it is not an OS security sandbox.
pub use crate::model::Plan;
use crate::model::{Event, Graph};
use rquickjs::{Context, Ctx, Exception, Function, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::io::{BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const RULE_API_VERSION: &str = "nodiform-rules-v6";
const MAX_SOURCE_BYTES: usize = 1_048_576;
const MAX_PARAMETER_BYTES: usize = 262_144;
const MAX_REQUEST_BYTES: usize = 2_097_152;
const NO_PROGRESS_TIMEOUT: Duration = Duration::from_secs(5);
const WORKER_TIMEOUT: Duration = Duration::from_secs(60);

/// Budgets scale with memory available when compilation begins. One quarter is
/// allowed for the JS heap and one quarter for events plus their validation graph.
/// The remaining half leaves room for IPC, the UI, solver state and the OS.
#[derive(Clone, Copy)]
struct MemoryBudget {
    heap: usize,
    plan: usize,
}

impl MemoryBudget {
    fn detect() -> Result<Self, String> {
        let meminfo = std::fs::read_to_string("/proc/meminfo")
            .map_err(|error| format!("Cannot determine available rule memory: {error}"))?;
        let mut available = meminfo
            .lines()
            .find_map(|line| {
                let value = line.strip_prefix("MemAvailable:")?;
                value
                    .split_whitespace()
                    .next()?
                    .parse::<usize>()
                    .ok()?
                    .checked_mul(1024)
            })
            .ok_or_else(|| "Cannot determine available rule memory".to_string())?;
        // Honour cgroup v2 limits as well as host RAM, including ancestor limits.
        if let Ok(groups) = std::fs::read_to_string("/proc/self/cgroup") {
            if let Some(group) = groups.lines().find_map(|line| line.strip_prefix("0::")) {
                let root = std::path::Path::new("/sys/fs/cgroup");
                let mut directory = root.join(group.trim_start_matches('/'));
                loop {
                    let maximum = std::fs::read_to_string(directory.join("memory.max"))
                        .ok()
                        .and_then(|value| value.trim().parse::<usize>().ok());
                    let current = std::fs::read_to_string(directory.join("memory.current"))
                        .ok()
                        .and_then(|value| value.trim().parse::<usize>().ok());
                    if let (Some(maximum), Some(current)) = (maximum, current) {
                        available = available.min(maximum.saturating_sub(current));
                    }
                    if directory == root || !directory.pop() {
                        break;
                    }
                }
            }
        }
        if available == 0 {
            return Err("No available memory remains for rule compilation".into());
        }
        Ok(Self {
            heap: available / 4,
            plan: available / 4,
        })
    }
}

#[derive(Clone)]
struct Progress {
    last: Rc<Cell<Instant>>,
    reported: Rc<Cell<Instant>>,
    report: Rc<RefCell<Box<dyn FnMut()>>>,
}

impl Progress {
    fn new(report: impl FnMut() + 'static) -> Self {
        Self {
            last: Rc::new(Cell::new(Instant::now())),
            reported: Rc::new(Cell::new(Instant::now())),
            report: Rc::new(RefCell::new(Box::new(report))),
        }
    }

    fn tick(&self) {
        let now = Instant::now();
        self.last.set(now);
        if now.duration_since(self.reported.get()) >= Duration::from_millis(250) {
            (self.report.borrow_mut())();
            self.reported.set(now);
        }
    }
}

struct Compilation {
    events: Vec<Event>,
    graph: Graph,
    total_ticks: u64,
    estimated_bytes: usize,
    budget: usize,
    failure: Option<String>,
}

impl Compilation {
    fn emit(&mut self, json: &str, progress: &Progress) -> Result<(), String> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if json.len() > self.budget.saturating_sub(self.estimated_bytes) {
            return Err(self.memory_error());
        }
        let event: Event = serde_json::from_str(json)
            .map_err(|error| format!("Invalid generated event: {error}"))?;
        // Account for the owned event, vector slack, duplicated validation graph
        // and its ID indexes. This is a conservative estimate, not a node quota.
        let owned = match &event {
            Event::Batch { nodes, edges } => {
                let node_bytes = nodes
                    .iter()
                    .enumerate()
                    .map(|(index, node)| {
                        if index & 1023 == 0 {
                            progress.tick();
                        }
                        (2 * std::mem::size_of_val(node)
                            + std::mem::size_of::<crate::model::Node>()
                            + 128)
                            .saturating_add(node.id.len().saturating_mul(4))
                            .saturating_add(node.label.as_ref().map_or(node.id.len(), String::len))
                            .saturating_add(node.color.len())
                    })
                    .fold(0usize, usize::saturating_add);
                let edge_bytes = edges
                    .iter()
                    .enumerate()
                    .map(|(index, edge)| {
                        if index & 1023 == 0 {
                            progress.tick();
                        }
                        (2 * std::mem::size_of_val(edge)
                            + std::mem::size_of::<crate::model::Edge>()
                            + 128)
                            .saturating_add(edge.id.len().saturating_mul(3))
                            .saturating_add(edge.source.len())
                            .saturating_add(edge.target.len())
                            .saturating_add(edge.color.len())
                    })
                    .fold(0usize, usize::saturating_add);
                node_bytes.saturating_add(edge_bytes)
            }
            Event::SetNode { id, color, .. } | Event::SetEdge { id, color, .. } => id
                .len()
                .saturating_add(color.as_ref().map_or(0, String::len)),
            Event::Wait { .. } => 0,
        };
        let estimated = self
            .estimated_bytes
            .checked_add(2 * std::mem::size_of::<Event>())
            .and_then(|value| value.checked_add(owned))
            .ok_or_else(|| self.memory_error())?;
        if estimated > self.budget {
            return Err(self.memory_error());
        }
        self.events
            .try_reserve(1)
            .map_err(|error| format!("Cannot allocate generated events: {error}"))?;
        self.graph
            .apply_with_progress(&event, || progress.tick())
            .map_err(|error| format!("Event {}: {error}", self.events.len() + 1))?;
        if let Event::Wait { ticks } = &event {
            self.total_ticks = self
                .total_ticks
                .checked_add(u64::from(*ticks))
                .ok_or_else(|| "Total simulation duration overflowed".to_string())?;
        }
        self.events.push(event);
        self.estimated_bytes = estimated;
        Ok(())
    }

    fn memory_error(&self) -> String {
        format!("Generated plan needs more memory than is currently available for compilation ({} MiB reserved from available RAM)", self.budget / 1024 / 1024)
    }
}

/// Compile JavaScript defining build(graph, p) or the legacy function* generate(N, params).
/// No filesystem, network, process, clock, unseeded randomness, or module loader is exposed.
#[cfg(test)]
pub fn compile_source(source: &str, parameters: Value, seed: u32) -> Result<Plan, String> {
    compile_with_budget(source, parameters, seed, MemoryBudget::detect()?, || {})
}

fn compile_with_budget(
    source: &str,
    parameters: Value,
    seed: u32,
    budget: MemoryBudget,
    report: impl FnMut() + 'static,
) -> Result<Plan, String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err("Rule source exceeds 1 MiB".into());
    }
    let controls = crate::experiment::parse_controls(source)?;
    let parameters = crate::experiment::merge_defaults(&controls, &parameters)?;
    let parameters_json = serde_json::to_string(&parameters).map_err(|error| error.to_string())?;
    if parameters_json.len() > MAX_PARAMETER_BYTES {
        return Err("Rule parameters exceed 256 KiB".into());
    }
    let runtime =
        Runtime::new().map_err(|error| format!("Cannot initialise rule runtime: {error}"))?;
    runtime.set_memory_limit(budget.heap);
    runtime.set_max_stack_size(512 * 1024);
    let progress = Progress::new(report);
    let last_progress = progress.last.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || {
        last_progress.get().elapsed() > NO_PROGRESS_TIMEOUT
    })));
    let compilation = Rc::new(RefCell::new(Compilation {
        events: Vec::new(),
        graph: Graph::new(seed),
        total_ticks: 0,
        estimated_bytes: 0,
        budget: budget.plan,
        failure: None,
    }));
    let context =
        Context::full(&runtime).map_err(|error| format!("Cannot create rule context: {error}"))?;
    context.with(|ctx| -> Result<(), String> {
        let state = compilation.clone();
        let emit_progress = progress.clone();
        let emit = Function::new(ctx.clone(), move |ctx: Ctx<'_>, json: String| {
            emit_progress.tick();
            let mut state = state.borrow_mut();
            let result = state.emit(&json, &emit_progress);
            emit_progress.tick();
            result.map_err(|error| {
                state.failure = Some(error.clone());
                Exception::throw_message(&ctx, &error)
            })
        }).map_err(|error| error.to_string())?;
        let checkpoint = Function::new(ctx.clone(), move || progress.tick())
            .map_err(|error| error.to_string())?;
        ctx.globals().set("__nodiform_emit", emit).map_err(|e| e.to_string())?;
        ctx.globals().set("__nodiform_checkpoint", checkpoint).map_err(|e| e.to_string())?;
        ctx.globals().set("__nodiform_source", source).map_err(|e| e.to_string())?;
        ctx.globals().set("__nodiform_params", parameters_json).map_err(|e| e.to_string())?;
        ctx.globals().set("__nodiform_seed", seed).map_err(|e| e.to_string())?;
        ctx.eval::<(), _>(RUNNER).map_err(|error| {
            if error.is_exception() {
                let exception = ctx.catch();
                let detail = exception.as_object().and_then(|object| {
                    let message = object.get::<_, String>("message").ok()?;
                    let stack = object.get::<_, String>("stack").unwrap_or_default();
                    Some(if stack.is_empty() { message } else { format!("{message}\n{stack}") })
                }).unwrap_or_else(|| format!("{exception:?}"));
                let detail: String = detail.chars().take(4_096).collect();
                format!("Rule error: {detail}. Available-memory budget: {} MiB JS heap; 512 KiB stack; generation must make progress at least every 5 seconds.", budget.heap / 1024 / 1024)
            } else { format!("Rule runtime error: {error}") }
        })
    })?;
    let mut state = compilation.borrow_mut();
    if let Some(error) = state.failure.take() {
        return Err(error);
    }
    Ok(Plan {
        events: std::mem::take(&mut state.events),
        total_ticks: state.total_ticks,
        node_count: state.graph.nodes.len(),
        edge_count: state.graph.edges.len(),
    })
}

// User code executes in a separate Function scope and cannot access the runner's counters.
// Yields and builder operations are snapshotted so later mutation cannot rewrite history.
const RUNNER: &str = concat!(
    r#"
(() => {
    'use strict';
    const source = globalThis.__nodiform_source;
    const parameters = JSON.parse(globalThis.__nodiform_params);
    let state = globalThis.__nodiform_seed >>> 0;
    delete globalThis.__nodiform_source;
    delete globalThis.__nodiform_params;
    delete globalThis.__nodiform_seed;
    const emitHost = globalThis.__nodiform_emit;
    const checkpointHost = globalThis.__nodiform_checkpoint;
    let operations = 0;
    function checkpoint() {
        operations += 1;
        if ((operations & 127) === 0) checkpointHost();
    }
    delete globalThis.__nodiform_emit;
    delete globalThis.__nodiform_checkpoint;
    const stringify = JSON.stringify;
    const parse = JSON.parse;
    const finite = Number.isFinite;
    const push = Function.prototype.call.bind(Array.prototype.push);
    const filter = Function.prototype.call.bind(Array.prototype.filter);
    const arrayFrom = Function.prototype.call.bind(Array.from, Array);
    const isArray = Array.isArray;
    const setHas = Function.prototype.call.bind(Set.prototype.has);
    const setAdd = Function.prototype.call.bind(Set.prototype.add);
    const toString = String;
    const freeze = Object.freeze;
    const define = Object.defineProperty;
    const ErrorType = Error;
    const stringifyCopy = value => parse(stringify(value, (_key, value) => {
        if (typeof value === 'number' && !finite(value)) {
            throw new ErrorType('Events cannot contain NaN or Infinity');
        }
        return value;
    }));
    const imul = Math.imul;
    // QuickJS's bare context has no host IO. Also remove its clock and default random source.
    for (const name of ['Date', 'performance', 'crypto', 'process', 'require', 'fetch',
                         'XMLHttpRequest', 'WebSocket', 'setTimeout', 'setInterval',
                         'SharedArrayBuffer', 'Atomics']) {
        define(globalThis, name, { value: undefined, configurable: false, writable: false });
    }
    define(Math, 'random', { value: undefined, configurable: false, writable: false });
    freeze(Math);
    const palette =
"#,
    include_str!("palette.js"),
    r#";
    let nextEdge = 0;
    const N = freeze({
        palette,
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
    function emit(value) {
        const event = stringify(value, (_key, value) => {
            if (typeof value === 'number' && !finite(value)) {
                throw new ErrorType('Events cannot contain NaN or Infinity');
            }
            return value;
        });
        if (event === undefined) throw new ErrorType('Every yield must contain an event');
        emitHost(event);
    }
    function id(value) {
        if (typeof value === 'string') return value;
        if (typeof value === 'number' && finite(value)) return toString(value);
        throw new ErrorType('Node and edge IDs must be strings or finite numbers');
    }
    let nodes = [], edges = [];
    const known = new Set();
    function flush() {
        if (nodes.length || edges.length) {
            emit(N.batch(nodes, edges));
            nodes = [];
            edges = [];
        }
    }
    const graph = freeze({
        add(value, options = {}) {
            const key = id(value);
            if (setHas(known, key)) throw new ErrorType(`Duplicate node ID '${key}'`);
            push(nodes, stringifyCopy(N.node(key, options)));
            setAdd(known, key);
            checkpoint();
            return key;
        },
        ids() { return arrayFrom(known); },
        others(value) {
            const key = id(value);
            return filter(arrayFrom(known), other => other !== key);
        },
        connect(from, targets, options = {}) {
            const source = id(from);
            const values = isArray(targets) ? targets : [targets];
            if (options.id !== undefined && values.length > 1) {
                throw new ErrorType('A custom edge ID can connect only one target');
            }
            const snapshot = stringifyCopy(options);
            const targetIds = [];
            for (let index = 0; index < values.length; index += 1) {
                push(targetIds, id(values[index]));
            }
            const added = [];
            for (const target of targetIds) {
                const edge = N.edge(source, target, snapshot);
                edge.id = id(edge.id);
                push(edges, edge);
                push(added, edge.id);
                checkpoint();
            }
            return added;
        },
        wait(ticks) { flush(); emit(N.wait(ticks)); },
        setNode(value, options = {}) { flush(); emit(N.setNode(id(value), options)); },
        setEdge(value, options = {}) { flush(); emit(N.setEdge(id(value), options)); },
        random: N.random,
        palette
    });
    const factory = new Function('N', 'params', '"use strict";\n' + source +
        '\nif (typeof generate === "function") return {builder:false,entry:generate};' +
        '\nif (typeof build === "function") return {builder:true,entry:build};' +
        '\nthrow new Error("Define function build(graph, p), or function* generate(N, params)");');
    const result = factory(N, parameters);
    const entry = result.entry;
    if (result.builder) {
        const value = entry(graph, parameters);
        if (value && (typeof value.then === 'function' || typeof value.next === 'function')) {
            throw new ErrorType('build must be an ordinary synchronous function');
        }
        flush();
    } else {
        const iterator = entry(N, parameters);
        if (!iterator || typeof iterator.next !== 'function') {
            throw new ErrorType('generate must return a synchronous generator');
        }
        for (;;) {
            const step = iterator.next();
            if (step && typeof step.then === 'function') {
                throw new ErrorType('Async generators are not supported; use function* generate');
            }
            if (!step || typeof step !== 'object') throw new ErrorType('Invalid iterator result');
            if (step.done) break;
            emit(step.value);
        }
    }
    return undefined;
})()
"#
);

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
    activity: Arc<Mutex<Instant>>,
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
        let budget = MemoryBudget::detect()?.plan;
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
        let activity = Arc::new(Mutex::new(Instant::now()));
        let reader_activity = activity.clone();
        std::thread::spawn(move || {
            let reader = ActivityReader {
                inner: stdout,
                remaining: budget,
                activity: reader_activity,
            };
            let _ = sender.send(read_worker_response(reader));
        });
        Ok(Self {
            child: Some(child),
            result,
            activity,
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
            Err(TryRecvError::Empty)
                if self.activity.lock().unwrap().elapsed() > WORKER_TIMEOUT =>
            {
                self.finish();
                Some(Err(
                    "Rule worker stopped reporting progress for 60 seconds".into()
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

/// Reader guard scales with the same available-memory budget as compilation;
/// incoming data is parsed directly, without collecting a second whole JSON plan.
struct ActivityReader<R> {
    inner: R,
    remaining: usize,
    activity: Arc<Mutex<Instant>>,
}

impl<R: Read> Read for ActivityReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let size = buffer.len().min(self.remaining.saturating_add(1));
        let count = self.inner.read(&mut buffer[..size])?;
        if count > self.remaining {
            return Err(std::io::Error::other(
                "Rule worker response exceeds available-memory budget",
            ));
        }
        self.remaining -= count;
        if count != 0 {
            *self.activity.lock().unwrap() = Instant::now();
        }
        Ok(count)
    }
}

fn read_worker_response(reader: impl Read) -> Result<Plan, String> {
    let mut reader = BufReader::new(reader);
    loop {
        let mut marker = [0u8; 2];
        reader
            .read_exact(&mut marker)
            .map_err(|error| format!("Cannot read rule worker response: {error}"))?;
        match &marker {
            b"P\n" => continue,
            b"R\n" => {
                return serde_json::from_reader::<_, Result<Plan, String>>(&mut reader)
                    .unwrap_or_else(|error| {
                        Err(format!(
                            "Rule worker stopped without a valid response: {error}"
                        ))
                    })
            }
            _ => return Err("Invalid rule worker progress message".into()),
        }
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
        compile_with_budget(
            &request.source,
            request.parameters,
            request.seed,
            MemoryBudget::detect()?,
            || {
                let mut output = std::io::stdout().lock();
                let _ = output.write_all(b"P\n").and_then(|_| output.flush());
            },
        )
    })();
    let status = i32::from(result.is_err());
    let mut output = std::io::stdout().lock();
    if output.write_all(b"R\n").is_err() {
        return 2;
    }
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

    fn palette_colours(count: usize) -> Vec<String> {
        let plan = compile_source(
            "function build(graph, p) { graph.palette(p.count).forEach((color, id) => graph.add(id, {color})); }",
            json!({"count": count}),
            1,
        ).unwrap();
        plan.events
            .into_iter()
            .flat_map(|event| match event {
                Event::Batch { nodes, .. } => nodes.into_iter().map(|node| node.color).collect(),
                _ => Vec::new(),
            })
            .collect()
    }

    // Measure the actual quantised sRGB outputs in Oklab, rather than
    // accepting generated hue labels as evidence of perceptual separation.
    fn oklab(hex: &str) -> [f64; 3] {
        let linear = |index| {
            let value = f64::from(u8::from_str_radix(&hex[index..index + 2], 16).unwrap()) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        let [r, g, b] = [linear(1), linear(3), linear(5)];
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        [
            0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
        ]
    }

    #[test]
    fn palettes_extend_beyond_the_former_node_limit() {
        assert!(palette_colours(0).is_empty());
        let colours = palette_colours(8_193);
        assert_eq!(colours.len(), 8_193);
        assert!(colours.iter().all(|colour| {
            colour.len() == 7
                && colour.starts_with('#')
                && colour.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
        }));
        // A fixed lightness/chroma circle has finitely many 8-bit outputs.
        // Very large palettes may repeat, but must keep the same colourfulness.
        let mut hue_sectors = [false; 12];
        for colour in &colours {
            let [lightness, a, b] = oklab(colour);
            assert!((lightness - 0.75015).abs() < 0.002, "{colour}: {lightness}");
            let chroma = a.hypot(b);
            assert!((chroma - 0.1275).abs() < 0.002, "{colour}: {chroma}");
            let hue = b.atan2(a).rem_euclid(std::f64::consts::TAU);
            hue_sectors[(hue / std::f64::consts::TAU * 12.0).floor() as usize] = true;
        }
        assert!(hue_sectors.into_iter().all(|covered| covered));
        // Each requested size spans the whole hue wheel rather than preserving
        // a categorical prefix at the cost of jumping around the rainbow.
        assert_ne!(&colours[..12], palette_colours(12));
        assert_ne!(&colours[..3], palette_colours(3));
    }

    #[test]
    fn palettes_follow_the_rainbow_even_after_hex_rounding() {
        for count in [1, 2, 3, 12, 128, 8_193] {
            let colours = palette_colours(count);
            let hues: Vec<_> = colours
                .iter()
                .map(|hex| {
                    let channel =
                        |index| f64::from(u8::from_str_radix(&hex[index..index + 2], 16).unwrap());
                    let [r, g, b] = [channel(1), channel(3), channel(5)];
                    // This chromaticity-plane angle independently preserves hue
                    // ordering without using the palette's RGB sector formula.
                    (3.0_f64.sqrt() * (g - b))
                        .atan2(2.0 * r - g - b)
                        .rem_euclid(std::f64::consts::TAU)
                })
                .collect();
            assert_eq!(colours.len(), count);
            assert_eq!(hues[0], 0.0);
            assert!(hues.windows(2).all(|pair| pair[0] <= pair[1]));
        }
    }

    #[test]
    fn palette_is_shared_by_both_apis_and_does_not_consume_randomness() {
        let baseline = compile_source(
            "function build(graph) { graph.add('a', {radius: graph.random() + 1}); graph.add('b', {radius: graph.random() + 1}); }",
            json!({}), 42,
        ).unwrap();
        let source = r#"function build(graph) {
            graph.add('a', {radius: graph.random() + 1});
            const colours = graph.palette(12);
            const unchanged = N.palette(12);
            if (JSON.stringify(colours) !== JSON.stringify(unchanged)) throw new Error('API mismatch');
            colours[0] = '#000000';
            colours.length = 0;
            if (JSON.stringify(graph.palette(12)) !== JSON.stringify(unchanged)) throw new Error('Mutated palette');
            const extended = N.palette(128);
            if (extended[1] === unchanged[1]) throw new Error('Palette did not respace hues');
            if (JSON.stringify(graph.palette(12)) !== JSON.stringify(unchanged)) throw new Error('Changed smaller palette');
            if (new Set(extended).size !== extended.length) throw new Error('Small palette repeats');
            graph.add('b', {radius: graph.random() + 1});
        }"#;
        assert_eq!(baseline, compile_source(source, json!({}), 42).unwrap());
        let legacy = "function* generate(N) { yield N.batch(N.palette(12).map((color, i) => N.node(String(i), {color}))); }";
        assert_eq!(
            compile_source(legacy, json!({}), 19).unwrap(),
            compile_source(legacy, json!({}), 42).unwrap()
        );
    }

    #[test]
    fn palette_rejects_invalid_counts_without_coercion() {
        for count in [
            "-1",
            "9007199254740992",
            "1.5",
            "NaN",
            "Infinity",
            "'3'",
            "null",
            "undefined",
            "{}",
        ] {
            let source = format!("function build(graph) {{ graph.palette({count}); }}");
            assert!(compile_source(&source, json!({}), 1)
                .unwrap_err()
                .contains("palette(count) requires a nonnegative safe integer"));
        }
    }

    #[test]
    fn small_palettes_are_vivid_and_perceptually_separated() {
        let colours: Vec<_> = palette_colours(12)
            .iter()
            .map(|colour| oklab(colour))
            .collect();
        for (index, colour) in colours.iter().enumerate() {
            assert!((colour[0] - 0.75015).abs() < 0.002);
            assert!((colour[1].hypot(colour[2]) - 0.1275).abs() < 0.002);
            for other in &colours[..index] {
                let squared: f64 = colour.iter().zip(other).map(|(a, b)| (a - b).powi(2)).sum();
                // Equally spaced hues give each neighbour the same intended
                // perceptual separation, with a little hex-rounding tolerance.
                assert!(squared.sqrt() > 0.063);
            }
        }
    }

    #[test]
    fn complete_growth_connects_each_birth_to_all_previous_nodes() {
        let plan = compile_source(
            include_str!("../examples/complete-growth.js"),
            json!({}),
            42,
        )
        .unwrap();
        assert_eq!(
            (plan.node_count, plan.edge_count, plan.total_ticks),
            (500, 124_750, 3_000)
        );
        assert_eq!(plan.events.len(), 1_000);
        for (index, birth) in plan.events.as_chunks::<2>().0.iter().enumerate() {
            let Event::Batch { nodes, edges } = &birth[0] else {
                panic!("expected birth batch");
            };
            let id = (index + 1).to_string();
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].id, id);
            assert_eq!(edges.len(), index);
            for (target, edge) in edges.iter().enumerate() {
                assert_eq!(edge.source, id);
                assert_eq!(edge.target, (target + 1).to_string());
                assert_ne!(edge.source, edge.target);
                assert_eq!(edge.strength, crate::model::DEFAULT_EDGE_STRENGTH);
            }
            assert_eq!(birth[1], Event::Wait { ticks: 6 });
        }
        // Worker serde round-trips preserve the materialised defaults.
        let bytes = serde_json::to_vec(&plan).unwrap();
        assert_eq!(plan, serde_json::from_slice::<Plan>(&bytes).unwrap());
    }

    #[test]
    fn starter_colour_controls_and_metadata_free_builders_work() {
        let plan = compile_source(include_str!("../examples/starter.js"), json!({}), 42).unwrap();
        assert_eq!(
            (plan.node_count, plan.edge_count, plan.total_ticks),
            (8, 7, 192)
        );
        let controls =
            crate::experiment::parse_controls(include_str!("../examples/starter.js")).unwrap();
        let colour = &controls["colorBy"];
        assert_eq!(colour.kind, crate::experiment::ControlKind::Select);
        assert_eq!(colour.default, json!("birth-order"));
        assert_eq!(colour.options, ["birth-order", "alternating", "single"]);
        let arbitrary = "function build(graph, p) { for(const id of p.names) graph.add(id); graph.connect(p.names[2], graph.ids().slice(0,2)); graph.wait(99); }";
        assert!(crate::experiment::parse_controls(arbitrary)
            .unwrap()
            .is_empty());
        let plan = compile_source(arbitrary, json!({"names":["gamma","alpha","beta"]}), 1).unwrap();
        let Event::Batch { nodes, edges } = &plan.events[0] else {
            panic!();
        };
        assert_eq!(
            nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            ["gamma", "alpha", "beta"]
        );
        assert_eq!(
            edges.iter().map(|e| e.target.as_str()).collect::<Vec<_>>(),
            ["gamma", "alpha"]
        );
        assert_eq!(plan.total_ticks, 99);
    }

    #[test]
    fn legacy_generator_can_use_an_unrelated_build_helper() {
        let source = r#"
            function build(value) { return `node:${value}`; }
            function* generate(N, params) {
                for (const value of params.values) {
                    yield N.batch([N.node(build(value))]);
                }
            }
        "#;
        let plan = compile_source(source, json!({"values":["a","b"]}), 1).unwrap();
        let ids = plan
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Batch { nodes, .. } => Some(nodes[0].id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, ["node:a", "node:b"]);
    }

    #[test]
    fn legacy_generator_can_bind_graph_in_its_own_source_scope() {
        let source = r#"
            const graph = ['left', 'right'];
            function* generate(N) {
                yield N.batch(graph.map(id => N.node(id)));
            }
        "#;
        let plan = compile_source(source, json!({}), 1).unwrap();
        let Event::Batch { nodes, .. } = &plan.events[0] else {
            panic!("expected node batch");
        };
        assert_eq!(
            nodes
                .iter()
                .map(|node| node.id.as_str())
                .collect::<Vec<_>>(),
            ["left", "right"]
        );
    }

    #[test]
    fn builder_uses_captured_intrinsics_for_snapshots_arrays_and_duplicate_checks() {
        let source = r#"
            JSON.parse = () => ({ id: 'corrupted' });
            Array.isArray = () => false;
            Set.prototype.has = () => false;
            function build(graph) {
                graph.add('a');
                graph.add('b');
                graph.connect('b', ['a']);
            }
        "#;
        let plan = compile_source(source, json!({}), 1).unwrap();
        assert_eq!((plan.node_count, plan.edge_count), (2, 1));
        let Event::Batch { nodes, edges } = &plan.events[0] else {
            panic!("expected graph batch");
        };
        assert_eq!(nodes[0].id, "a");
        assert_eq!(edges[0].target, "a");

        let duplicate = r#"
            Set.prototype.has = () => false;
            function build(graph) { graph.add('same'); graph.add('same'); }
        "#;
        assert!(compile_source(duplicate, json!({}), 1)
            .unwrap_err()
            .contains("Duplicate node ID"));
    }

    #[test]
    fn build_styles_edits_numeric_ids_and_snapshots() {
        let source = r#"function build(graph) {
            const opts = {color:'#ff0000',position:[1,2]};
            graph.add(1,opts); opts.color='#0000ff'; opts.position[0]=999;
            graph.add('2',opts);
            const edges = graph.connect(2, graph.others(2), {color:'#12345678',strength:0.3});
            if (graph.ids().join(',') !== '1,2') throw new Error('order');
            graph.wait(5);
            graph.setEdge(edges[0], {strength:0.5,color:'#ffffff'});
            graph.setNode(2,{color:'#00ff00',radius:2});
        }"#;
        let plan = compile_source(source, json!({}), 1).unwrap();
        let Event::Batch { nodes, edges } = &plan.events[0] else {
            panic!();
        };
        assert_eq!(nodes[0].color, "#ff0000");
        assert_eq!(nodes[0].position, Some([1.0, 2.0]));
        assert_eq!(edges[0].color, "#12345678");
        assert_eq!(edges[0].strength, 0.3);
        let mut graph = Graph::new(1);
        for event in &plan.events {
            graph.apply(event).unwrap();
        }
        assert_eq!(graph.nodes[0].label, "1");
        assert_eq!(graph.nodes[1].radius, 2.0);
        assert_eq!(graph.edges[0].strength, 0.5);
    }

    #[test]
    fn build_defaults_are_local_to_source_and_invalid_controls_block_execution() {
        let source = r#"/* @controls {"twigs":{"type":"integer","label":"Twigs","default":3,"min":1,"max":9}} */
        function build(graph,p) { for(let i=0;i<p.twigs;i++) graph.add(i); }"#;
        assert_eq!(compile_source(source, json!({}), 1).unwrap().node_count, 3);
        assert_eq!(
            compile_source(source, json!({"twigs":5}), 1)
                .unwrap()
                .node_count,
            5
        );
        assert!(compile_source(source, json!({"twigs":-1}), 1)
            .unwrap_err()
            .contains("twigs"));
        assert!(compile_source(
            "/* @controls { */ function build(graph) { graph.add('a'); }",
            json!({}),
            1
        )
        .unwrap_err()
        .contains("@controls"));
        assert_eq!(
            compile_source("function build(graph) {graph.add('a');}", json!({}), 1)
                .unwrap()
                .node_count,
            1
        );
    }

    #[test]
    fn declared_defaults_apply_to_generator_scripts_too() {
        let source = r#"/* @controls {"twigs":{"type":"integer","label":"Twigs","default":3,"min":1,"max":9}} */
        function* generate(N,p) { for(let i=0;i<p.twigs;i++) yield N.batch([N.node(String(i))]); }"#;
        assert_eq!(compile_source(source, json!({}), 1).unwrap().node_count, 3);
        assert_eq!(
            compile_source(source, json!({"twigs":5}), 1)
                .unwrap()
                .node_count,
            5
        );
    }

    #[test]
    fn build_rejects_ambiguous_ids_nonfinite_data_and_async_work() {
        for source in [
            "function build(g) { g.add(1); g.add('1'); }",
            "function build(g) { g.add({}); }",
            "function build(g) { g.add(Infinity); }",
            "function build(g) { g.add(1,{radius:NaN}); }",
            "function build(g) { g.add(1); g.add(2); g.connect(1,[1,2],{id:'same'}); }",
            "async function build(g) { g.add(1); }",
            "function* build(g) { g.add(1); }",
        ] {
            assert!(compile_source(source, json!({}), 1).is_err(), "{source}");
        }
    }

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
            json!({"alphabet": "AB", "maxLength": 2, "repetitions": true, "ticksPerNode": 7, "finalTicks": 0}), 1).unwrap();
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
        // Saved v0.1.1 projects carry source without a controls header. They keep
        // their own defaults and alias precedence; no metadata means no injection.
        let legacy_source = source.split_once("*/").unwrap().1;
        let legacy = compile_source(
            legacy_source,
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
    fn growing_ring_closes_and_uses_seed_independent_rainbow_colours() {
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
        assert_eq!(plan, compile_source(source, json!({}), 19).unwrap());
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
        let budget = MemoryBudget {
            heap: 4 * 1024 * 1024,
            plan: 4 * 1024 * 1024,
        };
        assert!(compile_with_budget(
            "function* generate() { yield new ArrayBuffer(16 * 1024 * 1024); }",
            json!({}),
            0,
            budget,
            || {},
        )
        .is_err());
        assert!(compile_with_budget(
            "function* generate() { yield* generate(); }",
            json!({}),
            0,
            budget,
            || {},
        )
        .is_err());
    }

    #[test]
    fn streamed_events_pass_former_count_and_json_limits() {
        let plan = compile_source(
            r#"function* generate(N) {
            for (let i = 0; i < 17000; i++) {
                yield N.batch([N.node(String(i), {label: 'x'.repeat(1024)})]);
            }
            for (let i = 0; i < 100001; i++) yield N.wait(1);
        }"#,
            json!({}),
            0,
        )
        .unwrap();
        assert_eq!(plan.node_count, 17_000);
        assert_eq!(plan.events.len(), 117_001);
        assert_eq!(plan.total_ticks, 100_001);
        assert!(serde_json::to_vec(&plan).unwrap().len() > 16 * 1024 * 1024);
    }

    #[test]
    fn generated_plan_respects_resource_budget_without_count_quota() {
        let result = compile_with_budget(
            "function* generate(N) { for(let i=0; i<100; i++) yield N.batch([N.node(String(i))]); }",
            json!({}), 0, MemoryBudget { heap: 4 * 1024 * 1024, plan: 4096 }, || {},
        );
        assert!(result
            .unwrap_err()
            .contains("more memory than is currently available"));
    }

    #[test]
    fn checkpoints_refresh_progress_and_native_callbacks_are_private() {
        let progress = Progress::new(|| {});
        progress.last.set(Instant::now() - Duration::from_secs(30));
        assert!(progress.last.get().elapsed() > NO_PROGRESS_TIMEOUT);
        progress.tick();
        assert!(progress.last.get().elapsed() < NO_PROGRESS_TIMEOUT);
        let plan = compile_source(r#"function build(graph) {
            if (typeof globalThis.__nodiform_emit !== 'undefined' ||
                typeof globalThis.__nodiform_checkpoint !== 'undefined') throw new Error('host leak');
            for(let i=0; i<10000; i++) graph.add(i);
        }"#, json!({}), 0).unwrap();
        assert_eq!(plan.node_count, 10_000);
    }

    #[test]
    fn worker_progress_protocol_streams_result_and_guards_memory() {
        let plan = Plan {
            events: vec![Event::Wait { ticks: 3 }],
            total_ticks: 3,
            node_count: 0,
            edge_count: 0,
        };
        let mut bytes = b"P\nP\nR\n".to_vec();
        serde_json::to_writer(&mut bytes, &Ok::<_, String>(&plan)).unwrap();
        let activity = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(90)));
        let reader = ActivityReader {
            inner: bytes.as_slice(),
            remaining: bytes.len(),
            activity: activity.clone(),
        };
        assert_eq!(read_worker_response(reader).unwrap(), plan);
        assert!(activity.lock().unwrap().elapsed() < WORKER_TIMEOUT);
        let reader = ActivityReader {
            inner: bytes.as_slice(),
            remaining: 4,
            activity,
        };
        assert!(read_worker_response(reader)
            .unwrap_err()
            .contains("available-memory budget"));
        assert!(read_worker_response(&b"P\nR\n{incomplete"[..]).is_err());
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
