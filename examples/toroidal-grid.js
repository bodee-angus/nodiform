/* @controls {
  "dimensions": {"type":"integer","label":"Dimension","description":"Number of wrapping coordinate axes. The simulation is still displayed in 2D.","default":2,"min":1},
  "range": {"type":"integer","label":"Range","description":"Values 1 through Range on every axis. Total nodes = Range raised to Dimension.","default":10,"min":1},
  "colorBy": {"type":"select","label":"Colour by","default":"first-coordinate","options":["first-coordinate","last-coordinate","coordinate-sum","birth-order"]},
  "ticksPerNode": {"type":"integer","label":"Ticks between births","default":6,"min":0,"max":4294967295},
  "strength": {"type":"number","label":"Connection strength","default":4,"min":0,"max":1000000},
  "finalTicks": {"type":"integer","label":"Final settling ticks","default":240,"min":0,"max":4294967295}
} */

// A toroidal grid: each coordinate axis forms a ring, wrapping Range back to 1.
// Dimension 1, Range 10: 10 nodes and 10 edges, forming a ring.
// Dimension 2, Range 10: 100 nodes and 200 edges, a grid with both axes wrapping.
// Dimension changes the connections, not the renderer's two-dimensional space.
// Colour bands may follow the first or last coordinate, the coordinate sum
// modulo Range (diagonal bands), or full birth order. New hues appear in order.
// Range 1 omits self-loops; Range 2 uses one edge per pair, not parallel edges.

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

    // Node IDs use the actual coordinates. Check their largest possible size
    // before allocating the coordinate array, including when Range is just 1.
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

    // An odometer visits coordinates in numerical lexicographic order:
    // 1-1, 1-2, ... 1-10, 2-1, ... 10-10. No table of all tuples is needed.
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

            // The closing neighbour already exists. With Range 2 it is the
            // same neighbour as above, so adding it again would double force.
            if (value === range && range > 2) {
                coordinates[axis] = 1;
                edges.push(N.edge(id, coordinates.join("-"), {
                    id: `wrap:${index}:${axis}`,
                    strength, gradient: true, color: "#ffffffcc"
                }));
            }
            coordinates[axis] = value;
        }

        // Accumulate modulo Range without letting an intermediate sum exceed
        // the safe-integer range when coordinates themselves are still valid.
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
