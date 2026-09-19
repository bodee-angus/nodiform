/* @controls {
  "count": {"type":"integer","label":"Nodes","default":500,"min":1},
  "interval": {"type":"integer","label":"Ticks between births","default":6,"min":0,"max":600},
  "strength": {"type":"number","label":"Connection strength","default":4,"min":0,"max":100,"step":0.05},
  "colorBy": {"type":"select","label":"Colour by","default":"birth-order","options":["birth-order","parity","single"]},
  "edgeColor": {"type":"color","label":"Edge colour","default":"#508dff28"}
} */
// Each new node connects to every node that came before it.
// 500 nodes form 124,750 connections, with no duplicates or self-connections.
function build(graph, p) {
    if (!Number.isSafeInteger(p.count) || p.count < 1) {
        throw new Error("count must be a positive safe integer");
    }
    const mode = p.colorBy ?? "birth-order";
    const colors = graph.palette(mode === "birth-order" ? p.count : mode === "parity" ? Math.min(p.count, 2) : 1);
    for (let n = 1; n <= p.count; n++) {
        const index = mode === "birth-order" ? n - 1 : mode === "parity" ? (n - 1) % 2 : 0;
        graph.add(n, { color: colors[index] });
        graph.connect(n, graph.others(n), {
            color: p.edgeColor, strength: p.strength
        });
        graph.wait(p.interval);
    }
}
