# Release changes

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
