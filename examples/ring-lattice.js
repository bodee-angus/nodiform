/* @controls {
  "count": {"type":"integer","label":"Node Count","description":"Generate nodes 1 through this number, in order.","default":10,"min":1},
  "previousCount": {"type":"integer","label":"Previous Node Connect Count","description":"Connect each node to this many predecessors, wrapping around the sequence. Connections are undirected.","default":2,"min":0},
  "colorBy": {"type":"select","label":"Colour by","default":"birth-order","options":["birth-order","ring-distance"]},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":24,"min":0,"max":4294967295},
  "strength": {"type":"number","label":"Connection strength","default":4,"min":0,"max":1000000},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":240,"min":0,"max":4294967295}
} */

// A ring lattice: every node connects to K predecessors in the cyclic sequence.
// Nodes are born as 1, 2, 3, ...; wraparound edges appear when both endpoints
// exist. With 10 nodes and K = 2, node 9 adds 9–1; node 10 adds 10–1 and 10–2.
// Connections are undirected, so each node finally has min(2K, Node Count - 1)
// neighbours. Overlapping connections are added once, without self-loops.
// K = 0 creates isolated nodes; K >= Node Count - 1 creates a complete graph.
// Colours follow birth order, or shortest distance around the ring from node 1.
// Both colour methods introduce new hues in rainbow order.

function* generate(N, params) {
    const count = params.count ?? 10;
    const previousCount = params.previousCount ?? 2;
    const colorBy = params.colorBy ?? "birth-order";
    const ticks = params.ticksPerNode ?? 24;
    const strength = params.strength ?? 4;
    const finalTicks = params.finalTicks ?? 240;

    if (!Number.isSafeInteger(count) || count < 1) {
        throw new Error("Node Count must be a positive safe integer.");
    }
    if (!Number.isSafeInteger(previousCount) || previousCount < 0) {
        throw new Error("Previous Node Connect Count must be a nonnegative safe integer.");
    }
    if (!["birth-order", "ring-distance"].includes(colorBy)) {
        throw new Error('Colour by must be "birth-order" or "ring-distance".');
    }

    const effectiveK = Math.min(previousCount, count - 1);
    const colors = N.palette(colorBy === "birth-order" ? count : Math.floor(count / 2) + 1);

    for (let index = 0; index < count; index++) {
        const number = index + 1;
        const id = String(number);
        const edges = [];
        const addEdge = previous => edges.push(N.edge(id, String(previous), {
            id: `ring:${number}:${previous}`,
            strength, gradient: true, color: "#ffffffcc"
        }));

        // Existing neighbours occupy at most two intervals: the preceding K
        // numbers and a prefix whose cyclic predecessors include this new node.
        // Exclude their overlap so no edge needs a duplicate-checking set.
        // Subtraction keeps the calculation safe even for very large counts.
        const firstPrevious = Math.max(1, number - effectiveK);
        const lastWrapped = Math.min(firstPrevious - 1, number - (count - effectiveK));
        for (let previous = 1; previous <= lastWrapped; previous++) addEdge(previous);
        for (let previous = firstPrevious; previous < number; previous++) addEdge(previous);

        const category = colorBy === "birth-order" ? index : Math.min(index, count - index);
        yield N.batch([N.node(id, {
            label: id, color: colors[category], radius: 1.6
        })], edges);
        yield N.wait(ticks);
    }

    yield N.wait(finalTicks);
}
