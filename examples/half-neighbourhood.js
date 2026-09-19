/* @controls {
  "count": {"type":"integer","label":"Nodes","default":500,"min":1},
  "colorBy": {"type":"select","label":"Colour by","description":"Neighbour count groups nodes by how many connections they have at birth.","default":"birth-order","options":["birth-order","neighbour-count","parity","single"]},
  "interval": {"type":"integer","label":"Ticks between births","default":6,"min":0,"max":600},
  "strength": {"type":"number","label":"Connection strength","default":4,"min":0,"max":100,"step":0.05}
} */
// Each new node n connects to the most recent floor(n / 2) predecessors.
// 1: none; 2: 1; 3: 2; 4: 3, 2; 5: 4, 3; 6: 5, 4, 3.
// The default 500 nodes form 62,500 connections.
function build(graph, p) {
    if (!Number.isSafeInteger(p.count) || p.count < 1) {
        throw new Error("count must be a positive safe integer");
    }
    const mode = p.colorBy ?? "birth-order";
    const colorCount = mode === "birth-order" ? p.count
        : mode === "neighbour-count" ? Math.floor(p.count / 2) + 1
        : mode === "parity" ? Math.min(p.count, 2) : 1;
    const colors = graph.palette(colorCount);
    for (let n = 1; n <= p.count; n++) {
        const neighbours = Math.floor(n / 2);
        const index = mode === "birth-order" ? n - 1
            : mode === "neighbour-count" ? neighbours
            : mode === "parity" ? (n - 1) % 2 : 0;
        graph.add(n, { color: colors[index] });
        for (let offset = 1; offset <= neighbours; offset++) {
            graph.connect(n, n - offset, {
                strength: p.strength, gradient: true, color: "#ffffff80"
            });
        }
        graph.wait(p.interval);
    }
}
