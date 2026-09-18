# Nodiform

A native laboratory for emergent graphs. Write rules, choose the order in which a graph grows, and watch its structure develop in two dimensions.

Nodiform is an experimental desktop alpha, designed with Bazzite Linux in mind. Version 0.1.1 adds an AppImage with desktop integration and an update channel through Gear Lever. Testing on an actual Bazzite machine and large-graph performance measurements remain outstanding.

## What this version does

- Runs editable JavaScript generators that create nodes and edges in a precise sequence.
- Applies code-defined colours, world-space node radii, and edge strengths, including changes later in a run.
- Calculates repulsion and weighted attraction on the GPU, and renders the graph on the GPU.
- Fits the whole graph into the view. Nodes become smaller on screen as the camera zooms out.
- Provides a syntax-coloured rule editor, line numbers, snippets, starter examples, and an in-app API reference.
- Offers a quick preview and a formal **Run & Record** path that streams fixed-timeline video to FFmpeg.

The included ABC experiment generates ordered strings and connects each longer string to its contiguous shorter substrings: `ABC` connects to `AB` and `BC`. Repeated letters are a separate choice, not an implicit meaning of “combinations.” The Growing ring starter builds a chain, then closes it into a ring without prearranging the nodes in a circle.

## Run the desktop app

Download **[Nodiform-x86_64.AppImage](https://github.com/bodee-angus/nodiform/releases/latest/download/Nodiform-x86_64.AppImage)** from the [latest release](https://github.com/bodee-angus/nodiform/releases/latest). On Bazzite, install **Gear Lever** from **Bazaar**, then open the AppImage with Gear Lever and integrate it into your application menu. You can then find **Nodiform** through desktop search. Bazzite recommends Gear Lever for managing AppImages. See the [installation and update guide](docs/appimage.md) for details. [Bazzite documentation](https://docs.bazzite.gg/Installing_and_Managing_Software/AppImage/)

This x86_64 Linux package targets current Bazzite and uses an Ubuntu 24.04 build environment. Compatibility with older distributions is not promised. The AppImage contains the application, examples, documentation, and icon; it does not bundle GPU drivers or FFmpeg. It is a portable package, not a security sandbox.

A working Vulkan-capable GPU driver is required. FFmpeg must be available on the host's `PATH` for recording, including the selected encoder. Preview does not require FFmpeg. The software encoding option uses `libx264`; `h264_nvenc` requires a compatible NVIDIA driver and FFmpeg build.

On an immutable distribution such as Bazzite, use your preferred supported method to make FFmpeg available to the environment launching Nodiform. Installing it only inside an unrelated container does not make it available to a host-launched application.

The older portable `.tar.gz` archive remains usable: extract it and open `nodiform` from your file manager, enabling executable permission if necessary. That archive does not provide AppImage update integration.

### A first experiment

1. Start with **ABC permutations** or **Growing ring** and inspect its JavaScript. Loading a starter asks before replacing current edits.
2. Expand **Experiment parameters** for the seed and simple ABC controls, including alphabet, maximum length, repeated letters, and generation order. **All parameters · JSON** exposes the complete input.
3. Use **Validate** to check the rules before starting a run.
4. Use **Preview** to explore the result without recording.
5. Use **Run & Record** for a recorded experiment. A run uses a snapshot of its source, parameters, and seed. The selected encoder is checked in the background before playback begins.
6. Pause, advance one output-frame interval with **Step**, or stop using the run controls. Let recording finalisation finish before closing the app.

The rule editor is the source of truth. Parameters are inputs to your program, not an alternative hidden rule system. Settings start collapsed to leave room for the code. Starting a run uses the current editor content; it does not reload a starter. See the [rule guide](docs/rules.md) for a small complete example.

Use **Open** and **Save** to keep a `.nodiform.json` project containing the source, parameters, seed, and run/recording settings. This saves an experiment definition, not the current moving graph or a resume checkpoint. A folder chooser sets the recording destination; the default is `Videos/Nodiform` under your home directory, with a separate uniquely named folder for each run. Runs write `simulation.mkv` and `manifest.json`; `run-outcome.json` records whether the simulation completed its planned timeline or stopped early.

The initial app settings use 1280 × 720 output at 60 fps, four solver ticks per frame, and 240 extra relaxation ticks after the generated plan. Higher output resolutions increase readback bandwidth, encoding cost, and disk use without changing the graph's rules.

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

The first solver evaluates every pair of nodes, so repulsion costs **O(N²)** per tick. This build limits graphs to **8,192 nodes and 100,000 edges**. Those are validation limits, not a promise of interactive frame rates. Dense graphs can be expensive well below them.

Forces consist of softened repulsion and weighted zero-rest-length attraction. There is no centring gravity. The camera follows the graph without applying a force to it. Disconnected attraction components can therefore drift apart indefinitely. Zero-strength edges do not hold components together.

The solver seeks relaxed configurations but does not promise a global minimum, monotonic energy reduction on every discrete step, or identical floating-point trajectories across different GPUs and drivers. The creation order and waits are deliberate parts of an experiment. See [architecture and scientific caveats](docs/architecture.md).

Recording streams frames into an MKV file with bounded buffering. It does not create a directory of PNG images. Simulation time is based on fixed ticks, not elapsed wall time: a slow encoder or GPU makes the run take longer instead of deliberately skipping recorded frames. Long recordings still need substantial disk space. MKV improves interruption tolerance, but a crash or full disk can still leave an incomplete or unusable file.

This alpha does **not** yet provide Barnes–Hut repulsion, resumable simulation checkpoints, a packaged FFmpeg runtime, or a complete debugger. Rules produce a finite event plan before playback; they cannot inspect the evolving GPU positions or react to live solver state. The JavaScript runtime is constrained for resource safety, but it is not a security boundary for running untrusted downloaded programs.

## Project status

Rule/model tests, shader validation, and FFmpeg recording tests have passed in the development environment. Check the [Actions results](https://github.com/bodee-angus/nodiform/actions) for each build's GPU and native-window smoke-test results. Those automated checks use software Vulkan and do not establish interactive performance on Bazzite or NVIDIA hardware.

The repository is public. No open-source licence has been selected yet; public visibility alone does not grant a licence to redistribute or modify the project.
