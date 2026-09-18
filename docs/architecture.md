# Architecture and experiment semantics

Nodiform separates the experiment, its playback, the GPU solver, and the encoder. The native desktop interface uses egui/eframe; GPU work uses wgpu. The same offscreen graph image is suitable for the live view and video readback.

## Rule compilation and the event model

A separate process evaluates the source with QuickJS. It receives the source, inputs, and seed, then returns a bounded finite event plan or an error. The preferred `build(graph, p)` API collects additions into batches and flushes them at waits, updates, and function completion. Existing `function* generate(N, params)` programs can still emit events directly. Both produce the same validated event model. This keeps a stuck rule program from directly blocking the user interface and allows cancellation. Runtime limits are defence in depth, not a hardened operating-system sandbox for untrusted code. Manifests identify these semantics as `nodiform-rules-v2`; the legacy generator interface remains compatible.

An optional leading JSON `/* @controls {...} */` comment defines script-specific input controls. The application knows control types, not experiment families or special keys such as alphabet and node count. Defaults fill absent input keys; supplied values and undeclared keys are preserved. The Rules and Inputs tabs edit one experiment definition. The native egui interface uses light cards, rounded controls, and blue accents; it does not use Apple UI frameworks or implement system backdrop blur.

The model validates stable IDs, colour values, finite numbers, endpoints, and graph size. A batch is transactional. Style and edge-strength changes are explicit events, so their place in the sequence is reproducible. Validation applies the plan to a model before the live run starts.

Generation order and simulation time are independent. Mutation events take no ticks; wait events consume ticks. The live GPU state is not visible to the generator. This first design supports scheduled dynamic rules, not feedback rules based on current positions.

## GPU state and forces

Node positions live in GPU buffers. Graph updates append birth positions and update style or connectivity without overwriting existing nodes' evolved positions. The solver alternates read and write position buffers so one tick uses a coherent input state.

Repulsion is evaluated over all pairs with short-distance softening. Attraction is gathered from incident edges using their current nonnegative strengths. Springs have zero rest length. Motion is overdamped and limited per step for numerical stability. There is no hidden gravity or centring force.

The numerical mobility per tick is `min(1/120, 0.5 / maximum incident strength sum)`, using `1/120` when there is no positive attraction. Increasing edge weights can therefore reduce the step size for the whole graph. This is a stability precaution for the force update, not a change to video frame rate or event scheduling. The displacement cap remains a separate safeguard; neither guarantees monotonic energy descent.

The exact all-pairs calculation costs O(N²) for repulsion. Edge attraction uses adjacency data rather than a dense edge matrix. The current caps of 8,192 nodes and 250,000 edges bound resource use; GPU buffers reserve these capacities at initialisation. They are not throughput targets. A future approximate solver would need explicit accuracy and reproducibility controls rather than silently replacing this model.

Generation retains the full finite plan before playback. Its separate limits include a 64 MiB QuickJS heap, a 512 KiB stack, 16 MiB of generated event JSON, and 100,000 events, along with instruction/time budgets. A graph within the model caps can exceed these generation limits. The supplied complete-growth experiment fits 500 nodes and 124,750 edges, but dense rendering and exact repulsion still impose a substantial runtime cost.

The shader renders edges and antialiased circular nodes. Node radii are world-space sizes. The camera derives a frame from the graph bounds, including node radii and a margin. Changing that frame affects presentation, not forces or stored positions.

## Connection-based node sizing

**Settings → Size nodes by connections** multiplies each script-defined base radius by:

```text
max(1, 3 × sqrt(d + 1) / 8)
```

Here `d` is the sum of incoming and outgoing unique directed connections. Parallel edges with the same source and target count once. Reciprocal connections count separately, and a self-loop contributes two. Edge strength does not affect this display count. The force solver still uses every edge and its strength.

The curve follows the normalised sizing behaviour inspected in the official [Obsidian 1.13.7 application archive](https://github.com/obsidianmd/obsidian-releases/releases/download/v1.13.7/obsidian-1.13.7.asar.gz), with its upper size cap deliberately omitted. The inspected radius before normalisation was a node multiplier times `max(8, min(3 × sqrt(d + 1), 30))`. Nodiform divides by the baseline of 8 and removes the upper clamp. Its multiplier is 1 through degree 6, 1.5 at degree 15, and approximately 8.385 at degree 499. This multiplies the rule's radius rather than replacing it.

The option changes presentation only: force calculations and stored node radii are unchanged, displayed radii are included in auto-fit bounds, and nodes still shrink on screen as the camera zooms out. It defaults to off and is saved in the project. Older files lacking the setting load with it off. It may change during preview; it is locked while a recording is being prepared, recorded, or finalised so the run retains its captured appearance settings.

## What “minimum configuration” means here

Repulsion and attraction can compete to create relaxed structures without introducing a preferred centre. However, a finite-step numerical solver is not a proof of minimisation. It does not guarantee that energy decreases on every tick, that motion fully stops, or that the final state is a global minimum.

Disconnected components have no attractive connection holding them at a finite distance. With continuing all-pair repulsion and no gravity, they can separate indefinitely. A zero-strength edge does not bind components. Auto-fit keeps an expanding graph visible but does not solve that physical non-equilibrium. Bounded experiments on disconnected graphs remain useful; interpret continued separation correctly.

Order, birth positions, pauses between events, and the final relaxation interval can all affect the observed structure. Cross-GPU floating-point execution can change trajectories, even when the event plan and seed match. The seed guarantees the intended deterministic rule randomness and default birth placement, not universal bit-identical physics.

## Fixed-timeline recording

A formal run captures its source and parameters instead of reading ongoing editor changes. Fixed simulation ticks determine output samples. Wall-clock rendering speed is not the simulation clock.

RGBA frames are read from the GPU with row-padding handled explicitly, then streamed to FFmpeg through a bounded queue. When that queue fills, playback must retain the pending sample and stop advancing until the encoder accepts it. This backpressure is what makes slower-than-real-time recording possible without intentionally omitting timeline samples.

The selected encoder is probed off the UI thread before a recording starts. FFmpeg writes an MKV file directly. A run manifest records configuration and provenance alongside the output. A separate `run-outcome.json` distinguishes completion of the planned simulation timeline from an early stop; successful encoder finalisation alone does not mean the full experiment ran. Encoder completion is asynchronous; stopping simulation is not the same as having a finalised video. No intermediate PNG sequence or second full-resolution frame archive is needed.

Container choice and streaming improve crash tolerance but do not guarantee recovery. Keep adequate disk space and wait for finalisation. Simulation checkpoints, restart-from-checkpoint, and interrupted-recording resume are not implemented.

## Validation boundaries

Unit tests can verify event validation, deterministic plan generation, rule limits, shader syntax, and encoder handling. They do not establish GPU performance, driver correctness, or desktop integration on a particular machine. Bazzite/Wayland testing and actual GPU measurements must be reported separately from a successful Linux build.
