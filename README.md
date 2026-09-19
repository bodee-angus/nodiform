# Nodiform

A native laboratory for emergent graphs. Write rules, choose the order in which a graph grows, and watch its structure develop in two dimensions.

Nodiform is an experimental desktop alpha, designed with Bazzite Linux in mind. Version 0.1.7 adds prime-factor, pi-digit, divisor, and toroidal-grid experiments, and changes generated palettes to vary hue at constant perceptual lightness and chroma. GPU storage grows with the graph without fixed node and edge caps; available memory and the GPU’s actual buffer and indexing limits still determine what can run. The AppImage uses the existing Gear Lever update channel. Testing on an actual Bazzite machine and large-graph performance measurements remain outstanding. See the [release changes](CHANGELOG.md).

## What this version does

- Runs editable JavaScript rules that create nodes and edges in a precise sequence.
- Grows graph storage as needed, without the former 8,192-node or 250,000-edge caps.
- Builds optional input controls from each script, without assuming an alphabet, node count, or graph family.
- Applies code-defined colours, world-space node radii, and edge strengths, including changes later in a run.
- Generates hex palettes with similar vividness, varying only hue in OKLCH before conversion to sRGB, with `graph.palette(count)` or `N.palette(count)`.
- Blends gradient edges between their endpoint colours, following later node colour changes.
- Places each new node near its already-created neighbours using their current positions, or near the world origin when none exist. Scripts can override the initial position.
- Calculates repulsion and weighted attraction on the GPU, with damped momentum, and renders the graph on the GPU.
- Fits the whole graph into the view. Nodes become smaller on screen as the camera zooms out.
- Provides a syntax-coloured rule editor, line numbers, snippets, starter examples, and an in-app API reference.
- Offers **Preview** at the canvas’s physical pixel resolution and **Record** at an independent output resolution; recording streams fixed-timeline video to FFmpeg.
- Optionally sizes nodes by connection count without changing the simulation's forces.

The default experiment is a small eight-node chain with no required inputs. **Experiment → Examples** contains **Connect to every earlier node**, **Connect to the previous half**, **Letter permutations**, **Growing ring**, **Modular residues**, **Prime factors**, **Digits of pi**, **Divisor graph**, and **Toroidal grid**. Both numbered growth examples default to 500 nodes: connecting to every earlier node creates 124,750 edges, while connecting to the previous half creates 62,500. These are editable experiments, not fixed application modes.

