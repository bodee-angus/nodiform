/* @controls {
  "alphabet": {"type":"text","label":"Alphabet","default":"ABC"},
  "maxLength": {"type":"integer","label":"Maximum length","default":3,"min":1,"max":10},
  "repetitions": {"type":"boolean","label":"Repeat letters","default":false},
  "order": {"type":"select","label":"Birth order","default":"lexicographic","options":["lexicographic","reverse","shuffle"]},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":24,"min":0,"max":1000000},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":0,"min":0,"max":1000000}
} */
// ABC permutations, without repeated letters by default.
// Parameters: alphabet, maxLength, repetitions, order, ticksPerNode, finalTicks.
// order: "lexicographic", "reverse", or "shuffle" (uses seeded N.random()).
// Metadata-free legacy scripts also support repeatLetters and reverse aliases.
function* generate(N, params) {
    const alphabet = Array.from(params.alphabet ?? "ABC");
    const maxLength = params.maxLength ?? 3;
    const repeat = params.repetitions ?? params.repeatLetters ?? false;
    const order = params.order ?? (params.reverse ? "reverse" : "lexicographic");
    const ticks = params.ticksPerNode ?? 24;
    const finalTicks = params.finalTicks ?? 0;
    if (!["lexicographic", "reverse", "shuffle"].includes(order)) {
        throw new Error('order must be "lexicographic", "reverse", or "shuffle"');
    }
    if (new Set(alphabet).size !== alphabet.length) throw new Error("Alphabet characters must be unique");
    if (alphabet.length < 1 || !Number.isInteger(maxLength) || maxLength < 1 || maxLength > 10) {
        throw new Error("Use a nonempty alphabet and an integer maxLength from 1 to 10");
    }
    const colors = ["#89b4fa", "#cba6f7", "#f5c2e7", "#fab387", "#a6e3a1"];
    function* words(length, prefix = "") {
        if (Array.from(prefix).length === length) { yield prefix; return; }
        for (const letter of alphabet) {
            if (repeat || !Array.from(prefix).includes(letter)) yield* words(length, prefix + letter);
        }
    }
    for (let length = 1; length <= maxLength; length++) {
        const layer = Array.from(words(length)).sort();
        if (order === "reverse") layer.reverse();
        if (order === "shuffle") {
            for (let i = layer.length - 1; i > 0; i--) {
                const j = Math.floor(N.random() * (i + 1));
                [layer[i], layer[j]] = [layer[j], layer[i]];
            }
        }
        for (const word of layer) {
            const letters = Array.from(word);
            // All contiguous windows of length n−1, with identical parents deduplicated.
            const parents = length === 1 ? [] : [...new Set([
                letters.slice(0, -1).join(""), letters.slice(1).join("")
            ])];
            yield N.batch(
                [N.node(word, { color: colors[(length - 1) % colors.length], radius: 1.6 })],
                parents.map(parent => N.edge(word, parent, {
                    id: `${word}:${parent}`, color: "#7f8fa6aa", strength: 1
                }))
            );
            yield N.wait(ticks);
        }
    }
    yield N.wait(finalTicks);
}
