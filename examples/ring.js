/* @controls {
  "count": {"type":"integer","label":"Nodes","default":80,"min":1},
  "colorBy": {"type":"select","label":"Colour by","default":"birth-order","options":["birth-order","alternating","single"]},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":12,"min":0,"max":1000000},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":0,"min":0,"max":1000000}
} */
// A growing chain closes into a ring. Observe how local insertions reshape the whole graph.
// Parameters: count (default 80), ticksPerNode (12), finalTicks (0).
// Birth positions use the run's seeded ID jitter, not a prearranged circle.
function* generate(N, params) {
    const count = params.count ?? 80;
    const ticks = params.ticksPerNode ?? 12;
    if (!Number.isSafeInteger(count) || count < 1) {
        throw new Error("count must be a positive safe integer");
    }
    const mode = params.colorBy ?? "birth-order";
    const colors = N.palette(mode === "birth-order" ? count : mode === "alternating" ? Math.min(count, 2) : 1);
    for (let n = 0; n < count; n++) {
        const edges = [];
        if (n > 0) {
            edges.push(N.edge(String(n - 1), String(n), {
                id: `link:${n}`, gradient: true, color: "#ffffffcc", strength: 4
            }));
        }
        if (n === count - 1 && count > 2) {
            edges.push(N.edge(String(n), "0", {
                id: "closure", gradient: true, color: "#ffffffcc", strength: 4
            }));
        }
        yield N.batch([N.node(String(n), {
            label: String(n + 1), radius: 1.5,
            color: colors[mode === "birth-order" ? n : mode === "alternating" ? n % 2 : 0]
        })], edges);
        yield N.wait(ticks);
    }
    yield N.wait(params.finalTicks ?? 0);
}
