/* @controls {
  "colorBy": {"type":"select","label":"Colour by","default":"birth-order","options":["birth-order","alternating","single"]}
} */
// A small starting point. Change the loop, connections, colours or timing freely.
function build(graph, p) {
    const count = 8;
    const mode = p.colorBy ?? "birth-order";
    const colors = graph.palette(mode === "birth-order" ? count : mode === "alternating" ? 2 : 1);
    for (let n = 1; n <= count; n++) {
        const index = mode === "birth-order" ? n - 1 : mode === "alternating" ? (n - 1) % 2 : 0;
        graph.add(n, { color: colors[index] });
        if (n > 1) graph.connect(n, n - 1, { strength: 4, gradient: true, color: "#ffffffcc" });
        graph.wait(24);
    }
}
