/* @controls {
  "dimensions": {"type":"integer","label":"Dimension","description":"Number of coordinate axes. The simulation is still displayed in 2D.","default":2,"min":1},
  "range": {"type":"integer","label":"Range","description":"Values 1 through Range on every axis. Total nodes = Range raised to Dimension.","default":10,"min":1},
  "colorBy": {"type":"select","label":"Colour by","default":"first-coordinate","options":["first-coordinate","last-coordinate","coordinate-sum","birth-order"]},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":6,"min":0,"max":4294967295},
  "strength": {"type":"number","label":"Connection strength","default":4,"min":0,"max":1000000},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":240,"min":0,"max":4294967295}
} */

// A grid with open boundaries: each coordinate axis is a path from 1 to Range.
// Dimension 1, Range 10: 10 nodes and 9 edges, forming a path.
// Dimension 2, Range 10: 100 nodes and 180 edges, with no wrapping connections.
// Dimension changes the connections, not the renderer's two-dimensional space.
// Colour bands may follow the first or last coordinate, the coordinate sum
// modulo Range (diagonal bands), or full birth order. New hues appear in order.
// Range 1 produces a single node, without self-loops.

function* generate(N, params) {
    const dimensions = params.dimensions ?? 2;
    const range = params.range ?? 10;
    const ticks = params.ticksPerNode ?? 6;
    const strength = params.strength ?? 4;
    const finalTicks = params.finalTicks ?? 240;
    const colorBy = params.colorBy ?? "first-coordinate";

    if (!Number.isSafeInteger(dimensions) || dimensions < 1 ||
        !Number.isSafeInteger(range) || range < 1) {
        throw new Error("Dimension and Range must be positive safe integers.");
    }
    if (!["first-coordinate", "last-coordinate", "coordinate-sum", "birth-order"].includes(colorBy)) {
        throw new Error('Colour by must be "first-coordinate", "last-coordinate", "coordinate-sum", or "birth-order".');
    }

    // Check the actual node ID representation before allocating coordinates,
    // including when Range is 1 and the whole grid contains only one node.
    if (dimensions > Math.floor(257 / (String(range).length + 1))) {
        throw new Error("These coordinates exceed Nodiform's 256-byte node ID limit; reduce Dimension or Range.");
    }

    let count = 1;
    for (let axis = 0; axis < dimensions; axis++) {
        count *= range;
        if (!Number.isSafeInteger(count)) {
            throw new Error("Range raised to Dimension exceeds JavaScript's safe integer range.");
        }
    }

    const colors = N.palette(colorBy === "birth-order" ? count : range);
    const categories = new Map();
    const coordinates = Array(dimensions).fill(1);

    // Numerical lexicographic order: 1-1, 1-2, ... 1-10, 2-1, ... 10-10.
    // Each predecessor already exists, so every birth after the first connects
    // immediately. Coordinates describe topology and do not fix node positions.
    for (let index = 0; index < count; index++) {
        const id = coordinates.join("-");
        const edges = [];

        for (let axis = 0; axis < dimensions; axis++) {
            const value = coordinates[axis];
            if (value === 1) continue;

            coordinates[axis] = value - 1;
            edges.push(N.edge(id, coordinates.join("-"), {
                id: `step:${index}:${axis}`,
                strength, gradient: true, color: "#ffffffcc"
            }));
            coordinates[axis] = value;
        }

        // Accumulate modulo Range without overflowing a safe integer.
        const category = colorBy === "birth-order" ? index : colorBy === "last-coordinate" ?
            coordinates[dimensions - 1] - 1 : colorBy === "coordinate-sum" ?
            coordinates.reduce((sum, value) => {
                const offset = value - 1;
                return sum >= range - offset ? sum - (range - offset) : sum + offset;
            }, 0) : coordinates[0] - 1;
        if (colorBy !== "birth-order" && !categories.has(category)) categories.set(category, categories.size);
        yield N.batch(
            [N.node(id, {
                label: id, color: colors[colorBy === "birth-order" ? index : categories.get(category)], radius: 1.6
            })],
            edges
        );
        yield N.wait(ticks);

        for (let axis = dimensions - 1; axis >= 0; axis--) {
            if (coordinates[axis] < range) {
                coordinates[axis]++;
                break;
            }
            coordinates[axis] = 1;
        }
    }

    yield N.wait(finalTicks);
}
