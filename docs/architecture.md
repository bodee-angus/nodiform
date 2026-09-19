# Architecture and experiment semantics

Nodiform separates the experiment, its playback, the GPU solver, and the encoder. The native desktop interface uses egui/eframe; GPU work uses wgpu. Preview and video use the same render pipeline with independent offscreen images and camera histories.

## Rule compilation and the event model

A separate process evaluates the source with QuickJS. It receives the source, inputs, and seed, then returns a validated finite event plan or an error. The preferred `build(graph, p)` API collects additions into batches and flushes them at waits, updates, and function completion. Existing `function* generate(N, params)` programs can still emit events directly. Both produce the same validated event model. This keeps a stuck rule program from directly blocking the user interface and allows cancellation. Runtime limits are defence in depth, not a hardened operating-system sandbox for untrusted code. Manifests identify the current semantics as `nodiform-rules-v6`; the legacy generator interface remains compatible.

An optional leading JSON `/* @controls {...} */` comment defines script-specific input controls. The application knows control types, not experiment families or special keys such as alphabet and node count. Defaults fill absent input keys; supplied values and undeclared keys are preserved. The Rules and Inputs tabs edit one experiment definition. The native egui interface supports System, Light and Dark themes. This application preference is persisted separately from project definitions and does not affect the offscreen graph renderer or video colours.

The model validates stable IDs, colour values, finite numbers, endpoints, and representable graph indices. A batch is transactional. Style and edge-strength changes are explicit events, so their place in the sequence is reproducible. Validation applies the plan to a model before the live run starts.

Generation order and simulation time are independent. Mutation events take no ticks; wait events consume ticks. The live GPU state is not visible to the generator. This first design supports scheduled dynamic rules, not feedback rules based on current positions.

## GPU state and forces

Node positions live in GPU buffers. Graph updates append birth positions and update style or connectivity without overwriting existing nodes' evolved positions. The solver alternates read and write position buffers so one tick uses a coherent input state. A separate momentum buffer stores each node's preceding capped displacement; newly created nodes begin with zero momentum. Storage starts small and grows geometrically with the graph. Growth copies both position buffers and momentum on the GPU, preserving evolved state and the preview and export camera histories.

The `live-neighbour-centroid-v2` birth policy resolves new nodes in insertion order on the GPU. A node without an explicit `position` starts at the unweighted mean of its connected predecessors' **current** positions, plus a seed-and-ID-derived offset in `[-1, 1)` world units on each axis. Parallel and reciprocal edges count as one neighbour for this mean; edge strength, including zero, does not affect placement. A node without connected predecessors uses the world origin plus the same small offset. This offset reduces exact coincidences, where repulsion has no direction. Explicit coordinates bypass automatic placement exactly.

Playback coalesces mutations at the same simulated time before synchronising the graph. Birth placement uses all connections present at that synchronisation, but only lower insertion indices can anchor a new node. This includes earlier births in the same synchronisation; later births cannot anchor earlier ones. A connection added after a positive wait cannot relocate a node already born. The GPU pass writes each birth to both position buffers without reading positions back to the CPU or moving existing nodes.

Repulsion is evaluated over all pairs with short-distance softening. Attraction is gathered from incident edges using their current nonnegative strengths. Springs have zero rest length. Motion uses damped momentum and a displacement limit. There is no hidden gravity or centring force.

`nodiform-force-v3` uses repulsion 512, softening squared 0.25, maximum displacement 2 per tick, default edge strength 4, and momentum retention 0.85. Version 0.1.4 halved repulsion relative to 0.1.3; these force semantics remain unchanged in 0.1.9. Explicit script strengths are honoured. Each tick adds the force-driven increment to 85% of the previous capped displacement, caps the resulting movement, then stores that capped movement as the next tick's momentum. This is displacement memory per tick, not velocity in world units per second; storing the capped result prevents momentum from accumulating behind the cap.

For an isolated pair with one edge of strength `s > 0`, a nonzero stationary equilibrium satisfies `distance² = 512 / s − 0.25`, when positive. This pair calculation does not predict the edge lengths of a general graph or guarantee that a discrete trajectory converges. Previously saved experiments run with the current force model and birth policy; manifests record their versions and the force coefficients.

