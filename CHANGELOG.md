# Release changes

## 0.1.9

- Edges now retain a minimum screen width when zoomed out: one physical pixel at the default thickness. The existing slider scales both world width and the pixel floor. Analytic antialiasing, gradients and opacity apply in previews and videos; nodes continue to scale with zoom.
- Added approximate time remaining to preview and recording progress, based on recent active playback speed. Estimates include all remaining waits and settling ticks, adapt to slowdown, and exclude pauses. Preparation and video finalisation retain separate statuses.
- Added **Grid**, a nonwrapping counterpart to Toroidal grid with the same Dimension, Range and colour modes. Dimension 2 and Range 10 produce 100 nodes and 180 edges; endpoints do not wrap across boundaries. All simulation and rendering remain 2D.

The minimum-width display policy is recorded as `world-with-pixel-floor-v1`. Saved projects keep their thickness setting. Rule API, palette, force model and birth policy remain unchanged. The existing Gear Lever update channel continues to work.

## 0.1.8

- Added a persistent **Edge thickness** slider in Settings → Appearance, from 0.25× to 8×. It affects preview and video independently of force strengths, updates live in preview, and is frozen during recording. Older projects default to 1×.
- Added timeline progress for preview and recording, including every script wait and both script and application final settling intervals. Completed and stopped progress remains visible; video finalisation has a separate status.
- Added **Colour by** settings to every example, including the starter. Structural categories receive successive rainbow colours in order of first appearance; birth-order modes traverse the rainbow as nodes arrive.
- Changed palettes to rainbow hue order, with count-dependent spacing. Fixed OKLCH lightness 0.75015 and chroma 0.1275 approach the maximum common sRGB chroma without changing lightness or chroma between hues. The chroma increase from 0.1.7 is small; equal perceptual vividness limits maximum saturation across all hues.
- Added **Fibonacci chain**: maximum 20 creates nodes 0–20, then adds the Fibonacci links 3–5, 5–8 and 8–13 after the sequential chain. Self-loops and duplicate chain edges are omitted.

The rule API is now `nodiform-rules-v6`. Palette calls use the new rainbow ordering, and increasing the palette count no longer preserves a prefix. Saved source and literal hex colours are unchanged. Force and birth policies remain `nodiform-force-v3` and `live-neighbour-centroid-v2`. The existing Gear Lever update channel continues to work.

## 0.1.7

- Added **Prime factors**, with edge strength multiplied by each prime factor's exponent and an optional shared node-1 anchor. Prime nodes omit self-connections; 1 is not a prime factor.
- Added **Digits of pi**, calculating exact digits with BigInt arithmetic. Ten digit hubs connect to occurrence nodes, which also form a chain in digit order. The digit count includes the initial 3.
- Added **Divisor graph**, connecting each number to all its smaller positive divisors. It requests one palette colour per node and sorts those colours by hue.
- Added **Toroidal grid**, with **Dimension** and **Range** inputs. Coordinates wrap along each axis; the default 2 dimensions and range 10 create 100 nodes and 200 edges. Higher logical dimensions still use the 2D force solver. Duplicate connections are removed for range 2; range 1 creates a single node without self-loops.
- Reworked `graph.palette(count)` and `N.palette(count)` to vary only hue at a fixed OKLCH lightness and chroma chosen to fit the full hue circle in sRGB. Generated colours have similar vividness without deliberately varying lightness or chroma. Hex quantisation introduces small deviations. The sequence remains deterministic and prefix-stable, with no fixed colour-count ceiling; sufficiently large palettes can repeat hex codes.

The rule API is now `nodiform-rules-v5`. Saved scripts keep their source, but calls to `palette` use the new colours when rerun; earlier palette values are not preserved. Literal hex colours, `nodiform-force-v3`, and `live-neighbour-centroid-v2` are unchanged. The AppImage continues to use the existing Gear Lever update channel.

## 0.1.6

- Added **connected-branch-walk** to the bundled **Letter permutations** example's **Birth order** input. It retains alternating branch traversal but creates every prefix before its descendants, preventing temporary disconnected islands when the maximum length is at least two.
- For `ABC` without repetition, the new order is `A, AB, ABC, AC, ACB, B, BA, BAC, BC, BCA, C, CA, CAB, CB, CBA`. The final nodes, contiguous prefix/suffix edges, colours, strengths and timing are unchanged.
- The original **branch-walk** and layer orders remain available. A maximum length of one still produces separate letters with no edges, as defined by the experiment. No scaffold connections are added.