**Toroidal grid** takes **Dimension** and **Range**. Each coordinate wraps back to 1, so dimension 1 makes a cycle and dimension 2 makes a grid with wrapped rows and columns. The default dimension 2 and range 10 produce 100 nodes and 200 edges. Higher logical dimensions add connections, while the force simulation and renderer remain 2D. Node colours identify the first coordinate. See the [example rules](docs/rules.md#included-experiments-and-limits).

Generated colours now share one OKLCH lightness and chroma; only hue varies. Small deviations remain after conversion to 8-bit hex. Saved script source is preserved, but rerunning a script that calls `palette` uses the new colours. Literal hex colours are unchanged.

For letter permutations, choose **Inputs → Birth order → connected-branch-walk** to create each prefix before its descendants. With a maximum length of at least two, every node after the first connects to the existing graph at birth. The final graph is the same as with the other birth orders. See the [permutation examples](docs/rules.md#included-experiments-and-limits).

The native egui interface offers **Settings → Appearance → System / Light / Dark**, with matching editor colours and controls. The theme is remembered separately from experiments and does not change recorded graph colours. Rounded cards and blue accents surround a pure black simulation canvas.

## Run the desktop app

Download **[Nodiform-x86_64.AppImage](https://github.com/bodee-angus/nodiform/releases/latest/download/Nodiform-x86_64.AppImage)** from the [latest release](https://github.com/bodee-angus/nodiform/releases/latest). On Bazzite, install **Gear Lever** from **Bazaar**, then open the AppImage with Gear Lever and integrate it into your application menu. You can then find **Nodiform** through desktop search. Bazzite recommends Gear Lever for managing AppImages. See the [installation and update guide](docs/appimage.md) for details. [Bazzite documentation](https://docs.bazzite.gg/Installing_and_Managing_Software/AppImage/)

This x86_64 Linux package targets current Bazzite and uses an Ubuntu 24.04 build environment. Compatibility with older distributions is not promised. The AppImage contains the application, examples, documentation, and icon; it does not bundle GPU drivers or FFmpeg. It is a portable package, not a security sandbox.

A working Vulkan-capable GPU driver is required. FFmpeg must be available on the host's `PATH` for recording, including the selected encoder. Preview does not require FFmpeg. The software encoding option uses `libx264`; `h264_nvenc` requires a compatible NVIDIA driver and FFmpeg build.

On an immutable distribution such as Bazzite, use your preferred supported method to make FFmpeg available to the environment launching Nodiform. Installing it only inside an unrelated container does not make it available to a host-launched application.

The older portable `.tar.gz` archive remains usable: extract it and open `nodiform` from your file manager, enabling executable permission if necessary. That archive does not provide AppImage update integration.

### A first experiment

1. Open the **Rules** tab and inspect the eight-node starter, or choose **Experiment → Examples**. Loading an example asks before replacing current edits.
2. Use **Inputs** for any controls declared by that script. **Advanced · Input JSON** exposes all stored inputs, including custom arrays and objects. A script without declared controls can keep its values directly in code.
3. Use **Validate** to check the rules before starting a run.
4. Use **Preview** to explore the result without recording.
5. Use **Record** for a recorded experiment. A run uses a snapshot of its source, inputs, seed, and settings. The selected encoder is checked in the background before playback begins.
6. Pause, advance one output-frame interval with **Step**, or stop using the run controls. Let recording finalisation finish before closing the app.

The rule editor is the source of truth. Define `function build(graph, p)` and use ordinary loops to describe creation, connections, and waits. The older `function* generate(N, params)` API remains supported. Starting a run uses the current editor content; it does not reload an example. See the [rule guide](docs/rules.md) for complete examples.

**Settings** contains the random seed, final settling interval, video settings, and **Size nodes by connections**. That sizing option affects appearance only, remains proportional to the script's radius, and still shrinks with zoom. It can change during preview and is locked during recording. **Inspect** opens the inspector for graph details.

Use **Experiment → Open…** and **Save…** to keep a `.nodiform.json` project containing the source, inputs, seed, and run/recording settings. This saves an experiment definition, not the current moving graph or a resume checkpoint. Older project files still open; connection-based sizing defaults to off if absent. **Choose video folder…** in Settings sets the recording destination; the default is `Videos/Nodiform` under your home directory, with a separate uniquely named folder for each run. Runs write `simulation.mkv` and `manifest.json`; `run-outcome.json` records whether the simulation completed its planned timeline or stopped early.

The initial app settings use 1280 × 720 video output at 60 fps, four solver ticks per frame, and 240 extra relaxation ticks after the generated plan. Preview targets 240 solver ticks per second with these settings, retaining fractional elapsed ticks between display refreshes. A slow frame has bounded catch-up work, so sustained overload slows playback instead of building an ever-growing queue.

The preview matches the canvas’s physical pixel dimensions, including desktop scaling, independently of the video resolution. Resizing the window or moving it between differently scaled displays updates the preview. Extremely large canvases are scaled down to the GPU texture limit. Higher video resolutions increase readback bandwidth, encoding cost, and disk use without changing preview sharpness or graph rules.

## Build from source

Install a current stable Rust toolchain and the native Linux build dependencies. For example, on Ubuntu 24.04:

```sh
sudo apt-get update
sudo apt-get install build-essential pkg-config libxkbcommon-dev libwayland-dev libx11-dev libudev-dev ffmpeg mesa-vulkan-drivers
git clone https://github.com/bodee-angus/nodiform.git
cd nodiform
cargo test --locked
cargo run --release --locked
```

On Bazzite, a development container is a useful place to build without changing the base system. Package names differ by distribution. Running a GUI or accessing the GPU from inside a container requires that container's display and device integration; the commands above do not configure it.

To make the AppImage after building:

```sh
cargo build --release --locked
bash packaging/package-appimage.sh
```

The packaging script downloads pinned, checksum-verified AppImage tooling and creates the application plus update metadata in `dist/`. It refuses to overwrite existing artifacts. See [packaging details](docs/appimage.md#build-and-release) for dependencies and release policy. The older portable archive can still be created with:

```sh
bash packaging/package-linux.sh
```

The build workflow includes unit tests, software-Vulkan GPU tests, and a native-window smoke test, then packages the Linux application. A successful CI build is not evidence of driver compatibility or interactive testing on a Bazzite machine.

The archive also includes an optional desktop-entry template. Its `Exec=nodiform` assumes the executable is on your desktop session's `PATH`; it is not a portable relative-path launcher. You can open the executable itself without installing that template.

## Physics and recording limits

There is no application-selected node or edge count cap. GPU buffers grow as needed while preserving existing positions and momentum. The device’s storage-buffer, buffer-size and dispatch limits, shader index ranges, and available RAM and GPU memory still bound a run. The application checks hardware capacity before playback or recording and reports allocation failures.

The solver still evaluates every pair of nodes, so repulsion costs **O(N²)** per tick: doubling the node count roughly quadruples the pair work. Removing count caps does not make larger graphs fast. Dense graphs also require quadratically many edges. Longer-than-real-time recording preserves the timeline, but cannot remove those computation and memory costs.

Updated examples no longer impose the former graph count ceilings. **Saved or custom scripts retain their own guards**, including any `8192` check or input `max` already in their source. Load an updated example, or edit those guards yourself; updating the application does not rewrite your experiments.

Forces consist of softened repulsion and weighted zero-rest-length attraction. There is no centring gravity. The camera follows the graph without applying a force to it. Disconnected attraction components can therefore drift apart indefinitely. Zero-strength edges do not hold components together.

The `nodiform-force-v3` model introduced in 0.1.4 remains unchanged: repulsion is 512, default edge strength is 4, and each tick retains 85% of the preceding capped movement before adding the next force-driven increment. Preview timing carries fractional elapsed time between frame boundaries. Explicit edge strengths remain unchanged.

Automatic births use the unweighted centre of connected nodes created earlier in the sequence, plus a small deterministic offset of less than or equal to one world unit per axis. With no such neighbours they start near `[0, 0]`. Connections created after a simulation wait do not reposition existing nodes. The seed fixes the offset, while birth order, waits, and the neighbours’ live positions also determine the resulting location. Supply `position: [x, y]` to choose an exact initial location.

The solver seeks relaxed configurations but does not promise a global minimum, monotonic energy reduction on every discrete step, or identical floating-point trajectories across different GPUs and drivers. The creation order and waits are deliberate parts of an experiment. See [architecture and scientific caveats](docs/architecture.md).

Recording streams frames into an MKV file with bounded buffering. It does not create a directory of PNG images. Simulation time is based on fixed ticks, not elapsed wall time: a slow encoder or GPU makes the run take longer instead of deliberately skipping recorded frames. Long recordings still need substantial disk space. MKV improves interruption tolerance, but a crash or full disk can still leave an incomplete or unusable file.

This alpha does **not** yet provide Barnes–Hut repulsion, resumable simulation checkpoints, a packaged FFmpeg runtime, or a complete debugger. Rules produce a finite event plan before playback; they cannot inspect the evolving GPU positions or react to live solver state. Generation budgets derive from available memory, and progress watchdogs allow longer finite compilations while stopping stalled code. You can cancel generation. The JavaScript runtime is constrained for resource safety, but it is not a security boundary for running untrusted downloaded programs.

## Project status

Rule/model tests, shader validation, and FFmpeg recording tests have passed in the development environment. Check the [Actions results](https://github.com/bodee-angus/nodiform/actions) for each build's GPU and native-window smoke-test results. Those automated checks use software Vulkan and do not establish interactive performance on Bazzite or NVIDIA hardware.

The repository is public. No open-source licence has been selected yet; public visibility alone does not grant a licence to redistribute or modify the project.