The force multiplier per tick is `min(1/120, 0.5 / maximum incident strength sum)`, using `1/120` when there is no positive attraction. Increasing edge weights can therefore reduce the force-driven increment for the whole graph. This is a stability precaution for the force update, not a change to video frame rate or event scheduling. Momentum can carry nodes past a relaxed configuration while damping reduces that motion. The displacement cap remains a separate safeguard; neither guarantees monotonic energy descent.

The exact all-pairs calculation costs O(N²) for repulsion. Edge attraction uses adjacency data rather than a dense edge matrix. Version 0.1.5 removes the fixed 8,192-node and 250,000-edge caps. Actual device storage-buffer binding sizes, buffer sizes, compute dispatch limits, and 32-bit shader indices still bound addressable graph storage. Preflight checks report hardware capacity errors before playback or recording; GPU allocation errors report insufficient memory. These checks cannot guarantee that a graph will run interactively. A future approximate solver would need explicit accuracy and reproducibility controls rather than silently replacing this model.

Generation retains the complete finite plan before playback. A private native emit callback snapshots, parses and validates each event immediately, avoiding a second full event array and giant JSON string inside QuickJS. Worker output is decoded incrementally with a memory-derived byte guard. There is no fixed node, edge or event count ceiling and no fixed 16 MiB generated-JSON ceiling.

The generation budget uses current Linux `MemAvailable`, constrained by remaining memory under cgroup-v2 limits when present. One quarter is assigned to the QuickJS heap and one quarter to a conservative estimate of the event plan and validation graph; the remaining half leaves room for the application, IPC and operating system. These are resource budgets, not guarantees against memory pressure changing during a run. The 1 MiB source limit, 256 KiB parameter limit, 2 MiB worker request limit, and 512 KiB QuickJS stack limit remain.

The QuickJS watchdog permits long finite generation while rule API calls make progress; it interrupts after five seconds without that progress. The parent process separately watches for 60 seconds without worker pipe activity, refreshing the deadline as chunks arrive. Compilation remains cancellable. The supplied complete-growth experiment defaults to 500 nodes and 124,750 edges, but users can increase its count; dense rendering and exact repulsion still impose a substantial runtime cost.

The shader renders edges and antialiased circular nodes. Node radii are world-space sizes. The camera derives a frame from the graph bounds, including node radii and a margin. Changing that frame affects presentation, not forces or stored positions.

Gradient edges sample the live node-style buffer at both endpoints and interpolate linear-light RGB and alpha along their length. The edge colour's alpha multiplies the endpoint alpha; its RGB is ignored in gradient mode. A later node colour change therefore updates connected gradients without rewriting edge events. Solid colour remains the default for older edges. Preview and recording use the same render pipeline. A project-level edge-thickness multiplier scales both the 0.18-world-unit base width and a one-physical-pixel minimum. Screen width is `max(0.18 / world_units_per_pixel, 1) × multiplier`. Edges shrink with zoom until the minimum is reached; their length and node radii still scale normally. The slider range is 0.25×–8×, giving a corresponding 0.25–8-pixel minimum. Existing analytic antialiasing and gradient opacity remain active; no supersampled framebuffer or multisample texture is added. Width is independent of edge strength. Recording metadata identifies the display policy as `world-with-pixel-floor-v1`. Old projects default to 1×. Preview can change it live; recording locks it to the captured value.

## Perceptual palettes

