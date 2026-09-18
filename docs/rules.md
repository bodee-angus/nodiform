# Writing a graph experiment

Rules are real JavaScript, evaluated by QuickJS in a separate worker process. Define `function* generate(N, params)` and yield graph events. The application validates the resulting finite plan before playback.

There is no TypeScript compiler or language server in this alpha. Syntax colouring and the API reference help with editing; **Validate** checks execution and graph consistency.

## A complete small example

```js
function* generate(N, params) {
  yield N.batch([
    N.node("A", { color: "#79dfc7" }),
    N.node("B", { color: "#a6bfff" })
  ], []);

  yield N.wait(120);

  yield N.batch([
    N.node("AB", { color: "#f4c97a", radius: 2 })
  ], [
    N.edge("AB", "A", { id: "AB-A", strength: 1 }),
    N.edge("AB", "B", { id: "AB-B", strength: 1 })
  ]);

  yield N.wait(240);
  yield N.setEdge("AB-A", { strength: 2, color: "#f4c97acc" });
  yield N.wait(240);
}
```

An edge can refer to nodes already created or nodes in the same batch. If any item in a batch is invalid, none of that batch is applied.

## Helpers

| Helper | Meaning |
| --- | --- |
| `N.node(id, options)` | Construct a node specification for a batch. |
| `N.edge(source, target, options)` | Construct an edge specification for a batch. Use an explicit `id` if you will update it. |
| `N.batch(nodes, edges)` | Create the supplied nodes and edges together. Yield this event. |
| `N.wait(ticks)` | Advance the force simulation by a specified number of ticks before the next event. |
| `N.setNode(id, options)` | Change a node's colour or radius at this point in the sequence. |
| `N.setEdge(id, options)` | Change an edge's colour or strength at this point in the sequence. |
| `N.random()` | Draw from the run's seeded pseudorandom sequence. |

Node options are `label`, `color`, `radius`, and an optional initial `position: [x, y]`. Edge options are `id`, `color`, and `strength`. Colours accept `#RRGGBB` or `#RRGGBBAA`. Radius must be greater than zero and at most 1,000,000 world units. Strength must be between zero and 1,000,000; zero turns off that edge's attraction without deleting it. Each birth coordinate must have an absolute value of at most 1,000,000. All numeric values must be finite. These input bounds prevent extreme values from overflowing GPU arithmetic; typical experiments should use much smaller values.

IDs are stable references. Node IDs must be unique among nodes and edge IDs unique among edges. Edges are attractive connections between their endpoints, not directed physical forces; the `source` and `target` names identify endpoints. Multiple differently named edges between two nodes add their attractions together.

`N.node` and `N.edge` return specifications, not events. Put them inside `N.batch`; do not yield an individual specification directly.

## Order and time are separate

The generator's yield order determines event order. Creating or updating graph elements consumes no simulation ticks. `N.wait` is what lets the graph move between edits.

For example, creating one hundred nodes in a single batch differs from creating one node, waiting, then creating the next. Merely splitting a batch into successive events with no intervening waits does not give the solver time to react between them.

A recording's frame rate and ticks per frame determine how these ticks are sampled into video. A tick is not one second and is not automatically one video frame. The solver's numerical step size is a mobility factor per tick, separate from playback timing. Any app-configured final relaxation interval runs after the generator has finished.

To test different orders fairly, keep the seed, birth-position policy, waits, and solver settings fixed. Default birth positions are deterministic from the seed and node ID in a 24 × 24 world-unit square centred on the origin. They do not depend on a neighbour's evolving position. Supply `position` if the experiment requires a different placement rule.

## Parameters and colours

`params` is the JSON value entered in the parameter editor. The ABC starter also exposes simple controls for commonly changed properties; these controls edit the same JSON. Read its properties directly, validate assumptions, and use ordinary JavaScript loops and functions to build a graph. A colour rule is just a function returning a colour string. Dynamic strength means yielding a strength update at a chosen point in the event sequence.

Use `N.random()` instead of `Math.random()` when an experiment needs randomness. The runner does not provide filesystem access, network access, or a general module loader. Do not rely on wall-clock time to schedule the simulation.

The worker has memory and execution-time bounds. A rule that produces too much output or does not finish will be rejected. The full finite plan is generated before playback; an unbounded generator is not a way to run an experiment indefinitely.

## The ABC experiment

The starter example builds strings over an alphabet. Without repetition, `A`, `B`, and `C` produce six ordered two-letter strings and six ordered three-letter strings. The three-letter string `ABC` connects to `AB` and `BC`, its contiguous two-letter substrings, not to `AC`.

Its parameters are `alphabet`, `maxLength`, `repetitions`, `order`, `ticksPerNode`, and `finalTicks`. For example:

```json
{
  "alphabet": "ABC",
  "maxLength": 3,
  "repetitions": false,
  "order": "lexicographic",
  "ticksPerNode": 24,
  "finalTicks": 0
}
```

`order` accepts `"lexicographic"`, `"reverse"`, or `"shuffle"`. Each changes insertion order within a length layer, keeping shorter parent nodes available before longer children. Shuffle uses the seeded `N.random()` sequence. Legacy `repeatLetters` and `reverse` parameters are still accepted when their canonical counterparts are absent. `finalTicks` is an explicit wait in this example's rules and is additional to any final relaxation interval configured in the app.

Allowing repetition changes the problem: a three-symbol alphabet then has nine two-letter strings and twenty-seven three-letter strings. Increasing alphabet size or maximum length can grow the graph very quickly. Validate before recording.

## Growing ring

The Growing ring starter accepts `count` (default 80), `ticksPerNode` (default 12), and `finalTicks` (default 0). It inserts a chain one node at a time and joins the last node to the first when there are at least three nodes. Birth positions use the usual seed-and-ID placement, so a circle is not supplied as the initial layout. Its node colours also use seeded randomness.

Node deletion, edge deletion, callbacks that inspect live positions, and an interactive JavaScript debugger are not part of this first rule API.