Load **Experiment → Examples → Letter permutations**, then choose **Inputs → Birth order → connected-branch-walk**. Saved scripts retain their own source; updating the application does not rewrite them. Graph storage, forces and automatic birth placement are unchanged from 0.1.5. The AppImage continues to use the existing Gear Lever update channel.

## 0.1.5

- Added **Connect to the previous half**: node `n` connects to the most recent `floor(n / 2)` earlier nodes. It defaults to 500 nodes and 62,500 edges, with script-defined inputs and no fixed count ceiling.
- Removed the fixed 8,192-node and 250,000-edge caps. GPU storage now grows with the graph, preserving existing positions and momentum. Actual device buffer, dispatch and index limits still apply, and allocation failures report insufficient memory.
- Reworked rule generation to validate each emitted event directly rather than retaining a second, large JSON plan inside JavaScript. Heap and plan budgets now derive from available memory; fixed event-count and generated-JSON ceilings are removed. Watchdogs track lack of progress instead of limiting every compilation to a short total duration.
- Removed the bundled examples’ former count ceilings and permutation length ceiling. Permutation traversal uses an explicit stack; lexicographic and reverse layers stream their words. The existing branch-walk order is unchanged.
- Removed the palette helper’s 8,192-colour ceiling and the Inputs panel’s implicit numeric ceiling. Script-declared input bounds and numeric representation limits remain. Larger palettes preserve existing colour prefixes, but may repeat hex codes.

Existing saved and custom scripts keep their own source, including any `8192` guard or input `max`. Load an updated example or edit those restrictions explicitly. The rule API is now `nodiform-rules-v4`; `nodiform-force-v3` and `live-neighbour-centroid-v2` are unchanged. The exact O(N²) solver remains, so larger graphs can be very slow and must fit available RAM and GPU memory. The AppImage continues to use the existing Gear Lever update channel.

## 0.1.4

- New nodes without an explicit position appear near the current centre of their already-created connected neighbours, with a small deterministic offset. With no such neighbours, they start near the world origin. Later connections do not teleport existing nodes; explicit initial positions remain supported.
- Reduced repulsion from 1024 to 512 while retaining default edge strength 4. The `nodiform-force-v3` solver retains 85% of the preceding capped displacement for more fluid, damped motion.
- Fixed preview pacing to carry fractional elapsed ticks between display refreshes, with bounded catch-up after a stall. Default preview speed remains 240 solver ticks per second; recording preserves every fixed timeline sample.
- Preview now renders at the canvas's physical pixel resolution, including desktop scaling, independently of video settings. Resizing and display scaling update it even while paused.
- Changed the simulation background to pure black. Preview and export use independent textures and camera histories, so resizing the preview does not affect a recording's framing history or output resolution.

Existing projects and rule scripts remain supported. Reruns use the new force model and `live-neighbour-centroid-v2` birth policy, so their trajectories can differ from earlier versions. New recording manifests identify both policies. The AppImage continues to use the existing Gear Lever update channel.

## 0.1.3

- Added System, Light and Dark appearance, remembered between launches, including matching editor syntax colours and input controls.
- Increased repulsion from 64 to 1024 and default edge strength from 1 to 4. Explicit script strengths are preserved. Runs use the versioned `nodiform-force-v2` model.
- Added `graph.palette(count)` and `N.palette(count)`, returning vivid, deterministic hex colours using Oklab/OKLCH. They work offline and do not consume seeded rule randomness.
- Added `gradient: true` on edges, blending live endpoint colours. Changing a node colour updates its connected gradients in previews and recordings.
- Refreshed every example with vibrant node colours. Permutations use one generated colour per starting letter and gradient edges.
- Added the permutation example's **branch-walk** birth order: `A, AB, ABC, ACB, AC, …`. Edges wait until both endpoints exist, preserving the final graph across birth orders.

Existing projects and generator scripts remain supported. Earlier experiments now use the stronger repulsion; the force version and coefficients are recorded in new video manifests.

## 0.1.2

- General-purpose rule editor with optional controls declared by each script.
- Added the simple `build(graph, p)` API while retaining existing generators.
- Added the complete-growth example with 500 nodes and 124,750 edges.
- Refined the native workspace and added connection-based node sizing using Obsidian's square-root curve without an upper cap.

## 0.1.1

- Added the AppImage package and the Gear Lever update channel.
