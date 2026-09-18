# Install Nodiform on Bazzite

Nodiform 0.1.6 is an **experimental alpha** for x86_64 Linux. The AppImage gives it a self-contained application file, desktop-menu entry, and a Gear Lever update source. Experiments and recordings stay outside the application file. This packaging does not sandbox Nodiform or change its alpha status.

## Install with Gear Lever

1. Open **Bazaar**, search for **Gear Lever**, and install it. This is Bazzite's recommended AppImage manager. [Bazzite guide](https://docs.bazzite.gg/Installing_and_Managing_Software/AppImage/)
2. Download **[Nodiform-x86_64.AppImage](https://github.com/bodee-angus/nodiform/releases/latest/download/Nodiform-x86_64.AppImage)** from [Nodiform's latest release](https://github.com/bodee-angus/nodiform/releases/latest).
3. Open Gear Lever and drag the downloaded file into its window, or open the file with Gear Lever from your file manager. Follow its prompts to integrate Nodiform into the application menu. Gear Lever can keep managed AppImages in a chosen folder, so the application need not remain in Downloads. [Gear Lever features](https://github.com/mijorus/gearlever#features)
4. Search for **Nodiform** in your desktop's application launcher. Try **Preview** with the small chain shown in **Rules**. Other experiments are under **Experiment → Examples**.

If execution is blocked by file permissions, enable executable permission in the AppImage's file properties and try again. After integration, confirm Gear Lever's managed copy launches before removing any duplicate download. Keep the managed file in place.

### What is included

The AppImage contains Nodiform, its icon, examples, and documentation. It uses the host's Linux libraries and Vulkan GPU driver. Release builds use Ubuntu 24.04 as their build basis; compatibility with older Linux distributions is not promised. Actual Bazzite hardware testing remains outstanding.

The AppImage runtime's upstream licence notice is included in [third-party/appimage-runtime-LICENSE.txt](third-party/appimage-runtime-LICENSE.txt). Its [corresponding source and build instructions](https://github.com/AppImage/type2-runtime/tree/dd6cebedcbddde9c82f89b011e8e1d40b6e43868) are available from AppImage. This notice applies to that third-party runtime, not to Nodiform's own source.

**Preview needs no video encoder.** Recording needs an `ffmpeg` executable visible on the host application's `PATH`, with `libx264` or the selected `h264_nvenc` encoder. FFmpeg and GPU drivers are not bundled. An FFmpeg installation confined to an unrelated container will not be found by a host-launched Nodiform.

## Apply updates

Open Nodiform's entry in Gear Lever to check for and apply updates. When given a choice, replace the previous application version rather than keeping both to save space. Gear Lever manages the download and desktop integration; Nodiform does not install background updates itself. Bazzite notes that Gear Lever can check for updates, while applying them remains a user action. [Bazzite guide](https://docs.bazzite.gg/Installing_and_Managing_Software/AppImage/)

Version 0.1.6 uses the same update channel as earlier versions, so an existing Gear Lever installation does not need a new source configured. It adds **connected-branch-walk** to the **Letter permutations** example. Load that example, then choose the new option under **Inputs → Birth order**. Graph storage still grows without the former 8,192-node and 250,000-edge caps removed in 0.1.5. The force model and automatic birth placement from 0.1.4 are unchanged. Available memory and the GPU’s actual limits still apply; the exact solver can become very slow as node counts grow.

Older saved experiments and generator scripts remain supported, but their source is preserved. If an older script contains an `8192` guard or a count control with a fixed `max`, that script still restricts itself. Choose the updated example from **Experiment → Examples**, or edit its guard and control yourself. Save any current edits before loading a replacement example.

The preview matches its on-screen physical pixel size, including desktop scaling, and has a black background. Video output resolution remains independent. **Settings → Appearance** still chooses System, Light or Dark, remembered independently of your experiment files. Connection-based node sizing remains uncapped and starts off for projects that do not contain that setting.

The AppImage embeds this update source:

```text
gh-releases-zsync|bodee-angus|nodiform|latest|Nodiform-x86_64.AppImage.zsync
```

If your Gear Lever version does not detect the embedded source, configure its GitHub update source with these fields:

| Field | Value |
| --- | --- |
| Repo | `bodee-angus/nodiform` |
| Release file name | `Nodiform-x86_64.AppImage` |

These field names follow Gear Lever's version 4 documentation. No pre-release opt-in is required for this channel. [Gear Lever GitHub updates](https://gearlever.mijorus.it/docs/github-updates/)

The channel follows GitHub's latest regular release. Releases are labelled **Experimental Alpha** in their titles and notes, but are published as regular GitHub releases so this update channel can discover them. That GitHub classification is not a claim of production readiness. Future public updates use a new application version; published release assets are not silently replaced.

### Your data stays separate

**Experiment → Save…** writes a `.nodiform.json` experiment wherever you choose. Recordings default to `Videos/Nodiform` under your home directory; **Settings → Choose video folder…** selects another destination. Replacing the AppImage leaves these files alone. Removing Nodiform through Gear Lever also leaves your saved experiments and videos to manage separately.

Only saved experiments survive closing the app. An AppImage update does not preserve unsaved editor changes or a live simulation, so save your work and let recording finalisation finish before updating.

## If it does not launch

First check that the download is the **x86_64 AppImage**, that it can be executed, and that your system has a working Vulkan driver. The release includes `SHA256SUMS` for checking downloaded files. A checksum verifies file consistency, not publisher identity on its own.

If an error specifically reports a FUSE/mount problem, this diagnostic runs without FUSE:

```sh
./Nodiform-x86_64.AppImage --appimage-extract-and-run
```

Use that from a terminal in the AppImage's folder only for troubleshooting. It temporarily extracts the payload and needs additional disk space. Normal Gear Lever installation does not require keeping an extracted copy.

For a concise version check:

```sh
./Nodiform-x86_64.AppImage --version
```

`--smoke-test` opens the native window, runs a short graph check, reports its result, and exits. It needs a display and Vulkan; it is intended for diagnostics and CI, not a benchmark or a substitute for testing normal use.

## Build and release

After a successful x86_64 Linux release build, run:

```sh
cargo build --release --locked
bash packaging/package-appimage.sh
```

The packaging script needs `curl`, `sha256sum`, `install`, `awk`, `readelf`, GNU `sed` and `timeout`, ordinary GNU command-line utilities, and `zsync` for update metadata. It verifies that the built executable's `--version` matches `Cargo.toml`, downloads pinned AppImage tools into `target/appimage-tools`, and verifies their SHA-256 hashes before execution. The build-tool invocation works without FUSE.

Local outputs are `dist/Nodiform-x86_64.AppImage`, its `.zsync` update file, and `dist/SHA256SUMS`, which covers both files. The script refuses to overwrite those paths; move an earlier build aside before rebuilding. The release workflow publishes all three files. Each `.zsync` file points to its matching versioned release asset, while the embedded update channel discovers the latest release.

Release maintainers must bump `Cargo.toml` and the lockfile's package version for a new public build. Keep an existing release's assets unchanged. The release channel is for explicitly versioned builds, not every development commit. Review automated test results before publishing; software-Vulkan and virtual-display checks do not establish real Bazzite/NVIDIA compatibility.
