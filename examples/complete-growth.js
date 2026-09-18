/* @controls {
  "count": {"type":"integer","label":"Nodes","default":500,"min":1,"max":500},
  "interval": {"type":"integer","label":"Ticks between births","default":6,"min":0,"max":600},
  "strength": {"type":"number","label":"Connection strength","default":1,"min":0,"max":100,"step":0.05},
  "nodeColor": {"type":"color","label":"Node colour","default":"#80c7ff"},
  "edgeColor": {"type":"color","label":"Edge colour","default":"#88aaff28"}
} */
// Each new node connects to every node that came before it.
// 500 nodes form 124,750 connections, with no duplicates or self-connections.
function build(graph, p) {
    for (let n = 1; n <= p.count; n++) {
        graph.add(n, { color: p.nodeColor });
        graph.connect(n, graph.others(n), {
            color: p.edgeColor, strength: p.strength
        });
        graph.wait(p.interval);
    }
}
