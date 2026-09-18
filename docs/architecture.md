# Architecture and experiment semantics

Nodiform separates the experiment, its playback, the GPU solver, and the encoder. The native desktop interface uses egui/eframe; GPU work uses wgpu. The same offscreen graph image is suitable for the live view and video readback.

## Rule compilation and the event model

A separate process evaluates the source with QuickJS. It receives the source, parameters, and seed, then returns a bounded finite event plan or an error. This keeps a stuck generator from directly blocking the user interface and allows cancellation. Runtime limits are defence in depth, not a hardened operating-system sandbox for untrusted code.

The model validates stable IDs, colour values, finite numbers, endpoints, and graph size. A batch is transactional. Style and edge-strength changes are explicit events, so their place in the sequence is reproducible. Validation applies the plan to a model before the live run starts.

Generation order and simulation time are independent. Mutation events take no ticks; wait events consume ticks. The live GPU state is not visible to the generator. This first design supports scheduled dynamic rules, not feedback rules based on current positions.

## GPU state and forces

Node positions live in GPU buffers. Graph updates append birth positions and update style or connectivity without overwriting existing nodes' evolved positions. The solver alternates read and write position buffers so one tick uses a coherent input state.

Repulsion is evaluated over all pairs with short-distance softening. Attraction is gathered from incident edges using their current nonnegative strengths. Springs have zero rest length. Motion is overdamped and limited per step for numerical stability. There is no hidden gravity or centring force.

The numerical mobility per tick is `min(1/120, 0.5 / maximum incident strength sum)`, using `1/120` when there is no positive attraction. Increasing edge weights can therefore reduce the step size for the whole graph. This is a stability precaution for the force update, not a change to video frame rate or event scheduling. The displacement cap remains a separate safeguard; neither guarantees monotonic energy descent.

The exact all-pairs calculation costs O(N²) for repulsion. Edge attraction uses adjacency data rather than a dense edge matrix. The current caps of 8,192 nodes and 100,000 edges bound resource use; they are not throughput targets. A future approximate solver would need explicit accuracy and reproducibility controls rather than silently replacing this model.

The shader renders edges and antialiased circular nodes. Node radii are world-space sizes. The camera derives a frame from the graph bounds, including node radii and a margin. Changing that frame affects presentation, not forces or stored positions.

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
