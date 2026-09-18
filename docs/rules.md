# Writing a graph experiment

Rules are JavaScript, evaluated by QuickJS in a separate worker process. Define an ordinary synchronous `function build(graph, p)`. The application validates its complete finite event plan before playback. There is no TypeScript compiler or language server in this alpha; **Validate** checks execution and graph consistency.

The **Rules** tab holds the program. **Inputs** shows optional controls declared by that program. An alphabet, a node count, or a choice of graph family has no special meaning to the application.

## Start with a small chain

The default experiment has no inputs. Its values live directly in the code:

```js
function build(graph) {
  const colors = graph.palette(4);
  for (let n = 1; n <= 8; n++) {
    graph.add(n, { color: colors[(n - 1) % colors.length] });
    if (n > 1) graph.connect(n, n - 1, { gradient: true });
    graph.wait(24);
  }
}
```

`graph.add` creates a node specification. `graph.connect` adds connections. `graph.wait` commits pending additions as a batch and schedules solver ticks before the next changes. Pending additions are also committed at the end of `build`, or before a node/edge update. A batch is transactional: if any item is invalid, none of that batch is applied.

## Connect every new node to every earlier node

```js
function build(graph) {
  for (let n = 1; n <= 500; n++) {
    graph.add(n);
    graph.connect(n, graph.others(n), { strength: 4 });
    graph.wait(6);
  }
}
```

This creates 500 nodes and `500 × 499 / 2 = 124,750` edges. Each pair is connected once, with no self-connections. **Experiment → Examples → Connect to every earlier node** includes this growth rule with optional inputs for count, interval, strength, and colours. Its default of 500 is an example value, not an application-wide limit or a performance promise.

## Builder API

| Call | Meaning |
| --- | --- |
| `graph.add(id, options)` | Add a node; return its normalised string ID. |
| `graph.ids()` | Return all node IDs added so far, in insertion order. |
| `graph.others(id)` | Return those IDs except the supplied ID. |
| `graph.connect(source, targetOrTargets, options)` | Add edges to one ID or an array of IDs; return an array of edge IDs. |
| `graph.wait(ticks)` | Commit pending additions, then advance the force simulation by this many ticks. |
| `graph.setNode(id, options)` | Commit pending additions, then change a node's colour or radius. |
| `graph.setEdge(id, options)` | Commit pending additions, then change an edge's colour, strength or gradient mode. |
| `graph.random()` | Draw from the run's seeded pseudorandom sequence. |
| `graph.palette(count)` | Return an array of vivid, perceptually spaced `#rrggbb` colours. |

Node options are `label`, `color`, `radius`, and an optional initial `position: [x, y]`. Edge options are `id`, `color`, `strength`, and `gradient`. Edge strength defaults to **4**, and gradient mode defaults to **false**. A custom edge ID can be supplied only when connecting to one target. For example:

```js
function build(graph) {
  graph.add("A", { color: "#80c7ff", radius: 2 });
  graph.add("B", { color: "#cda7ff", radius: 2 });
  const [edge] = graph.connect("A", "B", { strength: 0.5 });
  graph.wait(120);
  graph.setEdge(edge, { strength: 2, color: "#ffffffaa" });
  graph.wait(120);
}
```

Builder IDs may be strings or finite numbers. Numeric IDs are converted to strings, so `1` and `"1"` refer to the same ID. Node IDs must be unique among nodes, and edge IDs unique among edges. An edge can refer to nodes already committed or added in the same pending batch.

Colours accept `#RRGGBB` or `#RRGGBBAA`. Radius must be greater than zero and at most 1,000,000 world units. Strength must be between zero and 1,000,000; zero disables an edge's attraction without deleting it. Each birth coordinate must have an absolute value of at most 1,000,000. All numeric values must be finite. Typical experiments should use much smaller values.

