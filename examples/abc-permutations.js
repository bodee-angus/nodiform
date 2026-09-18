/* @controls {
  "alphabet": {"type":"text","label":"Alphabet","default":"ABC"},
  "maxLength": {"type":"integer","label":"Maximum length","default":3,"min":1,"max":10},
  "repetitions": {"type":"boolean","label":"Repeat letters","default":false},
  "order": {"type":"select","label":"Birth order","default":"lexicographic","options":["lexicographic","reverse","shuffle","branch-walk"]},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":24,"min":0,"max":1000000},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":0,"min":0,"max":1000000}
} */
// ABC permutations, without repeated letters by default.
// Parameters: alphabet, maxLength, repetitions, order, ticksPerNode, finalTicks.
// Layer orders: "lexicographic", "reverse", or "shuffle" (seeded N.random()).
// "branch-walk" visits alternating branches in opposite directions.
// Metadata-free legacy scripts also support repeatLetters and reverse aliases.
function* generate(N, params) {
    const alphabet = Array.from(params.alphabet ?? "ABC");
    const maxLength = params.maxLength ?? 3;
    const repeat = params.repetitions ?? params.repeatLetters ?? false;
    const order = params.order ?? (params.reverse ? "reverse" : "lexicographic");
    const ticks = params.ticksPerNode ?? 24;
    const finalTicks = params.finalTicks ?? 0;
    if (!["lexicographic", "reverse", "shuffle", "branch-walk"].includes(order)) {
        throw new Error('order must be "lexicographic", "reverse", "shuffle", or "branch-walk"');
    }
    if (new Set(alphabet).size !== alphabet.length) throw new Error("Alphabet characters must be unique");
    if (alphabet.length < 1 || !Number.isInteger(maxLength) || maxLength < 1 || maxLength > 10) {
        throw new Error("Use a nonempty alphabet and an integer maxLength from 1 to 10");
    }
    // Count before allocating words: factorial growth must fit the graph budget.
    let layerCount = 1;
    let total = 0;
    for (let length = 1; length <= maxLength; length++) {
        layerCount *= repeat ? alphabet.length : Math.max(0, alphabet.length - length + 1);
        total += layerCount;
        if (total > 8192) throw new Error("This experiment exceeds 8192 nodes; reduce alphabet or maximum length");
    }
    const colors = N.palette(alphabet.length);
    const letterColors = new Map(alphabet.map((letter, index) => [letter, colors[index]]));
    function* words(length, prefix = "") {
        if (Array.from(prefix).length === length) { yield prefix; return; }
        for (const letter of alphabet) {
            if (repeat || !Array.from(prefix).includes(letter)) yield* words(length, prefix + letter);
        }
    }
    function* branch(prefix, backwards = false) {
        // A forward subtree starts at its prefix. Alternate child subtrees
        // forward/reversed; reversing a subtree also moves its prefix last.
        // Top-level groups stay in alphabet order. ABC therefore starts:
        // A, AB, ABC, ACB, AC, B, BA, BAC, BCA, BC, C, CA, CAB, CBA, CB.
        if (!backwards) yield prefix.join("");
        if (prefix.length < maxLength) {
            const children = alphabet.filter(letter => repeat || !prefix.includes(letter));
            for (let step = 0; step < children.length; step++) {
                const index = backwards ? children.length - 1 - step : step;
                yield* branch([...prefix, children[index]], backwards !== (index % 2 === 1));
            }
        }
        if (backwards) yield prefix.join("");
    }
    function* births() {
        if (order === "branch-walk") {
            for (const letter of alphabet) yield* branch([letter]);
            return;
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
            yield* layer;
        }
    }
    const born = new Set();
    const pending = new Map();
    for (const word of births()) {
        const letters = Array.from(word);
        // All contiguous windows of length n−1, with identical parents deduplicated.
        const parents = letters.length === 1 ? [] : [...new Set([
            letters.slice(0, -1).join(""), letters.slice(1).join("")
        ])];
        // A branch can visit a child first. Wait for the other endpoint,
        // then create the edge in the same batch as that endpoint's birth.
        const edges = pending.get(word) ?? [];
        pending.delete(word);
        for (const parent of parents) {
            const edge = N.edge(word, parent, {
                id: `${word}:${parent}`, gradient: true, color: "#ffffffcc", strength: 4
            });
            if (born.has(parent)) edges.push(edge);
            else {
                if (!pending.has(parent)) pending.set(parent, []);
                pending.get(parent).push(edge);
            }
        }
        born.add(word);
        yield N.batch(
            [N.node(word, { color: letterColors.get(letters[0]), radius: 1.6 })], edges
        );
        yield N.wait(ticks);
    }
    yield N.wait(finalTicks);
}
