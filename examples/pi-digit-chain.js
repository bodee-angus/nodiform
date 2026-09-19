/* @controls {
  "digits": {"type":"integer","label":"Digits of pi","description":"Includes the initial 3. The decimal point is ignored.","default":20,"min":1},
  "colorBy": {"type":"select","label":"Colour by","default":"digit","options":["digit","birth-order","node-role"]},
  "ticksPerDigit": {"type":"integer","label":"Ticks between digits","default":24,"min":0},
  "hubStrength": {"type":"number","label":"Hub connection strength","default":4,"min":0},
  "chainStrength": {"type":"number","label":"Sequence connection strength","default":4,"min":0},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":240,"min":0}
} */

// Ten hubs: 0 through 9.
// Occurrences: 3-1, 1-1, 4-1, 1-2, 5-1, ...
// Each occurrence connects to its hub and the previous occurrence.
// The first occurrence connects only to its hub.
// Colour by digit (matching hubs), full birth order, or hub/occurrence role.
// New colour categories receive successive hues; sequence edges use gradients.

function* generate(N, params) {
    const count = params.digits ?? 20;
    const ticks = params.ticksPerDigit ?? 24;
    const hubStrength = params.hubStrength ?? 4;
    const chainStrength = params.chainStrength ?? 4;
    const finalTicks = params.finalTicks ?? 240;
    const colorBy = params.colorBy ?? "digit";

    if (!Number.isSafeInteger(count) || count < 1) {
        throw new Error("Digits of pi must be a positive safe integer.");
    }
    if (!Number.isSafeInteger(count + 10)) {
        throw new Error("Digits plus the ten hub nodes exceed JavaScript's safe integer range.");
    }
    if (!["digit", "birth-order", "node-role"].includes(colorBy)) {
        throw new Error('Colour by must be "digit", "birth-order", or "node-role".');
    }

    // Exact integer arithmetic emits one certified digit at a time.
    // BigInt avoids the precision limit of Math.PI.
    function* piDigits() {
        let q = 1n, r = 0n, t = 1n, k = 1n;

        while (true) {
            const digit = (3n * q + r) / t;

            if (digit === (4n * q + r) / t) {
                yield Number(digit);
                r = 10n * (r - digit * t);
                q *= 10n;
            } else {
                const factor = 2n * k + 1n;
                r = (2n * q + r) * factor;
                q *= k;
                t *= factor;
                k++;
            }
        }
    }

    const colors = N.palette(colorBy === "birth-order" ? count + 10 : colorBy === "node-role" ? 2 : 10);
    const categories = new Map();
    function colorFor(digit, index, isHub) {
        const category = colorBy === "birth-order" ? index : colorBy === "node-role" ?
            isHub ? "hub" : "occurrence" : digit;
        if (colorBy !== "birth-order" && !categories.has(category)) categories.set(category, categories.size);
        return colors[colorBy === "birth-order" ? index : categories.get(category)];
    }
    const occurrences = Array(10).fill(0);

    // Hubs have no connections until their digit appears.
    yield N.batch(
        Array.from({length: 10}, (_, digit) => N.node(String(digit), {
            label: String(digit), color: colorFor(digit, digit, true), radius: 2.4
        })),
        []
    );

    const sequence = piDigits();
    let previous = null;

    for (let index = 0; index < count; index++) {
        const digit = sequence.next().value;
        const id = `${digit}-${++occurrences[digit]}`;
        const edges = [N.edge(id, String(digit), {
            id: `hub:${id}`,
            strength: hubStrength,
            gradient: true,
            color: "#FFFFFFCC"
        })];

        if (previous !== null) {
            edges.push(N.edge(id, previous, {
                id: `chain:${previous}:${id}`,
                strength: chainStrength,
                gradient: true,
                color: "#FFFFFFCC"
            }));
        }

        yield N.batch(
            [N.node(id, { label: id, color: colorFor(digit, index + 10, false), radius: 1.6 })],
            edges
        );
        previous = id;
        yield N.wait(ticks);
    }

    yield N.wait(finalTicks);
}