`graph.palette(count)` and `N.palette(count)` are bundled offline helpers returning sRGB hex strings. The implementation uses [Oklab](https://bottosson.github.io/posts/oklab/) and its cylindrical OKLCH form. Every sample uses one common lightness and chroma chosen to keep the full hue circle inside sRGB. Only hue varies, avoiding per-colour lightness adjustments or chroma compression. This targets similar perceived lightness and vividness rather than constant HSL saturation, which is not a perceptually uniform measure. Conversion to 8-bit hex produces small quantisation differences.

Samples use evenly spaced OKLCH hues from the sRGB red axis, then sort the quantised hex values by conventional RGB hue to preserve rainbow order despite rounding. Lightness is 0.75015 and chroma is 0.1275, just inside the numerically determined maximum common full-circle sRGB chroma. Count-dependent hue spacing means increasing the count changes previous colours. Returned arrays are independent, and palette generation does not draw from the experiment's random stream. The fixed-lightness/chroma circle has finitely many 8-bit hex representations; sufficiently large palettes can repeat colours and cannot keep all categories visually distinct. Counts must be nonnegative safe integers and the returned array must fit available memory and JavaScript array representation limits.

Version 0.1.8 replaces the categorical hue sequence with rainbow order and identifies the change with `nodiform-rules-v6`. Existing source is preserved, but any rerun that calls the helper uses the new palette; literal hex values are unaffected. The force and birth-policy versions do not change. Toroidal-grid dimensions are script-level connectivity coordinates only, so adding that example does not introduce higher-dimensional physics or rendering.

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

Order, birth positions, pauses between events, and the final relaxation interval can all affect the observed structure. Cross-GPU floating-point execution can change trajectories, even when the event plan and seed match. The seed fixes rule randomness and each automatic birth offset. The complete birth position also depends on its connected predecessors, their evolving positions, and timing; it is no longer an ID-and-seed-only coordinate. Matching a seed does not guarantee universal bit-identical physics.

## Live preview

The preview texture matches the canvas's physical pixel dimensions: logical UI size multiplied by the current pixels-per-point scale. It updates after a window resize or scale change, even while paused. If either dimension would exceed the GPU texture limit, both dimensions scale down together to preserve the aspect ratio. The clear colour is opaque black in both preview and recording. Preview and export have independent textures and camera histories, so preview resizing and display refresh cadence cannot alter recorded resolution or camera smoothing.

Interactive playback targets `video fps × ticks per frame` solver ticks per wall-clock second, which is 240 with the defaults. It requests a repaint each available display frame and carries fractional ticks between updates. The previous frame-rate gate reset its timer after each sample, discarding fractional elapsed time; combined with the old solver's lack of momentum, that could make motion feel sluggish.

Catch-up considers at most 100 ms of elapsed time and advances at most 240 ticks per UI update. Pausing, resuming and stepping reset the clock. Sustained overload therefore slows the preview instead of accumulating unbounded catch-up work; it does not skip rule events or solver ticks within the simulated timeline. These limits apply to preview pacing, not to the fixed samples written for recording.

## Timeline progress

Progress uses completed solver ticks divided by the compiled plan’s total waits plus the application’s final settling interval. It includes every birth wait and the script’s own final wait. A finished zero-tick plan reports completion; trailing zero-time mutations must be applied before the display reports 100%. Pausing leaves progress fixed and an early stop retains its actual fraction. Compilation has no percentage until the plan is known. Encoder finalisation is a distinct status after simulation progress reaches its end.

Time remaining is estimated from recent active wall time per completed simulation tick. Observations include GUI rendering, GPU stalls and encoder backpressure. A short warm-up avoids estimating from one frame, and smoothing reduces frame-to-frame jumps. Pauses, manual steps while paused, rule compilation and encoder preparation do not count as active playback time. The estimate uses all remaining ticks, including script and application settling. It adapts to changing speed, but cannot foresee the full cost of later graph growth or encoding finalisation; the UI marks it as approximate and shows finalisation without a numerical ETA.

## Fixed-timeline recording

A formal run captures its source and parameters instead of reading ongoing editor changes. Fixed simulation ticks determine output samples at the configured video resolution, independently of preview dimensions. Wall-clock rendering speed is not the simulation clock. Recording advances an output sample when the encoder can accept it, rather than waiting for a display-frame timer.

RGBA frames are read from the GPU with row-padding handled explicitly, then streamed to FFmpeg through a bounded queue. When that queue fills, playback must retain the pending sample and stop advancing until the encoder accepts it. This backpressure is what makes slower-than-real-time recording possible without intentionally omitting timeline samples.

The selected encoder is probed off the UI thread before a recording starts. FFmpeg writes an MKV file directly. A run manifest records configuration and provenance alongside the output. A separate `run-outcome.json` distinguishes completion of the planned simulation timeline from an early stop; successful encoder finalisation alone does not mean the full experiment ran. Encoder completion is asynchronous; stopping simulation is not the same as having a finalised video. No intermediate PNG sequence or second full-resolution frame archive is needed.

Container choice and streaming improve crash tolerance but do not guarantee recovery. Keep adequate disk space and wait for finalisation. Simulation checkpoints, restart-from-checkpoint, and interrupted-recording resume are not implemented.

## Validation boundaries

Unit tests can verify event validation, deterministic plan generation, rule limits, shader syntax, and encoder handling. They do not establish GPU performance, driver correctness, or desktop integration on a particular machine. Bazzite/Wayland testing and actual GPU measurements must be reported separately from a successful Linux build.
