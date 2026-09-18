# Release changes

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
