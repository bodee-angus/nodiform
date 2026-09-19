/* @controls {
  "count": {"type":"integer","label":"Nodes","default":500,"min":1},
  "connectToOne": {"type":"boolean","label":"Connect to 1","description":"Add 1 as a shared anchor. It is not a prime factor.","default":true},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":6,"min":0},
  "strength": {"type":"number","label":"Base edge strength","description":"Prime-factor edges multiply this by the factor's exponent.","default":4,"min":0},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":240,"min":0}
} */

// Generate 1 through count. Connect to distinct prime factors, excluding self-links.
// 15 = 3 × 5: both prime-factor edges use the base strength.
// 16 = 2^4: its edge to 2 uses four times the base strength.
// 1 is not prime. Its optional anchor edges always use the base strength.
// Three equally vivid palette colours distinguish 1, primes, and composites.
function* generate(N, params) {
    const count = params.count ?? 500;
    const connectToOne = params.connectToOne ?? true;
    const ticks = params.ticksPerNode ?? 6;
    const strength = params.strength ?? 4;
    const finalTicks = params.finalTicks ?? 240;

    if (!Number.isSafeInteger(count) || count < 1) {
        throw new Error("Nodes must be a positive safe integer.");
    }

    const colors = N.palette(3);
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
        const color = colors[number === 1 ? 0 : isPrime ? 1 : 2];
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
