/* @controls {
  "count": {"type":"integer","label":"Nodes","default":500,"min":1},
  "colorBy": {"type":"select","label":"Colour by","default":"number-type","options":["number-type","birth-order","factor-count"]},
  "connectToOne": {"type":"boolean","label":"Connect to 1","description":"Add 1 as a shared anchor. It is not a prime factor.","default":true},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":6,"min":0},
  "strength": {"type":"number","label":"Base edge strength","description":"Prime-factor edges multiply this by the factor's exponent.","default":4,"min":0},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":240,"min":0}
} */

// Generate 1 through count. Connect to distinct prime factors, excluding self-links.
// 15 = 3 × 5: both prime-factor edges use the base strength.
// 16 = 2^4: its edge to 2 uses four times the base strength.
// 1 is not prime. Its optional anchor edges always use the base strength.
// Colour by number type (1 / prime / composite), birth order, or total prime
// factor count including repetitions. New categories follow successive hues.
function* generate(N, params) {
    const count = params.count ?? 500;
    const connectToOne = params.connectToOne ?? true;
    const ticks = params.ticksPerNode ?? 6;
    const strength = params.strength ?? 4;
    const finalTicks = params.finalTicks ?? 240;
    const colorBy = params.colorBy ?? "number-type";

    if (!Number.isSafeInteger(count) || count < 1) {
        throw new Error("Nodes must be a positive safe integer.");
    }

    if (!["number-type", "birth-order", "factor-count"].includes(colorBy)) {
        throw new Error('Colour by must be "number-type", "birth-order", or "factor-count".');
    }
    let maximumFactorCount = 0;
    for (let power = 2; power <= count; power *= 2) maximumFactorCount++;
    const categoryCount = colorBy === "birth-order" ? count : colorBy === "factor-count" ?
        maximumFactorCount + 1 : count >= 4 ? 3 : count >= 2 ? 2 : 1;
    const colors = N.palette(categoryCount);
    const categories = new Map();
    const primes = [];

    function primeFactors(number) {
        const factors = [];
        let remaining = number;
        for (const prime of primes) {
            if (prime > remaining / prime) break;
            if (remaining % prime === 0) {
                let exponent = 0;
                do {
                    remaining /= prime;
                    exponent++;
                } while (remaining % prime === 0);
                factors.push({ prime, exponent });
            }
        }
        if (remaining > 1) factors.push({ prime: remaining, exponent: 1 });
        return factors;
    }

    for (let number = 1; number <= count; number++) {
        const factors = primeFactors(number);
        const isPrime = factors.length === 1 && factors[0].prime === number;
        if (isPrime) primes.push(number);

        const targets = factors.filter(factor => factor.prime < number);
        if (connectToOne && number > 1) targets.unshift({ prime: 1, exponent: 1 });

        const id = String(number);
        const category = colorBy === "birth-order" ? number : colorBy === "factor-count" ?
            factors.reduce((sum, factor) => sum + factor.exponent, 0) :
            number === 1 ? "one" : isPrime ? "prime" : "composite";
        if (colorBy !== "birth-order" && !categories.has(category)) categories.set(category, categories.size);
        const color = colors[colorBy === "birth-order" ? number - 1 : categories.get(category)];
        const edges = targets.map(({ prime, exponent }) => N.edge(id, String(prime), {
            id: `${number}:${prime}`,
            strength: strength * exponent,
            gradient: true,
            color: "#FFFFFFCC"
        }));

        yield N.batch([N.node(id, { label: id, color, radius: 1.6 })], edges);
        yield N.wait(ticks);
    }

    yield N.wait(finalTicks);
}
