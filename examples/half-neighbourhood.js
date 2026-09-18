/* @controls {
  "count": {"type":"integer","label":"Nodes","default":500,"min":1},
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
    const colors = graph.palette(8);
    for (let n = 1; n <= p.count; n++) {
        graph.add(n, { color: colors[(n - 1) % colors.length] });
        const neighbours = Math.floor(n / 2);
        for (let offset = 1; offset <= neighbours; offset++) {
            graph.connect(n, n - offset, {
                strength: p.strength, gradient: true, color: "#ffffff80"
            });
        }
        graph.wait(p.interval);
    }
}
