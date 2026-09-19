/* @controls {
  "count": {"type":"integer","label":"Generation number","description":"Create nodes 1 through this number.","default":20,"min":1},
  "colorBy": {"type":"select","label":"Colour by","default":"birth-order","options":["birth-order","parity","number-type"]},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":24,"min":0},
  "strength": {"type":"number","label":"Connection strength","default":4,"min":0},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":240,"min":0}
} */

// Each number connects to every smaller positive integer that divides it exactly.
// 8 connects to 4, 2, 1. 12 connects to 6, 4, 3, 2, 1.
// Node 1 has no connections at birth. Each divisor gets exactly one edge.
// Colour by birth order, odd/even membership, or type (1 / prime / composite).
// Newly encountered categories follow red, yellow, green, cyan, blue, magenta.

function* generate(N, params) {
    const count = params.count ?? 20;
    const ticks = params.ticksPerNode ?? 24;
    const strength = params.strength ?? 4;
    const finalTicks = params.finalTicks ?? 240;
    const colorBy = params.colorBy ?? "birth-order";

    if (!Number.isSafeInteger(count) || count < 1) {
        throw new Error("Generation number must be a positive safe integer.");
    }

    if (!["birth-order", "parity", "number-type"].includes(colorBy)) {
        throw new Error('Colour by must be "birth-order", "parity", or "number-type".');
    }
    const categoryCount = colorBy === "birth-order" ? count : colorBy === "parity" ?
        Math.min(2, count) : count >= 4 ? 3 : count >= 2 ? 2 : 1;
    const colors = N.palette(categoryCount);
    const categories = new Map();

    function properDivisors(number) {
        const divisors = [];

        // Divisors occur in pairs, so only search up to the square root.
        for (let divisor = 1; divisor <= number / divisor; divisor++) {
            if (number % divisor !== 0) continue;

            const partner = number / divisor;

            if (divisor < number) divisors.push(divisor);
            if (partner < number && partner !== divisor) {
                divisors.push(partner);
            }
        }

        return divisors.sort((a, b) => b - a);
    }

    for (let number = 1; number <= count; number++) {
        const divisors = properDivisors(number);
        const id = String(number);
        const category = colorBy === "birth-order" ? number : colorBy === "parity" ?
            number % 2 : number === 1 ? "one" : divisors.length === 1 ? "prime" : "composite";
        if (colorBy !== "birth-order" && !categories.has(category)) categories.set(category, categories.size);
        const color = colors[colorBy === "birth-order" ? number - 1 : categories.get(category)];

        const edges = divisors.map(divisor => N.edge(id, String(divisor), {
            id: `${number}:${divisor}`,
            strength,
            gradient: true,
            color: "#FFFFFFCC"
        }));

        yield N.batch(
            [N.node(id, { label: id, color, radius: 1.6 })],
            edges
        );

        yield N.wait(ticks);
    }

    yield N.wait(finalTicks);
}
