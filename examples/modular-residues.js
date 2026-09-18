/* @controls {
  "count": {"type":"integer","label":"Nodes","default":100,"min":1,"max":8192},
  "moduli": {"type":"json","label":"Moduli","default":[3,5,7]},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":18,"min":0,"max":1000000},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":600,"min":0,"max":1000000},
  "scaffoldStrength": {"type":"number","label":"Scaffold strength","default":0.15,"min":0,"max":100,"step":0.05}
} */
// Connect integers that share a residue, then add chain edges as an explicit scaffold.
// Parameters: count, moduli, ticksPerNode, finalTicks, scaffoldStrength.
// Scaffold edges are real, visible forces, never hidden gravity.
function* generate(N, params) {
    const count = params.count ?? 100;
    const moduli = params.moduli ?? [3, 5, 7];
    const ticks = params.ticksPerNode ?? 18;
    const scaffold = params.scaffoldStrength ?? 0.15;
    const colors = ["#89b4fa", "#cba6f7", "#a6e3a1", "#fab387", "#f38ba8"];
    if (!Number.isInteger(count) || count < 1 || count > 8192) throw new Error("count must be 1–8192");
    if (!Array.isArray(moduli) || moduli.some(m => !Number.isInteger(m) || m < 2)) {
        throw new Error("moduli must be an array of integers greater than one");
    }
    for (let n = 0; n < count; n++) {
        const edges = [];
        for (let i = 0; i < moduli.length; i++) {
            const previous = n - moduli[i];
            if (previous >= 0) {
                edges.push(N.edge(String(n), String(previous), {
                    id: `mod:${i}:${n}`, color: colors[i % colors.length] + "a0",
                    strength: 1 / Math.sqrt(moduli[i])
                }));
            }
        }
        if (n > 0 && scaffold > 0) {
            edges.push(N.edge(String(n), String(n - 1), {
                id: `scaffold:${n}`, color: "#65718a60", strength: scaffold
            }));
        }
        yield N.batch([N.node(String(n), {
            color: colors[(n % (moduli[0] ?? 3)) % colors.length], radius: 1.4
        })], edges);
        yield N.wait(ticks);
    }
    yield N.wait(params.finalTicks ?? 600);
}
