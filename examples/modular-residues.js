/* @controls {
  "count": {"type":"integer","label":"Nodes","default":100,"min":1},
  "moduli": {"type":"json","label":"Moduli","default":[3,5,7]},
  "colorBy": {"type":"select","label":"Colour by","default":"residue","options":["residue","birth-order","parity"]},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":18,"min":0,"max":1000000},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":600,"min":0,"max":1000000},
  "scaffoldStrength": {"type":"number","label":"Scaffold strength","default":0.6,"min":0,"max":100,"step":0.05}
} */
// Connect integers that share a residue, then add chain edges as an explicit scaffold.
// Colour by the first modulus, birth order, or odd/even membership.
// New colour categories receive successive hues in order of first appearance.
// Scaffold edges are real, visible forces, never hidden gravity.
function* generate(N, params) {
    const count = params.count ?? 100;
    const moduli = params.moduli ?? [3, 5, 7];
    const ticks = params.ticksPerNode ?? 18;
    const scaffold = params.scaffoldStrength ?? 0.6;
    const colorBy = params.colorBy ?? "residue";
    if (!Number.isSafeInteger(count) || count < 1) throw new Error("count must be a positive safe integer");
    if (!Array.isArray(moduli) ||
        moduli.some(m => !Number.isSafeInteger(m) || m < 2)) {
        throw new Error("moduli must contain safe integers greater than one");
    }
    if (!["residue", "birth-order", "parity"].includes(colorBy)) {
        throw new Error('Colour by must be "residue", "birth-order", or "parity".');
    }
    const modulus = moduli[0] ?? 3;
    const nodeColors = N.palette(colorBy === "birth-order" ? count : Math.min(count, colorBy === "parity" ? 2 : modulus));
    const categories = new Map();
    const edgeColors = N.palette(moduli.length);
    const edgeCategories = new Map();
    for (let n = 0; n < count; n++) {
        const edges = [];
        for (let i = 0; i < moduli.length; i++) {
            const previous = n - moduli[i];
            if (previous >= 0) {
                if (!edgeCategories.has(i)) edgeCategories.set(i, edgeCategories.size);
                edges.push(N.edge(String(n), String(previous), {
                    id: `mod:${i}:${n}`, color: edgeColors[edgeCategories.get(i)] + "a0",
                    strength: 4 / Math.sqrt(moduli[i])
                }));
            }
        }
        if (n > 0 && scaffold > 0) {
            edges.push(N.edge(String(n), String(n - 1), {
                id: `scaffold:${n}`, color: "#ffffff60", gradient: true, strength: scaffold
            }));
        }
        const category = colorBy === "birth-order" ? n : n % (colorBy === "parity" ? 2 : modulus);
        if (colorBy !== "birth-order" && !categories.has(category)) categories.set(category, categories.size);
        yield N.batch([N.node(String(n), {
            color: nodeColors[colorBy === "birth-order" ? n : categories.get(category)], radius: 1.4
        })], edges);
        yield N.wait(ticks);
    }
    yield N.wait(params.finalTicks ?? 600);
}
