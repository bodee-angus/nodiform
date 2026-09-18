// Nodiform Force v3: weighted zero-rest springs and exact softened repulsion.
// Damped second-order steps retain 85% of the prior tick displacement. A shared
// force step limits attraction stiffness; the final displacement cap also caps
// stored momentum, preventing accumulated velocity behind the cap. No gravity.
struct Parameters {
    count: u32,
    _pad0: u32,
    _pad1: u32,
    momentum_retention: f32,
    repulsion: f32,
    dt: f32,
    softening_squared: f32,
    max_displacement: f32,
}

struct Neighbour {
    other: u32,
    _pad0: u32,
    strength: f32,
    _pad1: f32,
}

@group(0) @binding(0) var<storage, read> previous: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> next: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> offsets: array<u32>;
@group(0) @binding(3) var<storage, read> neighbours: array<Neighbour>;
@group(0) @binding(4) var<uniform> parameters: Parameters;
@group(0) @binding(5) var<storage, read_write> momentum: array<vec2<f32>>;

var<workgroup> tile: array<vec2<f32>, 128>;

@compute @workgroup_size(128)
fn main(@builtin(workgroup_id) wid: vec3<u32>,
        @builtin(num_workgroups) workgroups: vec3<u32>,
        @builtin(local_invocation_id) lid: vec3<u32>) {
    let i = (wid.y * workgroups.x + wid.x) * 128u + lid.x;
    let enabled = i < parameters.count;
    var p = vec2<f32>(0.0);
    if enabled { p = previous[i]; }
    var force = vec2<f32>(0.0);

    // Inactive lanes must still participate in both barriers.
    for (var base = 0u; base < parameters.count; base += 128u) {
        let j = base + lid.x;
        tile[lid.x] = vec2<f32>(0.0);
        if j < parameters.count { tile[lid.x] = previous[j]; }
        workgroupBarrier();
        if enabled {
            let tile_count = min(128u, parameters.count - base);
            for (var k = 0u; k < tile_count; k += 1u) {
                if base + k != i {
                    let delta = p - tile[k];
                    let denominator = dot(delta, delta) + parameters.softening_squared;
                    force += parameters.repulsion * delta / denominator;
                }
            }
        }
        workgroupBarrier();
    }
    if !enabled { return; }

    // Each undirected edge is present in the adjacency of both endpoints.
    // Total attraction work is O(E), without float atomics or an E loop/node.
    for (var j = offsets[i]; j < offsets[i + 1u]; j += 1u) {
        let neighbour = neighbours[j];
        force += neighbour.strength * (previous[neighbour.other] - p);
    }
    var movement = parameters.momentum_retention * momentum[i] + parameters.dt * force;
    // Scale before taking a norm so large, finite force sums cannot overflow
    // the length's squared components and accidentally freeze the node.
    let largest_component = max(abs(movement.x), abs(movement.y));
    if largest_component > 0.0 {
        let direction = movement / largest_component;
        let direction_length = length(direction);
        if largest_component > parameters.max_displacement / direction_length {
            movement = direction * (parameters.max_displacement / direction_length);
        }
    }
    momentum[i] = movement;
    next[i] = p + movement;
}
