/* @controls {
  "count": {"type":"integer","label":"Generation number","description":"Create nodes 1 through this number.","default":20,"min":1},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":24,"min":0},
  "strength": {"type":"number","label":"Connection strength","default":4,"min":0},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":240,"min":0}
} */

// Each number connects to every smaller positive integer that divides it exactly.
// 8 connects to 4, 2, 1. 12 connects to 6, 4, 3, 2, 1.
// Node 1 has no connections at birth. Each divisor gets exactly one edge.
// Generate one palette colour per node, then sort by hue.
// Increasing node numbers follow red, yellow, green, cyan, blue, magenta.

function* generate(N, params) {
    const count = params.count ?? 20;
    const ticks = params.ticksPerNode ?? 24;
    const strength = params.strength ?? 4;
    const finalTicks = params.finalTicks ?? 240;

    if (!Number.isSafeInteger(count) || count < 1) {
        throw new Error("Generation number must be a positive safe integer.");
    }

    function hueOf(hex) {
        const r = parseInt(hex.slice(1, 3), 16);
        const g = parseInt(hex.slice(3, 5), 16);
        const b = parseInt(hex.slice(5, 7), 16);
        const maximum = Math.max(r, g, b);
        const delta = maximum - Math.min(r, g, b);

        if (delta === 0) return 360;

        let hue;
        if (maximum === r) hue = (g - b) / delta;
        else if (maximum === g) hue = (b - r) / delta + 2;
        else hue = (r - g) / delta + 4;

        return (hue * 60 + 360) % 360;
    }

    const colors = N.palette(count)
        .map((color, index) => ({ color, index, hue: hueOf(color) }))
        .sort((a, b) => a.hue - b.hue || a.index - b.index)
        .map(entry => entry.color);

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
        const color = colors[number - 1];

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
