struct Style {
    color: vec4<f32>,
    radius: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}
struct Camera { center: vec2<f32>, half_extent: vec2<f32> }
struct Frame {
    width: f32,
    height: f32,
    count: u32,
    edges: u32,
}
@group(0) @binding(0) var<storage, read> positions: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read> styles: array<Style>;
@group(0) @binding(2) var<storage, read_write> camera: Camera;
@group(0) @binding(3) var<uniform> frame: Frame;

// One GPU lane is deliberately used for this bounded 8,192-node first engine.
// Bounds never leave the GPU and include each node's world-space radius.
@compute @workgroup_size(1)
fn main() {
    let aspect = frame.width / max(frame.height, 1.0);
    if frame.count == 0u {
        camera.center = vec2<f32>(0.0);
        camera.half_extent = vec2<f32>(10.0 * aspect, 10.0);
        return;
    }
    var lower = vec2<f32>(3.402823e38);
    var upper = vec2<f32>(-3.402823e38);
    for (var i = 0u; i < frame.count; i += 1u) {
        let radius = vec2<f32>(styles[i].radius);
        lower = min(lower, positions[i] - radius);
        upper = max(upper, positions[i] + radius);
    }
    let half = (upper - lower) * 0.5;
    let required = max(max(half.y, half.x / aspect) * 1.08, 1.0);
    // Centre tracks the exact bounding midpoint. Width expands immediately,
    // contracts by 2% per rendered simulation sample, never by wall-clock time.
    let old = camera.half_extent.y;
    var chosen = required;
    if old > required { chosen = mix(old, required, 0.02); }
    camera.center = (lower + upper) * 0.5;
    camera.half_extent = vec2<f32>(chosen * aspect, chosen);
}
