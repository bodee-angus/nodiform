struct Style {
    color: vec4<f32>,
    radius: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}
struct Edge {
    source: u32,
    target_index: u32,
    strength: f32,
    gradient: u32,
    color: vec4<f32>,
}
struct Camera { center: vec2<f32>, half_extent: vec2<f32> }
struct Frame {
    width: f32, height: f32, count: u32, edges: u32,
    edge_width: f32, _pad0: f32, _pad1: f32, _pad2: f32,
}
@group(0) @binding(0) var<storage, read> positions: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read> styles: array<Style>;
@group(0) @binding(2) var<storage, read> edges: array<Edge>;
@group(0) @binding(3) var<storage, read> camera: Camera;
@group(0) @binding(4) var<uniform> frame: Frame;

fn corner(index: u32) -> vec2<f32> {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0)
    );
    return corners[index];
}
fn clip(world: vec2<f32>) -> vec4<f32> {
    let p = (world - camera.center) / camera.half_extent;
    return vec4<f32>(p.x, -p.y, 0.0, 1.0);
}
struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) local_pixels: vec2<f32>,
    @location(1) @interpolate(flat) half_pixels: vec2<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) end_color: vec4<f32>,
}

@vertex
fn node_vertex(@builtin(vertex_index) vertex: u32,
               @builtin(instance_index) index: u32) -> Vertex {
    let q = corner(vertex);
    let world_per_pixel = (camera.half_extent.y * 2.0) / frame.height;
    let radius_pixels = styles[index].radius / world_per_pixel;
    let local = q * (radius_pixels + 1.0);
    var output: Vertex;
    output.position = clip(positions[index] + local * world_per_pixel);
    output.local_pixels = local;
    output.half_pixels = vec2<f32>(radius_pixels);
    output.color = styles[index].color;
    output.end_color = output.color;
    return output;
}
@fragment
fn node_fragment(input: Vertex) -> @location(0) vec4<f32> {
    let coverage = clamp(input.half_pixels.x + 0.5 - length(input.local_pixels), 0.0, 1.0);
    return vec4<f32>(input.color.rgb, input.color.a * coverage);
}

@vertex
fn edge_vertex(@builtin(vertex_index) vertex: u32,
               @builtin(instance_index) index: u32) -> Vertex {
    let edge = edges[index];
    let a = positions[edge.source];
    let b = positions[edge.target_index];
    let delta = b - a;
    let distance = length(delta);
    var direction = vec2<f32>(1.0, 0.0);
    if distance > 0.000001 { direction = delta / distance; }
    let normal = vec2<f32>(-direction.y, direction.x);
    let world_per_pixel = (camera.half_extent.y * 2.0) / frame.height;
    // Keep world-space scaling up close, then stop at one physical pixel before
    // applying the appearance multiplier. Both preview and export use their
    // own target pixels, independently of monitor/UI scaling.
    let half_width_pixels = max(0.09 / world_per_pixel, 0.5) * frame.edge_width;
    let half_pixels = vec2<f32>(distance * 0.5 / world_per_pixel, half_width_pixels);
    let local = corner(vertex) * (half_pixels + vec2<f32>(1.0));
    let world = (a + b) * 0.5 + world_per_pixel * (direction * local.x + normal * local.y);
    var output: Vertex;
    output.position = clip(world);
    output.local_pixels = local;
    output.half_pixels = half_pixels;
    output.color = edge.color;
    output.end_color = edge.color;
    if edge.gradient != 0u {
        // Node styles are already linear-light RGB. Sample live endpoint
        // colours, preserving endpoint alpha and the edge colour's opacity.
        output.color = vec4<f32>(styles[edge.source].color.rgb,
            styles[edge.source].color.a * edge.color.a);
        output.end_color = vec4<f32>(styles[edge.target_index].color.rgb,
            styles[edge.target_index].color.a * edge.color.a);
    }
    return output;
}
// Integral of a square pixel projected onto one of the edge's local axes.
// The projection is a trapezoid (a triangle at 45 degrees), not a fixed-width
// ramp. Integrating it preserves thin-line brightness across orientations and
// subpixel positions, without multisample textures or extra render passes.
fn pixel_integral(point: f32, footprint: vec2<f32>) -> f32 {
    let major = max(max(footprint.x, footprint.y), 0.000001);
    let minor = min(footprint.x, footprint.y);
    if minor < 0.00001 {
        return clamp(0.5 + point / major, 0.0, 1.0);
    }
    let distance = abs(point);
    var positive = 0.5 + distance / major;
    if distance > 0.5 * (major - minor) {
        let tail = max(0.5 * (major + minor) - distance, 0.0);
        positive = 1.0 - tail * tail / (2.0 * major * minor);
    }
    return select(1.0 - positive, positive, point >= 0.0);
}
fn filtered_coverage(half_width: f32, point: f32, footprint: vec2<f32>) -> f32 {
    return clamp(pixel_integral(half_width - point, footprint)
        - pixel_integral(-half_width - point, footprint), 0.0, 1.0);
}

@fragment
fn edge_fragment(input: Vertex) -> @location(0) vec4<f32> {
    let dx = abs(dpdx(input.local_pixels));
    let dy = abs(dpdy(input.local_pixels));
    let coverage = vec2<f32>(
        filtered_coverage(input.half_pixels.x, input.local_pixels.x, vec2<f32>(dx.x, dy.x)),
        filtered_coverage(input.half_pixels.y, input.local_pixels.y, vec2<f32>(dx.y, dy.y)));
    let fraction = clamp(0.5 + input.local_pixels.x / max(2.0 * input.half_pixels.x, 0.000001), 0.0, 1.0);
    let color = mix(input.color, input.end_color, fraction);
    return vec4<f32>(color.rgb, color.a * coverage.x * coverage.y);
}
