/* @controls {
  "count": {"type":"integer","label":"Nodes","default":80,"min":1,"max":8192},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":12,"min":0,"max":1000000},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":0,"min":0,"max":1000000}
} */
// A growing chain closes into a ring. Observe how local insertions reshape the whole graph.
// Parameters: count (default 80), ticksPerNode (12), finalTicks (0).
// Birth positions use the run's seeded ID jitter, not a prearranged circle.
function* generate(N, params) {
    const count = params.count ?? 80;
    const ticks = params.ticksPerNode ?? 12;
    const colors = ["#89b4fa", "#cba6f7", "#a6e3a1", "#fab387", "#f5c2e7"];
    if (!Number.isInteger(count) || count < 1 || count > 8192) {
        throw new Error("count must be an integer from 1 to 8192");
    }
    for (let n = 0; n < count; n++) {
        const edges = [];
        if (n > 0) {
            edges.push(N.edge(String(n - 1), String(n), {
                id: `link:${n}`, color: "#7f8fa6b0", strength: 1
            }));
        }
        if (n === count - 1 && count > 2) {
            edges.push(N.edge(String(n), "0", {
                id: "closure", color: "#89dcebd0", strength: 1
            }));
        }
        yield N.batch([N.node(String(n), {
            label: String(n + 1), radius: 1.5,
            color: colors[Math.floor(N.random() * colors.length)]
        })], edges);
        yield N.wait(ticks);
    }
    yield N.wait(params.finalTicks ?? 0);
}