Edges attract both endpoints. `source` and `target` identify the connection; they do not make its force one-way. Multiple differently named edges between two nodes add their attractions together. Connection-based visual sizing deduplicates these differently, as described in the [architecture guide](architecture.md#connection-based-node-sizing).

## Generate colours and blend edges

Ask for the number of categories you need. The helper returns ordinary hex strings that work everywhere a node or edge colour is accepted:

```js
function build(graph) {
  const [first, second] = graph.palette(2);
  graph.add("A", { color: first });
  graph.add("B", { color: second });
  const [edge] = graph.connect("A", "B", {
    gradient: true, color: "#ffffffcc"
  });
  graph.wait(240);
  graph.setNode("B", { color: "#ff29bb" });
  graph.wait(240); // The gradient follows B's new colour.
  graph.setEdge(edge, { gradient: false, color: "#ffffff" });
}
```

`graph.palette(count)` accepts a nonnegative safe integer; zero returns an empty array. The former 8,192-colour ceiling is removed, although the requested array must still fit memory and JavaScript’s array representation. It uses Oklab/OKLCH with sRGB gamut handling to select vibrant colours. The same request returns the same hex codes, and increasing the count preserves the earlier colours. Returned arrays are independent copies. It does not consume `graph.random()` or require a network connection. **Insert → Colour palette** inserts the call in either editor API.

Small palettes maximise separation among a fixed set of vivid candidates. Beyond 128 colours the helper uses deterministic sampling, with a bounded number of attempts per colour. The colour prefix supported by earlier versions is preserved. Larger palettes can repeat hex values once candidate retries are exhausted; finite sRGB colour space cannot provide arbitrarily many unique colours. Increasingly large palettes also become difficult to distinguish. This is not a colour-vision-deficiency guarantee.

When `gradient: true`, the edge blends from its source node's current colour to its target's current colour in linear-light RGB. The `color` option's **alpha** controls edge opacity, multiplied by endpoint alpha; its RGB is ignored. `#ffffffcc` means 80% edge opacity, and `#ffffff` means fully opaque. Omitting `color` keeps the default edge opacity of 60%. Node colour updates affect gradients automatically in both preview and video. Set `gradient: false` to return to an ordinary solid edge.

## Optional script-defined inputs

Put an `@controls` block at the very beginning of the script when particular values should be adjustable in **Inputs**. The comment contains JSON, so use double quotes and no trailing commas:

```js
/* @controls {
  "count": {
    "type": "integer", "label": "Nodes",
    "default": 500, "min": 1
  },
  "interval": {
    "type": "integer", "label": "Ticks between births",
    "default": 6, "min": 0, "max": 600
  }
} */
function build(graph, p) {
  for (let n = 1; n <= p.count; n++) {
    graph.add(n);
    graph.connect(n, graph.others(n));
    graph.wait(p.interval);
  }
}
```

Each property name becomes a key on `p`. The required fields are `type`, `label`, and `default`; `description` is optional. Available types are:

| Type | Value and optional fields |
| --- | --- |
| `integer` | Whole number; optional `min`, `max`, and `step`. |
| `number` | Finite number; optional `min`, `max`, and `step`. |
| `boolean` | `true` or `false`. |
| `text` | String. |
| `color` | Hex colour string. |
| `select` | String chosen from the required `options` array of strings. |
| `json` | Any JSON value, including arrays and objects. |

Only the leading comment declares controls; whitespace before it is allowed. A script may expose up to 64 controls, with at most 64 KiB of metadata. Invalid metadata or input values produce an error rather than being silently replaced.

Defaults fill missing input keys when compiling. Existing values take precedence, and undeclared keys remain available to the script. Merely opening **Inputs** does not replace stored values with defaults. **Advanced · Input JSON** edits the same input object, including properties without a control. A script without `@controls` can still read values from that object.

A count control can omit `max`, as above. A `max` declared by your script remains a constraint on that input; removing the application’s graph caps does not override it.

The seed, playback timing, recording settings, and appearance options belong in the application's **Settings** window. Experiment-specific inputs belong to the script.

## Order and time are separate

Creation and updates consume no simulation ticks. `graph.wait` lets the graph move between edits. Creating a hundred nodes and then waiting therefore differs from waiting after each birth. Multiple operations without intervening waits give the solver no time to react between them.

A recording's frame rate and ticks per frame determine how ticks become video samples. Preview targets their product in solver ticks per second, carrying fractional ticks between display refreshes; the defaults give 240 ticks per second. Preview has bounded catch-up after a stall, while recording preserves every fixed timeline sample even when it takes longer than real time. A tick is neither one second nor necessarily one video frame. The solver's force multiplier is separate from playback timing. The configured final settling interval runs after the plan has finished.

### Where new nodes appear

Without a `position` option, a new node starts near the average **current** position of the connected nodes created before it. A deterministic seed-and-ID offset in `[-1, 1)` world units per axis avoids placing every birth at exactly the same coordinate. If it has no connected predecessors, it starts near the world origin `[0, 0]` with that offset. The average counts each neighbour once regardless of duplicate edges, edge direction or strength; zero-strength edges still identify neighbours.

Operations without an intervening positive wait are synchronised together. A birth can use connections present at that synchronisation, including connections to nodes created earlier at the same simulated time. It cannot use a node created later in the sequence as an anchor. Connections added after a positive wait only change the forces and drawing; they do not reposition nodes that already exist.

Use `graph.add("A", { position: [20, 10] })` for an exact initial position, or the equivalent `N.node` option. Explicit positions override automatic placement and remain subject to normal force simulation afterward. The generator cannot read live positions; automatic neighbour placement is resolved by the GPU during playback.

For fair order comparisons, keep the seed, birth policy, waits and solver version fixed. The seed fixes the offset, but the complete initial position also depends on the birth order and the evolving neighbours. Changing the order can therefore change both placement and subsequent motion.

Use `graph.random()` instead of `Math.random()`. Filesystem access, network access, a general module loader, wall-clock timers, and unseeded randomness are not exposed.

## Existing generators still work

Saved projects using `function* generate(N, params)` remain supported. Prefer a single entry function. If both names exist, `generate` takes precedence so older scripts can retain a helper named `build`. The legacy generator explicitly yields events:

```js
function* generate(N, params) {
  yield N.batch([N.node("A"), N.node("B")], [
    N.edge("A", "B", { id: "AB", strength: 1 })
  ]);
  yield N.wait(120);
  yield N.setEdge("AB", { strength: 2 });
  yield N.wait(120);
}
```

`N.node` and `N.edge` construct specifications for `N.batch`; they are not events to yield individually. `N.wait`, `N.setNode`, and `N.setEdge` produce events. `N.random` uses the same seeded sequence. `N.palette(count)` provides the same palette helper, and `N.edge` / `N.setEdge` accept `gradient`. Leading `@controls` metadata also works with this API.

## Included experiments and limits

**Connect to the previous half** creates numbered nodes in order. Node `n` connects to the most recent `floor(n / 2)` predecessors: `1` has no edges at birth, `2` connects to `1`, `3` to `2`, `4` to `3` and `2`, `5` to `4` and `3`, and `6` to `5`, `4` and `3`. The default 500 nodes create 62,500 edges. Choose it under **Experiment → Examples**, then adjust its script-defined inputs. It has no fixed node-count ceiling; available resources and the solver's cost still apply. Its source is [half-neighbourhood.js](../examples/half-neighbourhood.js).

**Letter permutations** builds strings over an alphabet. With `ABC` and no repetition it creates six ordered two-letter strings and six ordered three-letter strings. `ABC` connects to the contiguous substrings `AB` and `BC`, not `AC`. Alphabet, length, repetition, and order are declared by that example's source. **Growing ring** builds a chain and closes its endpoints without prearranging a circle. **Modular residues** connects numbers by residue rules and can add explicit scaffold edges.

The permutation example uses `N.palette(alphabet.length)` and assigns each node the colour of its **first letter**. `A`, `AB` and `ACB` therefore share a colour. Edges blend their endpoint colours. Alphabet characters are Unicode code points, not full grapheme clusters.

Choose **Inputs → Birth order → connected-branch-walk** for:

```text
A, AB, ABC, AC, ACB,
B, BA, BAC, BC, BCA,
C, CA, CAB, CB, CBA
```

This keeps starting-letter groups in alphabet order and alternates child traversal directions, but always creates a prefix before its descendants. Each longer word connects to its existing prefix in the same batch as its birth. Later single-letter roots attach through two-letter words created in earlier groups. With a maximum length of at least two, every node after the first therefore joins the existing connected graph immediately. Repeated letters and Unicode alphabets use the same rule. A maximum length of one produces letters without edges, so those nodes remain separate by definition.

The original **branch-walk** remains available for:

```text
A, AB, ABC, ACB, AC,
B, BA, BAC, BCA, BC,
C, CA, CAB, CBA, CB
```

This also alternates forward and backward child subtrees, but reversing a subtree puts its prefix last, which deliberately places `ACB` before `AC` and can create temporary disconnected islands. Both branch walks use an explicit stack and work with other lengths and repeated letters. An edge waits until both of its nodes exist, then appears with the later node. Changing birth order preserves the final graph, colours and edge strengths while changing its evolution; connected mode adds no scaffold edges. The layer-based `lexicographic`, `reverse` and seeded `shuffle` options remain available.

There is no application-selected node or edge count cap. The graph must fit the GPU’s actual buffer and dispatch limits, shader index ranges, available GPU memory, and host memory. Exact all-pairs repulsion remains O(N²), so larger graphs can be much slower even when they fit.

The updated examples remove their former node and edge count guards. Older saved or custom scripts are not rewritten: an `8192` check, a count-control `max`, or another restriction in their source still applies. Load an updated example or edit those rules explicitly.

Rule generation still creates and validates a complete finite plan before playback. Events are emitted directly to the native worker, avoiding a second full JSON plan inside JavaScript. Heap and plan budgets derive from available host memory, including the remaining memory allowance in a cgroup. There is no fixed event-count or generated-JSON ceiling. Source, input, stack and numeric representation limits remain.

Long finite generation can continue while it makes progress and fits memory; use **Stop** to cancel it. The JavaScript watchdog stops code that goes five seconds without rule API progress. A separate worker watchdog stops a worker that produces no pipe activity for 60 seconds. Neither is a total duration allowance for every valid experiment.

Rules cannot inspect live GPU positions or react to solver state. Deletion, resumable checkpoints, async functions, and an interactive debugger are not part of this API. The worker isolates failures but is not a security sandbox for untrusted programs.
