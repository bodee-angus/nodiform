/* @controls {
  "alphabet": {"type":"text","label":"Alphabet","default":"ABC"},
  "maxLength": {"type":"integer","label":"Maximum length","default":3,"min":1},
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
    if (alphabet.length < 1 || !Number.isSafeInteger(maxLength) || maxLength < 1) {
        throw new Error("Use a nonempty alphabet and a positive safe integer maxLength");
    }
    // Without repetition, no word can exceed the number of available letters.
    const depthLimit = repeat ? maxLength : Math.min(maxLength, alphabet.length);
    const colors = N.palette(alphabet.length);
    const letterColors = new Map(alphabet.map((letter, index) => [letter, colors[index]]));
    const sortedAlphabet = [...alphabet].sort();
    function* words(length, letters = sortedAlphabet) {
        // Keep traversal state on an explicit stack, so depth is not limited
        // by JavaScript recursion. Ordered layers stream one word at a time.
        const prefix = [];
        const next = [0];
        while (next.length) {
            if (prefix.length === length) {
                yield prefix.join("");
                prefix.pop();
                next.pop();
                continue;
            }
            const index = next[prefix.length]++;
            if (index === letters.length) {
                next.pop();
                if (prefix.length) prefix.pop();
                continue;
            }
            const letter = letters[index];
            if (!repeat && prefix.includes(letter)) continue;
            prefix.push(letter);
            next.push(0);
        }
    }
    function* branch(root) {
        // A forward subtree starts at its prefix. Alternate child subtrees
        // forward/reversed; reversing a subtree also moves its prefix last.
        // Top-level groups stay in alphabet order. ABC therefore starts:
        // A, AB, ABC, ACB, AC, B, BA, BAC, BCA, BC, C, CA, CAB, CBA, CB.
        const prefix = [root];
        const stack = [{ backwards: false, children: null, step: 0 }];
        while (stack.length) {
            const frame = stack[stack.length - 1];
            if (frame.children === null) {
                if (!frame.backwards) yield prefix.join("");
                frame.children = prefix.length < depthLimit
                    ? alphabet.filter(letter => repeat || !prefix.includes(letter))
                    : [];
            }
            if (frame.step < frame.children.length) {
                const index = frame.backwards
                    ? frame.children.length - 1 - frame.step
                    : frame.step;
                frame.step++;
                prefix.push(frame.children[index]);
                stack.push({
                    backwards: frame.backwards !== (index % 2 === 1),
                    children: null,
                    step: 0
                });
            } else {
                if (frame.backwards) yield prefix.join("");
                stack.pop();
                prefix.pop();
            }
        }
    }
    function* births() {
        if (order === "branch-walk") {
            for (const letter of alphabet) yield* branch(letter);
            return;
        }
        const letters = order === "reverse" ? [...sortedAlphabet].reverse() : sortedAlphabet;
        for (let length = 1; length <= depthLimit; length++) {
            if (order === "shuffle") {
                // Shuffling needs the current layer in memory; ordered modes
                // and branch-walk do not allocate an entire layer up front.
                const layer = Array.from(words(length));
                for (let i = layer.length - 1; i > 0; i--) {
                    const j = Math.floor(N.random() * (i + 1));
                    [layer[i], layer[j]] = [layer[j], layer[i]];
                }
                yield* layer;
            } else yield* words(length, letters);
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
