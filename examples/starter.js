// A small starting point. Change the loop, connections, colours or timing freely.
function build(graph) {
    const count = 8;
    const colors = graph.palette(4);
    for (let n = 1; n <= count; n++) {
        graph.add(n, { color: colors[(n - 1) % colors.length] });
        if (n > 1) graph.connect(n, n - 1, { strength: 4, gradient: true, color: "#ffffffcc" });
        graph.wait(24);
    }
}
