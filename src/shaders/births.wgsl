// A single invocation resolves births in rule order, using live coordinates.
// Both ping-pong buffers receive each birth before the next force dispatch.
struct Birth {
    offset: u32,
    count: u32,
    automatic: u32,
    padding: u32,
};

struct Parameters {
    first: u32,
    count: u32,
    padding: vec2<u32>,
};

@group(0) @binding(0) var<storage, read_write> positions: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> mirror: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> births: array<Birth>;
@group(0) @binding(3) var<storage, read> neighbours: array<u32>;
@group(0) @binding(4) var<uniform> params: Parameters;

@compute @workgroup_size(1)
fn main() {
    for (var i = params.first; i < params.count; i += 1u) {
        let birth = births[i];
        var position = positions[i];
        if (birth.automatic != 0u) {
            var centre = vec2<f32>(0.0);
            for (var n = 0u; n < birth.count; n += 1u) {
                let parent = neighbours[birth.offset + n];
                centre += positions[parent];
            }
            if (birth.count > 0u) {
                centre /= f32(birth.count);
            }
            // Automatic positions initially contain only the seeded ID offset.
            position += centre;
        }
        positions[i] = position;
        mirror[i] = position;
    }
}
