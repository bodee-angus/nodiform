// A small starting point. Change the loop, connections, colours or timing freely.
function build(graph) {
    const count = 8;
    for (let n = 1; n <= count; n++) {
        graph.add(n, { color: "#8ecbff" });
        if (n > 1) graph.connect(n, n - 1, { strength: 1 });
        graph.wait(24);
    }
}
