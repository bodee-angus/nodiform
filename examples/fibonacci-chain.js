/* @controls {
  "maxNumber": {"type":"integer","label":"Maximum number","description":"Create every integer from 0 through this number, inclusive. 20 creates 21 nodes.","default":20,"min":0},
  "colorBy": {"type":"select","label":"Colour by","default":"birth-order","options":["birth-order","fibonacci-membership","parity"]},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":24,"min":0},
  "ticksPerEdge": {"type":"integer","label":"Ticks between Fibonacci links","default":24,"min":0},
  "strength": {"type":"number","label":"Connection strength","default":4,"min":0},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":240,"min":0}
} */

// First create the chain 0—1—2—3—...—Maximum number.
// Then connect consecutive Fibonacci values: 3—5, 5—8, 8—13, 13—21, ...
// Earlier Fibonacci pairs already belong to the chain; repeated 1 adds no loop.
// Ratios of consecutive positive Fibonacci values approach the golden ratio.
// Colour by birth order, Fibonacci membership, or odd/even membership.
// New categories receive successive rainbow hues in order of first appearance.
function* generate(N, params) {
    const maximum = params.maxNumber ?? 20;
    const colorBy = params.colorBy ?? "birth-order";
    const ticks = params.ticksPerNode ?? 24;
    const edgeTicks = params.ticksPerEdge ?? 24;
    const strength = params.strength ?? 4;
    const finalTicks = params.finalTicks ?? 240;

    if (!Number.isSafeInteger(maximum) || maximum < 0 || !Number.isSafeInteger(maximum + 1)) {
        throw new Error("Maximum number must be a nonnegative safe integer whose node count (Maximum number + 1) is also safe.");
    }
    if (!["birth-order", "fibonacci-membership", "parity"].includes(colorBy)) {
        throw new Error('Colour by must be "birth-order", "fibonacci-membership", or "parity".');
    }

    const fibonacci = [0];
    let previous = 0;
    let current = 1;
    while (current <= maximum) {
        if (current !== fibonacci[fibonacci.length - 1]) fibonacci.push(current);
        const next = previous + current;
        if (!Number.isSafeInteger(next)) break;
        previous = current;
        current = next;
    }
    const fibonacciNumbers = new Set(fibonacci);
    const count = maximum + 1;
    const categoryCount = colorBy === "birth-order" ? count : colorBy === "parity" ?
        Math.min(count, 2) : maximum >= 4 ? 2 : 1;
    const colors = N.palette(categoryCount);
    const categories = new Map();

    for (let number = 0; number <= maximum; number++) {
        const id = String(number);
        const category = colorBy === "birth-order" ? number : colorBy === "parity" ?
            number % 2 : fibonacciNumbers.has(number);
        if (colorBy !== "birth-order" && !categories.has(category)) categories.set(category, categories.size);
        const edges = number === 0 ? [] : [N.edge(id, String(number - 1), {
            id: `chain:${number}`, strength, gradient: true, color: "#FFFFFFCC"
        })];
        yield N.batch([N.node(id, {
            label: id, color: colors[colorBy === "birth-order" ? number : categories.get(category)], radius: 1.6
        })], edges);
        yield N.wait(ticks);
    }

    for (let index = 1; index < fibonacci.length; index++) {
        const source = fibonacci[index - 1];
        const target = fibonacci[index];
        if (target - source <= 1) continue;
        yield N.batch([], [N.edge(String(source), String(target), {
            id: `fibonacci:${source}:${target}`,
            strength, gradient: true, color: "#FFFFFFCC"
        })]);
        yield N.wait(edgeTicks);
    }

    yield N.wait(finalTicks);
}
