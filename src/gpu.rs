//! Strictly 2D, all-GPU simulation and rendering on eframe's existing device.
//!
//! Force v3 uses rho=512, epsilon²=0.25 and a 2-world-unit displacement cap.
//! Each tick retains 85% of the previous displacement and adds the complete
//! force times min(1/120, 0.5 / maximum weighted node degree). This damped
//! second-order update preserves momentum while limiting attraction stiffness.
//! Every node has the same charge. Springs have zero rest length; visual radii
//! do not change forces. Damping removes kinetic energy without adding gravity.
//! This is not a proof of energy-monotonic descent or a global minimum.
//!
//! Repulsion is exact O(N²), attraction is O(E). This dynamically sized engine is
//! not a Barnes-Hut implementation and has no claimed hardware throughput.

use std::sync::{mpsc, Arc};

use bytemuck::{Pod, Zeroable};
use eframe::wgpu::{self, util::DeviceExt};

use crate::model::{
    Graph, BASE_TIMESTEP, MAX_DISPLACEMENT, MOMENTUM_RETENTION, REPULSION, SOFTENING_SQUARED,
};
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct NodeStyle {
    color: [f32; 4],
    radius: f32,
    padding: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuEdge {
    source: u32,
    target: u32,
    strength: f32,
    gradient: u32,
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Neighbour {
    other: u32,
    padding0: u32,
    strength: f32,
    padding1: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PhysicsParameters {
    count: u32,
    padding: [u32; 2],
    momentum_retention: f32,
    repulsion: f32,
    dt: f32,
    softening_squared: f32,
    max_displacement: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameParameters {
    // The camera bounds shader reads the first 16 bytes; the renderer also
    // reads appearance settings from the second, explicitly padded block.
    width: f32,
    height: f32,
    count: u32,
    edges: u32,
    edge_width: f32,
    padding: [f32; 3],
}

/// Storage counts grow geometrically, bounded only by the selected GPU's
/// actual binding/buffer sizes and the shader's 32-bit indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Capacity {
    nodes: usize,
    edges: usize,
}

impl Capacity {
    fn for_counts(
        nodes: usize,
        edges: usize,
        old_nodes: usize,
        old_edges: usize,
        limits: &wgpu::Limits,
    ) -> Result<Self, String> {
        let bytes = limits
            .max_buffer_size
            .min(u64::from(limits.max_storage_buffer_binding_size));
        let node_limit = (bytes / 32)
            .min((bytes / 4).saturating_sub(1))
            .min(u64::from(u32::MAX - 127));
        let edge_limit = (bytes / 32).min(u64::from(u32::MAX / 2));
        if nodes as u64 > node_limit {
            return Err(format!("This graph needs {nodes} nodes, exceeding this GPU's per-buffer capacity of {node_limit}. Use a smaller experiment or a GPU with larger storage-buffer limits."));
        }
        if edges as u64 > edge_limit {
            return Err(format!("This graph needs {edges} edges, exceeding this GPU's per-buffer capacity of {edge_limit}. Use fewer connections or a GPU with larger storage-buffer limits."));
        }
        if node_limit == 0 || edge_limit == 0 {
            return Err(
                "This GPU cannot provide the storage buffers needed for graph simulation".into(),
            );
        }
        let (x, y) = dispatch_grid(nodes as u32, limits.max_compute_workgroups_per_dimension);
        if y > limits.max_compute_workgroups_per_dimension
            || u64::from(x) * u64::from(y) * 128 > u64::from(u32::MAX)
        {
            return Err("This graph exceeds this GPU's compute-dispatch or 32-bit shader-addressing capacity".into());
        }
        let grow = |needed: usize, old: usize, maximum: u64| {
            if needed <= old && old != 0 {
                old
            } else {
                needed
                    .max(old.saturating_mul(2))
                    .max(64)
                    .min(maximum as usize)
            }
        };
        Ok(Self {
            nodes: grow(nodes, old_nodes, node_limit),
            edges: grow(edges, old_edges, edge_limit),
        })
    }
}

fn dispatch_grid(nodes: u32, maximum: u32) -> (u32, u32) {
    let groups = nodes.div_ceil(128).max(1);
    let x = groups.min(maximum.max(1));
    (x, groups.div_ceil(x))
}

struct GraphStorage {
    capacity: Capacity,
    positions: [wgpu::Buffer; 2],
    momentum: wgpu::Buffer,
    styles: wgpu::Buffer,
    edges: wgpu::Buffer,
    offsets: wgpu::Buffer,
    neighbours: wgpu::Buffer,
}

impl GraphStorage {
    fn new(device: &wgpu::Device, capacity: Capacity) -> Self {
        Self {
            capacity,
            positions: std::array::from_fn(|_| {
                storage(device, "node positions", capacity.nodes * 8, true)
            }),
            momentum: storage(
                device,
                "tick displacement momentum",
                capacity.nodes * 8,
                true,
            ),
            styles: storage(device, "node styles", capacity.nodes * 32, true),
            edges: storage(device, "edges", capacity.edges * 32, true),
            offsets: storage(device, "adjacency offsets", (capacity.nodes + 1) * 4, true),
            neighbours: storage(device, "adjacency entries", capacity.edges * 32, true),
        }
    }

    fn grow(&self, device: &wgpu::Device, capacity: Capacity) -> Self {
        let nodes_grew = capacity.nodes != self.capacity.nodes;
        let edges_grew = capacity.edges != self.capacity.edges;
        let resized = |old: &wgpu::Buffer, changed, label, bytes, copy_source| {
            if changed {
                storage(device, label, bytes, copy_source)
            } else {
                old.clone()
            }
        };
        Self {
            capacity,
            positions: std::array::from_fn(|i| {
                resized(
                    &self.positions[i],
                    nodes_grew,
                    "node positions",
                    capacity.nodes * 8,
                    true,
                )
            }),
            momentum: resized(
                &self.momentum,
                nodes_grew,
                "tick displacement momentum",
                capacity.nodes * 8,
                true,
            ),
            styles: resized(
                &self.styles,
                nodes_grew,
                "node styles",
                capacity.nodes * 32,
                true,
            ),
            edges: resized(&self.edges, edges_grew, "edges", capacity.edges * 32, true),
            offsets: resized(
                &self.offsets,
                nodes_grew,
                "adjacency offsets",
                (capacity.nodes + 1) * 4,
                true,
            ),
            neighbours: resized(
                &self.neighbours,
                edges_grew,
                "adjacency entries",
                capacity.edges * 32,
                true,
            ),
        }
    }
}

/// Native wgpu resolves these error scopes without a GPU readback. Only
/// successful allocations replace live state; OOM/limit failures reach the UI.
fn gpu_allocation_scope<T>(
    device: &wgpu::Device,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = operation();
    let validation = pollster::block_on(device.pop_error_scope());
    let memory = pollster::block_on(device.pop_error_scope());
    if let Some(error) = memory {
        return Err(format!("GPU memory was exhausted while growing this graph. Close other GPU applications or reduce the experiment: {error}"));
    }
    if let Some(error) = validation {
        return Err(format!(
            "The GPU could not allocate or upload this graph: {error}"
        ));
    }
    result
}

struct GraphUpload {
    reset: bool,
    first: usize,
    births: Vec<[f32; 2]>,
    rest: Vec<[f32; 2]>,
    base_styles: Vec<NodeStyle>,
    edges: Vec<GpuEdge>,
    offsets: Vec<u32>,
    neighbours: Vec<Neighbour>,
    incident_degrees: Vec<u32>,
    effective_dt: f32,
    birth_plan: crate::births::BirthPlan,
}

pub struct GpuGraph {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    storage: GraphStorage,
    births: crate::births::BirthInitializer,
    physics_layout: wgpu::BindGroupLayout,
    bounds_layout: wgpu::BindGroupLayout,
    render_layout: wgpu::BindGroupLayout,
    physics_parameters: wgpu::Buffer,
    physics_pipeline: wgpu::ComputePipeline,
    bounds_pipeline: wgpu::ComputePipeline,
    node_pipeline: wgpu::RenderPipeline,
    edge_pipeline: wgpu::RenderPipeline,
    physics_groups: [wgpu::BindGroup; 2],
    active: usize,
    node_count: usize,
    edge_count: usize,
    effective_dt: f32,
    base_styles: Vec<NodeStyle>,
    incident_degrees: Vec<u32>,
    degree_sizing: bool,
    edge_width: f32,
    output: FrameTarget,
    preview: FrameTarget,
}

/// Each view owns its camera history, so preview resizing or refresh cadence
/// cannot alter the reproducible export camera or the selected video size.
struct FrameTarget {
    frame_parameters: wgpu::Buffer,
    camera: wgpu::Buffer,
    bounds_groups: [wgpu::BindGroup; 2],
    render_groups: [wgpu::BindGroup; 2],
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    dimensions: (u32, u32),
}

impl GpuGraph {
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Result<Self, String> {
        let scope_device = device.clone();
        gpu_allocation_scope(&scope_device, || Self::create(device, queue))
    }

    fn create(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Result<Self, String> {
        let capacity = Capacity::for_counts(0, 0, 0, 0, &device.limits())?;
        let storage = GraphStorage::new(&device, capacity);
        let GraphStorage {
            positions,
            momentum,
            styles,
            edges,
            offsets,
            neighbours,
            ..
        } = &storage;
        let births = crate::births::BirthInitializer::new(
            &device,
            positions,
            capacity.nodes,
            capacity.edges,
        );
        let physics_parameters = uniform(
            &device,
            "force parameters",
            &PhysicsParameters {
                count: 0,
                padding: [0; 2],
                momentum_retention: MOMENTUM_RETENTION,
                repulsion: REPULSION,
                dt: BASE_TIMESTEP,
                softening_squared: SOFTENING_SQUARED,
                max_displacement: MAX_DISPLACEMENT,
            },
        );
        let compute = wgpu::ShaderStages::COMPUTE;
        let vertex = wgpu::ShaderStages::VERTEX;
        let physics_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("force layout"),
            entries: &[
                buffer_layout(0, compute, true, false),
                buffer_layout(1, compute, false, false),
                buffer_layout(2, compute, true, false),
                buffer_layout(3, compute, true, false),
                buffer_layout(4, compute, true, true),
                buffer_layout(5, compute, false, false),
            ],
        });
        let bounds_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bounds layout"),
            entries: &[
                buffer_layout(0, compute, true, false),
                buffer_layout(1, compute, true, false),
                buffer_layout(2, compute, false, false),
                buffer_layout(3, compute, true, true),
            ],
        });
        let render_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("graph rendering layout"),
            entries: &[
                buffer_layout(0, vertex, true, false),
                buffer_layout(1, vertex, true, false),
                buffer_layout(2, vertex, true, false),
                buffer_layout(3, vertex, true, false),
                buffer_layout(4, vertex, true, true),
            ],
        });
        let physics_groups = std::array::from_fn(|i| {
            bind_group(
                &device,
                "force bindings",
                &physics_layout,
                &[
                    &positions[i],
                    &positions[1 - i],
                    offsets,
                    neighbours,
                    &physics_parameters,
                    momentum,
                ],
            )
        });
        let output = FrameTarget::new(
            &device,
            positions,
            styles,
            edges,
            &bounds_layout,
            &render_layout,
        );
        let preview = FrameTarget::new(
            &device,
            positions,
            styles,
            edges,
            &bounds_layout,
            &render_layout,
        );

        let physics_shader = shader(
            &device,
            "force shader",
            include_str!("shaders/physics.wgsl"),
        );
        let bounds_shader = shader(
            &device,
            "bounds shader",
            include_str!("shaders/bounds.wgsl"),
        );
        let render_shader = shader(
            &device,
            "render shader",
            include_str!("shaders/render.wgsl"),
        );
        let physics_pipeline =
            compute_pipeline(&device, "force pipeline", &physics_layout, &physics_shader);
        let bounds_pipeline =
            compute_pipeline(&device, "bounds pipeline", &bounds_layout, &bounds_shader);
        let render_pipeline_layout =
            pipeline_layout(&device, "render pipeline layout", &render_layout);
        let node_pipeline = render_pipeline(
            &device,
            "node pipeline",
            &render_pipeline_layout,
            &render_shader,
            "node_vertex",
            "node_fragment",
        );
        let edge_pipeline = render_pipeline(
            &device,
            "edge pipeline",
            &render_pipeline_layout,
            &render_shader,
            "edge_vertex",
            "edge_fragment",
        );
        Ok(Self {
            device,
            queue,
            storage,
            births,
            physics_layout,
            bounds_layout,
            render_layout,
            physics_parameters,
            physics_pipeline,
            bounds_pipeline,
            node_pipeline,
            edge_pipeline,
            physics_groups,
            active: 0,
            node_count: 0,
            edge_count: 0,
            effective_dt: BASE_TIMESTEP,
            base_styles: Vec::new(),
            incident_degrees: Vec::new(),
            degree_sizing: false,
            edge_width: 1.0,
            output,
            preview,
        })
    }

    fn ensure_capacity(&mut self, nodes: usize, edges: usize) -> Result<(), String> {
        let old = self.storage.capacity;
        let capacity =
            Capacity::for_counts(nodes, edges, old.nodes, old.edges, &self.device.limits())?;
        if capacity == old {
            return Ok(());
        }
        let (storage, births, physics_groups, output_groups, preview_groups) =
            gpu_allocation_scope(&self.device, || {
                let storage = self.storage.grow(&self.device, capacity);
                let births = self.births.with_capacity(
                    &self.device,
                    &storage.positions,
                    capacity.nodes,
                    capacity.edges,
                );
                let physics_groups = std::array::from_fn(|i| {
                    bind_group(
                        &self.device,
                        "force bindings",
                        &self.physics_layout,
                        &[
                            &storage.positions[i],
                            &storage.positions[1 - i],
                            &storage.offsets,
                            &storage.neighbours,
                            &self.physics_parameters,
                            &storage.momentum,
                        ],
                    )
                });
                let output_groups = self.output.groups(
                    &self.device,
                    &storage,
                    &self.bounds_layout,
                    &self.render_layout,
                );
                let preview_groups = self.preview.groups(
                    &self.device,
                    &storage,
                    &self.bounds_layout,
                    &self.render_layout,
                );
                Ok((
                    storage,
                    births,
                    physics_groups,
                    output_groups,
                    preview_groups,
                ))
            })?;
        gpu_allocation_scope(&self.device, || {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("grow live graph storage"),
                });
            if capacity.nodes != old.nodes && self.node_count != 0 {
                let bytes = (self.node_count * 8) as u64;
                for i in 0..2 {
                    encoder.copy_buffer_to_buffer(
                        &self.storage.positions[i],
                        0,
                        &storage.positions[i],
                        0,
                        bytes,
                    );
                }
                encoder.copy_buffer_to_buffer(
                    &self.storage.momentum,
                    0,
                    &storage.momentum,
                    0,
                    bytes,
                );
                encoder.copy_buffer_to_buffer(
                    &self.storage.styles,
                    0,
                    &storage.styles,
                    0,
                    (self.node_count * 32) as u64,
                );
                encoder.copy_buffer_to_buffer(
                    &self.storage.offsets,
                    0,
                    &storage.offsets,
                    0,
                    ((self.node_count + 1) * 4) as u64,
                );
            }
            if capacity.edges != old.edges && self.edge_count != 0 {
                encoder.copy_buffer_to_buffer(
                    &self.storage.edges,
                    0,
                    &storage.edges,
                    0,
                    (self.edge_count * 32) as u64,
                );
                encoder.copy_buffer_to_buffer(
                    &self.storage.neighbours,
                    0,
                    &storage.neighbours,
                    0,
                    (self.edge_count * 32) as u64,
                );
            }
            self.queue.submit(Some(encoder.finish()));
            Ok(())
        })?;
        self.storage = storage;
        self.births = births;
        self.physics_groups = physics_groups;
        (self.output.bounds_groups, self.output.render_groups) = output_groups;
        (self.preview.bounds_groups, self.preview.render_groups) = preview_groups;
        Ok(())
    }

    /// Check actual device and shader addressability before playback/recording.
    /// Memory remains allocated on demand as nodes and edges are born.
    pub fn validate_capacity(&self, nodes: usize, edges: usize) -> Result<(), String> {
        Capacity::for_counts(nodes, edges, 0, 0, &self.device.limits()).map(|_| ())
    }

    /// Synchronise structure/styles without replacing live positions or momentum.
    pub fn sync_graph(&mut self, graph: &Graph, reset: bool) -> Result<(), String> {
        self.validate_capacity(graph.nodes.len(), graph.edges.len())?;
        if graph
            .edges
            .iter()
            .any(|edge| edge.source >= graph.nodes.len() || edge.target >= graph.nodes.len())
        {
            return Err("An edge refers to a node outside the live graph".into());
        }
        let upload = self.prepare_upload(graph, reset)?;
        self.ensure_capacity(graph.nodes.len(), graph.edges.len())?;
        let device = self.device.clone();
        gpu_allocation_scope(&device, || {
            self.upload_graph(&upload);
            Ok(())
        })?;
        if upload.reset {
            self.active = 0;
        }
        self.node_count = graph.nodes.len();
        self.edge_count = graph.edges.len();
        self.base_styles = upload.base_styles;
        self.incident_degrees = upload.incident_degrees;
        self.effective_dt = upload.effective_dt;
        Ok(())
    }

    fn prepare_upload(&self, graph: &Graph, reset: bool) -> Result<GraphUpload, String> {
        let reset = reset || graph.nodes.len() < self.node_count;
        let first = if reset { 0 } else { self.node_count };
        // Reserve upload scratch explicitly so host memory errors are useful
        // errors too, rather than an allocation abort during graph growth.
        let births = collected(
            graph.nodes[first..].iter().map(|node| node.position),
            "new node positions",
        )?;
        let rest = filled(births.len(), [0.0_f32; 2], "new node momentum")?;
        let base_styles = collected(
            graph.nodes.iter().map(|node| NodeStyle {
                color: linear_color(node.color),
                radius: node.radius,
                padding: [0.0; 3],
            }),
            "node styles",
        )?;
        let edges = collected(
            graph.edges.iter().map(|edge| GpuEdge {
                source: edge.source as u32,
                target: edge.target as u32,
                strength: edge.strength,
                gradient: u32::from(edge.gradient),
                color: linear_color(edge.color),
            }),
            "edges",
        )?;
        let mut offsets = filled(graph.nodes.len() + 1, 0u32, "adjacency offsets")?;
        for edge in &graph.edges {
            offsets[edge.source + 1] += 1;
            offsets[edge.target + 1] += 1;
        }
        for i in 1..offsets.len() {
            offsets[i] += offsets[i - 1];
        }
        let incident_degrees = display_degrees(graph)?;
        let mut cursor = collected(offsets.iter().copied(), "adjacency cursors")?;
        let mut neighbours = filled(
            graph.edges.len() * 2,
            Neighbour::zeroed(),
            "adjacency entries",
        )?;
        for edge in &graph.edges {
            for (owner, other) in [(edge.source, edge.target), (edge.target, edge.source)] {
                neighbours[cursor[owner] as usize] = Neighbour {
                    other: other as u32,
                    padding0: 0,
                    strength: edge.strength,
                    padding1: 0,
                };
                cursor[owner] += 1;
            }
        }
        let effective_dt = force_timestep(graph)?;
        let birth_plan = crate::births::prepare_births(graph, first)?;
        Ok(GraphUpload {
            reset,
            first,
            births,
            rest,
            base_styles,
            edges,
            offsets,
            neighbours,
            incident_degrees,
            effective_dt,
            birth_plan,
        })
    }

    fn upload_graph(&self, upload: &GraphUpload) {
        let GraphUpload {
            reset,
            first,
            births,
            rest,
            base_styles,
            edges,
            offsets,
            neighbours,
            incident_degrees,
            effective_dt,
            birth_plan,
        } = upload;
        if !births.is_empty() {
            for buffer in &self.storage.positions {
                self.queue
                    .write_buffer(buffer, (first * 8) as u64, bytemuck::cast_slice(births));
            }
            self.queue.write_buffer(
                &self.storage.momentum,
                (first * 8) as u64,
                bytemuck::cast_slice(rest),
            );
        }
        if *reset {
            self.reset_camera();
        }
        self.births.initialise(
            &self.device,
            &self.queue,
            birth_plan,
            if *reset { 0 } else { self.active },
        );
        self.write_styles(base_styles, incident_degrees);
        if !edges.is_empty() {
            self.queue
                .write_buffer(&self.storage.edges, 0, bytemuck::cast_slice(edges));
        }
        self.queue
            .write_buffer(&self.storage.offsets, 0, bytemuck::cast_slice(offsets));
        if !neighbours.is_empty() {
            self.queue.write_buffer(
                &self.storage.neighbours,
                0,
                bytemuck::cast_slice(neighbours),
            );
        }
        self.queue.write_buffer(
            &self.physics_parameters,
            0,
            bytemuck::bytes_of(&PhysicsParameters {
                count: base_styles.len() as u32,
                padding: [0; 2],
                momentum_retention: MOMENTUM_RETENTION,
                repulsion: REPULSION,
                dt: *effective_dt,
                softening_squared: SOFTENING_SQUARED,
                max_displacement: MAX_DISPLACEMENT,
            }),
        );
    }

    /// The same stiffness-limited scalar applies to every node's full force.
    /// Timeline ticks remain discrete events, independent of wall-clock time.
    pub fn effective_timestep(&self) -> f32 {
        self.effective_dt
    }

    /// A view option only. Keep rule radii and physics intact, update the GPU
    /// styles immediately, and let the next render fit the displayed radii.
    pub fn set_degree_sizing(&mut self, enabled: bool) {
        if self.degree_sizing != enabled {
            self.degree_sizing = enabled;
            self.write_display_styles();
            // A deliberate appearance change should refit immediately even
            // while idle; normal simulation frames retain smooth contraction.
            self.reset_camera();
        }
    }

    /// Multiply the base 0.18-world-unit edge width in both preview and export,
    /// with a one-physical-pixel minimum before applying this multiplier.
    /// This appearance setting never changes node sizes or simulation forces.
    pub fn set_edge_width(&mut self, multiplier: f32) {
        self.edge_width = if multiplier.is_finite() {
            multiplier.clamp(0.25, 8.0)
        } else {
            1.0
        };
    }

    fn write_display_styles(&self) {
        self.write_styles(&self.base_styles, &self.incident_degrees);
    }

    fn write_styles(&self, base_styles: &[NodeStyle], incident_degrees: &[u32]) {
        if base_styles.is_empty() {
            return;
        }
        if !self.degree_sizing {
            self.queue
                .write_buffer(&self.storage.styles, 0, bytemuck::cast_slice(base_styles));
            return;
        }
        // Fixed scratch avoids a second graph-sized host allocation when the
        // visual sizing toggle is used on a large graph.
        let mut adjusted = [NodeStyle::zeroed(); 256];
        for (chunk_index, styles) in base_styles.chunks(adjusted.len()).enumerate() {
            let first = chunk_index * adjusted.len();
            for (i, style) in styles.iter().enumerate() {
                adjusted[i] = NodeStyle {
                    radius: display_radius(style.radius, incident_degrees[first + i]),
                    ..*style
                };
            }
            self.queue.write_buffer(
                &self.storage.styles,
                (first * 32) as u64,
                bytemuck::cast_slice(&adjusted[..styles.len()]),
            );
        }
    }

    /// Advance fixed simulation ticks. Callers choose a fixed tick/frame ratio
    /// during recording and apply timeline events at exact tick boundaries.
    pub fn step(&mut self, ticks: u32) {
        if ticks == 0 || self.node_count == 0 {
            return;
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("simulation ticks"),
            });
        for _ in 0..ticks {
            // Separate passes give explicit storage-write -> storage-read hazards
            // between ping-pong updates on every supported backend.
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fixed force tick"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.physics_pipeline);
                pass.set_bind_group(0, &self.physics_groups[self.active], &[]);
                let (x, y) = dispatch_grid(
                    self.node_count as u32,
                    self.device.limits().max_compute_workgroups_per_dimension,
                );
                pass.dispatch_workgroups(x, y, 1);
            }
            self.active = 1 - self.active;
        }
        self.queue.submit(Some(encoder.finish()));
    }

    /// Render one export camera sample at the selected recording resolution.
    pub fn render(&mut self, width: u32, height: u32) {
        self.render_target(width, height, false);
    }

    /// Render at the canvas's physical pixel dimensions. Its independent camera
    /// and image never change export resolution or export camera smoothing.
    pub fn render_preview(&mut self, width: u32, height: u32) {
        self.render_target(width, height, true);
    }

    fn render_target(&mut self, width: u32, height: u32, preview: bool) {
        let dimensions =
            fitted_dimensions(width, height, self.device.limits().max_texture_dimension_2d);
        let target = if preview {
            &mut self.preview
        } else {
            &mut self.output
        };
        if dimensions != target.dimensions {
            target.texture = output_texture(&self.device, dimensions);
            target.view = target
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            target.dimensions = dimensions;
        }
        let parameters = FrameParameters {
            width: dimensions.0 as f32,
            height: dimensions.1 as f32,
            count: self.node_count as u32,
            edges: self.edge_count as u32,
            edge_width: self.edge_width,
            padding: [0.0; 3],
        };
        self.queue
            .write_buffer(&target.frame_parameters, 0, bytemuck::bytes_of(&parameters));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("graph frame"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("strict auto-fit camera"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.bounds_pipeline);
            pass.set_bind_group(0, &target.bounds_groups[self.active], &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("2D graph render"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &target.render_groups[self.active], &[]);
            pass.set_pipeline(&self.edge_pipeline);
            pass.draw(0..6, 0..self.edge_count as u32);
            pass.set_pipeline(&self.node_pipeline);
            pass.draw(0..6, 0..self.node_count as u32);
        }
        self.queue.submit(Some(encoder.finish()));
    }

    pub fn reset_camera(&self) {
        for target in [&self.output, &self.preview] {
            self.queue
                .write_buffer(&target.camera, 0, bytemuck::cast_slice(&[0.0_f32; 4]));
        }
    }

    pub fn preview_view(&self) -> &wgpu::TextureView {
        &self.preview.view
    }
    pub fn preview_dimensions(&self) -> (u32, u32) {
        self.preview.dimensions
    }
    #[cfg(test)]
    pub fn dimensions(&self) -> (u32, u32) {
        self.output.dimensions
    }

    /// Explicit synchronous readback for lossless frame delivery to the video
    /// encoder. The caller must apply backpressure instead of dropping frames.
    pub fn read_rgba(&self) -> Result<Vec<u8>, String> {
        self.read_target_rgba(&self.output)
    }

    /// Diagnostics only; normal interactive preview never transfers pixels.
    pub fn read_preview_rgba(&self) -> Result<Vec<u8>, String> {
        self.read_target_rgba(&self.preview)
    }

    fn read_target_rgba(&self, target: &FrameTarget) -> Result<Vec<u8>, String> {
        let (width, height) = target.dimensions;
        let unpadded = width * 4;
        let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let size = u64::from(padded) * u64::from(height);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video frame readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("video frame transfer"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        let bytes = self.map_readback(&buffer)?;
        let mut rgba = Vec::with_capacity((unpadded * height) as usize);
        for row in bytes.chunks_exact(padded as usize) {
            rgba.extend_from_slice(&row[..unpadded as usize]);
        }
        Ok(rgba)
    }

    /// Inspector/testing readback only. Normal animation never reads positions
    /// to the CPU; recording reads pixels but uses the same GPU solver buffers.
    pub fn read_positions(&self, count: usize) -> Result<Vec<[f32; 2]>, String> {
        if count > self.node_count {
            return Err("Requested more positions than the live graph contains".into());
        }
        if count == 0 {
            return Ok(Vec::new());
        }
        let size = (count * 8) as u64;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("explicit position readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("position readback"),
            });
        encoder.copy_buffer_to_buffer(&self.storage.positions[self.active], 0, &buffer, 0, size);
        self.queue.submit(Some(encoder.finish()));
        let bytes = self.map_readback(&buffer)?;
        // Vec<u8> does not promise f32 alignment, so decode rather than cast it.
        Ok(bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|bytes| {
                [
                    f32::from_le_bytes(bytes[0..4].try_into().expect("four-byte x")),
                    f32::from_le_bytes(bytes[4..8].try_into().expect("four-byte y")),
                ]
            })
            .collect())
    }

    fn map_readback(&self, buffer: &wgpu::Buffer) -> Result<Vec<u8>, String> {
        let (sender, receiver) = mpsc::sync_channel(1);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        let _ = self.device.poll(wgpu::Maintain::Wait);
        receiver
            .recv()
            .map_err(|error| format!("GPU mapping callback was lost: {error}"))?
            .map_err(|error| format!("GPU readback failed: {error}"))?;
        let bytes = buffer.slice(..).get_mapped_range().to_vec();
        buffer.unmap();
        Ok(bytes)
    }
}

impl FrameTarget {
    fn groups(
        &self,
        device: &wgpu::Device,
        storage: &GraphStorage,
        bounds_layout: &wgpu::BindGroupLayout,
        render_layout: &wgpu::BindGroupLayout,
    ) -> ([wgpu::BindGroup; 2], [wgpu::BindGroup; 2]) {
        let bounds = std::array::from_fn(|i| {
            bind_group(
                device,
                "bounds bindings",
                bounds_layout,
                &[
                    &storage.positions[i],
                    &storage.styles,
                    &self.camera,
                    &self.frame_parameters,
                ],
            )
        });
        let render = std::array::from_fn(|i| {
            bind_group(
                device,
                "render bindings",
                render_layout,
                &[
                    &storage.positions[i],
                    &storage.styles,
                    &storage.edges,
                    &self.camera,
                    &self.frame_parameters,
                ],
            )
        });
        (bounds, render)
    }
    fn new(
        device: &wgpu::Device,
        positions: &[wgpu::Buffer; 2],
        styles: &wgpu::Buffer,
        edges: &wgpu::Buffer,
        bounds_layout: &wgpu::BindGroupLayout,
        render_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let camera = storage(device, "auto-fit camera", 16, true);
        let frame_parameters = uniform(
            device,
            "frame parameters",
            &FrameParameters {
                width: 1280.0,
                height: 720.0,
                count: 0,
                edges: 0,
                edge_width: 1.0,
                padding: [0.0; 3],
            },
        );
        let bounds_groups = std::array::from_fn(|i| {
            bind_group(
                device,
                "bounds bindings",
                bounds_layout,
                &[&positions[i], styles, &camera, &frame_parameters],
            )
        });
        let render_groups = std::array::from_fn(|i| {
            bind_group(
                device,
                "render bindings",
                render_layout,
                &[&positions[i], styles, edges, &camera, &frame_parameters],
            )
        });
        let dimensions = (1280, 720);
        let texture = output_texture(device, dimensions);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            frame_parameters,
            camera,
            bounds_groups,
            render_groups,
            texture,
            view,
            dimensions,
        }
    }
}

fn fitted_dimensions(width: u32, height: u32, maximum: u32) -> (u32, u32) {
    let (width, height, maximum) = (width.max(1), height.max(1), maximum.max(1));
    let largest = width.max(height);
    if largest <= maximum {
        return (width, height);
    }
    let scale = f64::from(maximum) / f64::from(largest);
    (
        (f64::from(width) * scale).floor().max(1.0) as u32,
        (f64::from(height) * scale).floor().max(1.0) as u32,
    )
}

fn storage(device: &wgpu::Device, label: &str, bytes: usize, copy_source: bool) -> wgpu::Buffer {
    let mut usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
    if copy_source {
        usage |= wgpu::BufferUsages::COPY_SRC;
    }
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes as u64,
        usage,
        mapped_at_creation: false,
    })
}

fn uniform<T: Pod>(device: &wgpu::Device, label: &str, value: &T) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::bytes_of(value),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

fn buffer_layout(
    binding: u32,
    visibility: wgpu::ShaderStages,
    read_only: bool,
    uniform: bool,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: if uniform {
                wgpu::BufferBindingType::Uniform
            } else {
                wgpu::BufferBindingType::Storage { read_only }
            },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn bind_group(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    buffers: &[&wgpu::Buffer],
) -> wgpu::BindGroup {
    let entries: Vec<_> = buffers
        .iter()
        .enumerate()
        .map(|(index, buffer)| wgpu::BindGroupEntry {
            binding: index as u32,
            resource: buffer.as_entire_binding(),
        })
        .collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &entries,
    })
}

fn shader(device: &wgpu::Device, label: &str, source: &'static str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

fn pipeline_layout(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::PipelineLayout {
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    })
}

fn compute_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    shader: &wgpu::ShaderModule,
) -> wgpu::ComputePipeline {
    let layout = pipeline_layout(device, label, layout);
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        module: shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    })
}

fn render_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    vertex: &str,
    fragment: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex),
            buffers: &[],
            compilation_options: Default::default(),
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: FORMAT,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    })
}

fn output_texture(device: &wgpu::Device, dimensions: (u32, u32)) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Nodiform offscreen frame"),
        size: wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn collected<T>(values: impl ExactSizeIterator<Item = T>, label: &str) -> Result<Vec<T>, String> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(values.len())
        .map_err(|error| format!("Not enough memory for {label}: {error}"))?;
    result.extend(values);
    Ok(result)
}

fn filled<T: Clone>(count: usize, value: T, label: &str) -> Result<Vec<T>, String> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|error| format!("Not enough memory for {label}: {error}"))?;
    result.resize(count, value);
    Ok(result)
}

fn linear_color(color: [f32; 4]) -> [f32; 4] {
    fn linear(value: f32) -> f32 {
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    [
        linear(color[0]),
        linear(color[1]),
        linear(color[2]),
        color[3],
    ]
}

fn display_radius(base_radius: f32, degree: u32) -> f32 {
    // Obsidian 1.13.7 global-graph square-root curve, normalised to its minimum
    // radius of 8. Omit its upper limit of 30: Nodiform's view has no size cap.
    // World-space radii still follow the rule's scale and the normal camera.
    // Source release: https://github.com/obsidianmd/obsidian-releases/releases/tag/v1.13.7
    base_radius * (3.0 * (degree as f32 + 1.0).sqrt() / 8.0).max(1.0)
}

fn display_degrees(graph: &Graph) -> Result<Vec<u32>, String> {
    // As in Obsidian's global graph, repeated links in the same direction
    // count once, while reciprocal links each count. This display convention
    // does not alter weighted attraction or the full physical adjacency.
    let mut connections = std::collections::HashSet::new();
    connections
        .try_reserve(graph.edges.len())
        .map_err(|error| format!("Not enough memory to count node connections: {error}"))?;
    let mut degrees = filled(graph.nodes.len(), 0u32, "node connection counts")?;
    for edge in &graph.edges {
        if connections.insert((edge.source, edge.target)) {
            degrees[edge.source] += 1;
            degrees[edge.target] += 1;
        }
    }
    Ok(degrees)
}

fn force_timestep(graph: &Graph) -> Result<f32, String> {
    // f64 accumulation keeps large weighted degrees finite and avoids losing
    // small incident strengths while summing edges in their declared order.
    let mut incident = filled(graph.nodes.len(), 0.0_f64, "weighted node connections")?;
    for edge in &graph.edges {
        incident[edge.source] += f64::from(edge.strength);
        incident[edge.target] += f64::from(edge.strength);
    }
    let maximum = incident.into_iter().fold(0.0_f64, f64::max);
    Ok(if maximum == 0.0 {
        BASE_TIMESTEP
    } else {
        f64::from(BASE_TIMESTEP).min(0.5 / maximum) as f32
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn capacities_grow_on_demand_and_clamp_headroom_to_device_limits() {
        use super::Capacity;
        let limits = eframe::wgpu::Limits::default();
        assert_eq!(
            Capacity::for_counts(0, 0, 0, 0, &limits).unwrap(),
            Capacity {
                nodes: 64,
                edges: 64
            }
        );
        assert_eq!(
            Capacity::for_counts(65, 63, 64, 64, &limits).unwrap(),
            Capacity {
                nodes: 128,
                edges: 64
            }
        );
        assert_eq!(
            Capacity::for_counts(9_001, 250_001, 128, 64, &limits).unwrap(),
            Capacity {
                nodes: 9_001,
                edges: 250_001
            }
        );
        let limits = eframe::wgpu::Limits {
            max_storage_buffer_binding_size: 3200,
            ..limits
        };
        assert_eq!(
            Capacity::for_counts(65, 100, 64, 64, &limits).unwrap(),
            Capacity {
                nodes: 100,
                edges: 100
            }
        );
        assert!(Capacity::for_counts(101, 0, 0, 0, &limits)
            .unwrap_err()
            .contains("per-buffer capacity"));
        assert!(Capacity::for_counts(0, 101, 0, 0, &limits)
            .unwrap_err()
            .contains("per-buffer capacity"));
        assert!(Capacity::for_counts(usize::MAX, usize::MAX, 0, 0, &limits).is_err());
        let buffer_limited = eframe::wgpu::Limits {
            max_buffer_size: 2048,
            ..limits
        };
        assert!(Capacity::for_counts(65, 0, 0, 0, &buffer_limited).is_err());
    }

    #[test]
    fn compute_grid_spans_rows_and_rejects_unaddressable_dispatches() {
        use super::{dispatch_grid, Capacity};
        assert_eq!(dispatch_grid(1, 65_535), (1, 1));
        assert_eq!(dispatch_grid(129, 65_535), (2, 1));
        assert_eq!(dispatch_grid(513, 2), (2, 3));
        assert_eq!(dispatch_grid(8_388_481, 65_535), (65_535, 2));
        let limits = eframe::wgpu::Limits {
            max_compute_workgroups_per_dimension: 2,
            ..Default::default()
        };
        assert!(Capacity::for_counts(512, 0, 0, 0, &limits).is_ok());
        assert!(Capacity::for_counts(513, 0, 0, 0, &limits)
            .unwrap_err()
            .contains("compute-dispatch"));
    }

    #[test]
    fn degree_sizing_matches_uncapped_obsidian_curve_and_preserves_rule_scale() {
        use super::display_radius;
        for degree in 0..=6 {
            assert_eq!(display_radius(2.0, degree), 2.0);
        }
        assert_eq!(display_radius(2.0, 7), 2.0 * (3.0 * 8.0_f32.sqrt() / 8.0));
        assert_eq!(display_radius(2.0, 15), 3.0);
        assert_eq!(display_radius(2.0, 63), 6.0);
        assert_eq!(display_radius(2.0, 99), 7.5);
        // Unlike Obsidian's upper limit of 30 / 8, growth continues past this.
        assert!(display_radius(2.0, 100) > 7.5);
        assert_eq!(display_radius(2.0, 399), 15.0);
        assert!((display_radius(1.0, 499) - 8.385_255).abs() < 0.000_01);
        let degrees = [6, 7, 31, 32, 99, 100, 499, 500, 250_000, 500_000];
        for pair in degrees.windows(2) {
            assert!(display_radius(2.0, pair[1]) > display_radius(2.0, pair[0]));
        }
        for degree in degrees {
            assert_eq!(
                display_radius(4.0, degree),
                2.0 * display_radius(2.0, degree)
            );
        }
    }

    #[test]
    fn display_degree_counts_unique_directed_connections_without_changing_edges() {
        use crate::model::{Event, Graph};
        let event: Event = serde_json::from_value(serde_json::json!({
            "op":"batch", "nodes":[{"id":"a"},{"id":"b"},{"id":"isolated"}],
            "edges":[
                {"id":"ab","source":"a","target":"b"},
                {"id":"duplicate-ab","source":"a","target":"b"},
                {"id":"ba","source":"b","target":"a","strength":0},
                {"id":"bb","source":"b","target":"b"},
                {"id":"duplicate-bb","source":"b","target":"b"}
            ]
        }))
        .unwrap();
        let mut graph = Graph::new(0);
        graph.apply(&event).unwrap();
        assert_eq!(super::display_degrees(&graph).unwrap(), vec![2, 4, 0]);
        assert_eq!(graph.edges.len(), 5);
        assert_eq!(
            graph.edges.iter().map(|edge| edge.strength).sum::<f32>(),
            4.0 * crate::model::DEFAULT_EDGE_STRENGTH
        );
        graph
            .apply(&Event::Batch {
                nodes: vec![],
                edges: vec![crate::model::EdgeSpec {
                    id: "to-isolated".into(),
                    source: "a".into(),
                    target: "isolated".into(),
                    color: "#ffffff".into(),
                    strength: 0.0,
                    gradient: false,
                }],
            })
            .unwrap();
        assert_eq!(super::display_degrees(&graph).unwrap(), vec![3, 4, 1]);
    }

    #[test]
    fn timestep_bounds_incident_stiffness_and_zero_degree() {
        use crate::model::{Edge, Graph, Node};
        let mut graph = Graph::new(1);
        assert_eq!(super::force_timestep(&graph).unwrap(), 1.0 / 120.0);
        graph.nodes = (0..3)
            .map(|index| Node {
                id: index.to_string(),
                label: index.to_string(),
                color: [1.0; 4],
                radius: 1.0,
                position: [index as f32, 0.0],
                auto_position: false,
            })
            .collect();
        assert_eq!(super::force_timestep(&graph).unwrap(), 1.0 / 120.0);
        graph.edges = vec![
            Edge {
                id: "01".into(),
                source: 0,
                target: 1,
                color: [1.0; 4],
                strength: 1_000_000.0,
                gradient: false,
            },
            Edge {
                id: "02".into(),
                source: 0,
                target: 2,
                color: [1.0; 4],
                strength: 1_000_000.0,
                gradient: false,
            },
        ];
        assert_eq!(super::force_timestep(&graph).unwrap(), 0.25e-6);
        graph.edges[1].strength = 0.0;
        let dt = super::force_timestep(&graph).unwrap();
        assert_eq!(dt, 0.5e-6);
        // The spring Laplacian has largest eigenvalue at most 2 × weighted
        // degree. The selected force step keeps its product no greater than 1,
        // inside the damped second-order linear stability interval.
        assert!(dt * 2.0 * 1_000_000.0 <= 1.0);
        assert!(dt * 2.0 * 1_000_000.0 < 2.0 * (1.0 + super::MOMENTUM_RETENTION));
        graph.edges[0].strength = 0.0;
        assert_eq!(super::force_timestep(&graph).unwrap(), 1.0 / 120.0);
    }

    #[test]
    fn native_preview_dimensions_preserve_aspect_with_device_limits() {
        use super::fitted_dimensions;
        assert_eq!(fitted_dimensions(3840, 2160, 8192), (3840, 2160));
        assert_eq!(fitted_dimensions(16384, 8192, 8192), (8192, 4096));
        assert_eq!(fitted_dimensions(5000, 10000, 8192), (4096, 8192));
        assert_eq!(fitted_dimensions(0, 0, 8192), (1, 1));
        assert_eq!(fitted_dimensions(u32::MAX, 1, 8192), (8192, 1));
    }

    #[test]
    fn wgsl_shaders_parse_and_validate() {
        for (name, source) in [
            ("births", include_str!("shaders/births.wgsl")),
            ("physics", include_str!("shaders/physics.wgsl")),
            ("bounds", include_str!("shaders/bounds.wgsl")),
            ("render", include_str!("shaders/render.wgsl")),
        ] {
            let module = naga::front::wgsl::parse_str(source)
                .unwrap_or_else(|error| panic!("{name}: {}", error.emit_to_string(source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        }
    }

    #[test]
    fn gpu_struct_sizes_match_wgsl() {
        assert_eq!(std::mem::size_of::<super::NodeStyle>(), 32);
        assert_eq!(std::mem::size_of::<super::GpuEdge>(), 32);
        assert_eq!(std::mem::size_of::<super::Neighbour>(), 16);
        assert_eq!(std::mem::size_of::<super::PhysicsParameters>(), 32);
        assert_eq!(std::mem::size_of::<super::FrameParameters>(), 32);
    }

    /// Opt-in actual backend validation, also usable with Mesa lavapipe in CI:
    /// cargo test gpu::tests::headless_gpu_smoke -- --ignored --nocapture
    #[test]
    #[ignore = "requires a Vulkan device or software Vulkan driver"]
    fn headless_gpu_smoke() {
        use super::*;
        use crate::model::{EdgeSpec, Event, NodeSpec};

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("No Vulkan adapter is available for the opt-in GPU test");
        eprintln!("GPU validation adapter: {:?}", adapter.get_info());
        let supported = adapter.limits();
        let required_limits = wgpu::Limits {
            max_buffer_size: supported.max_buffer_size,
            max_storage_buffer_binding_size: supported.max_storage_buffer_binding_size,
            max_compute_workgroups_per_dimension: supported.max_compute_workgroups_per_dimension,
            max_texture_dimension_2d: supported.max_texture_dimension_2d,
            ..Default::default()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("Nodiform validation device"),
                required_features: wgpu::Features::empty(),
                required_limits,
                memory_hints: Default::default(),
            },
            None,
        ))
        .expect("Vulkan device creation failed");
        let mut gpu = GpuGraph::new(Arc::new(device), Arc::new(queue)).unwrap();
        validate_dynamic_storage(&gpu);
        // Both targets really clear to black, including sRGB output conversion.
        gpu.render(31, 17);
        gpu.render_preview(29, 19);
        assert!(gpu
            .read_rgba()
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 0, 0, 255]));
        assert!(gpu
            .read_preview_rgba()
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 0, 0, 255]));
        let mut graph = Graph::new(42);
        let node = |id: &str, position: [f32; 2]| NodeSpec {
            id: id.into(),
            label: None,
            color: "#89b4fa".into(),
            radius: 1.0,
            position: Some(position),
        };
        graph
            .apply(&Event::Batch {
                nodes: vec![node("a", [-3.0, 0.0]), node("b", [3.0, 0.0])],
                edges: vec![EdgeSpec {
                    id: "ab".into(),
                    source: "a".into(),
                    target: "b".into(),
                    color: "#60708b".into(),
                    strength: 1.0,
                    gradient: false,
                }],
            })
            .unwrap();
        gpu.sync_graph(&graph, true).unwrap();
        gpu.step(1);
        let positions = gpu.read_positions(2).unwrap();
        let expected =
            -3.0 + ((REPULSION * -6.0 / (36.0 + SOFTENING_SQUARED)) + 6.0) * BASE_TIMESTEP;
        assert!((positions[0][0] - expected).abs() < 0.00001);
        assert!((positions[0][0] + positions[1][0]).abs() < 0.00001);

        // A style/strength update must never put simulated nodes back at birth.
        graph
            .apply(&Event::SetNode {
                id: "a".into(),
                color: Some("#ff0000".into()),
                radius: Some(2.0),
            })
            .unwrap();
        graph
            .apply(&Event::SetEdge {
                id: "ab".into(),
                color: None,
                strength: Some(2.0),
                gradient: None,
            })
            .unwrap();
        gpu.sync_graph(&graph, false).unwrap();
        assert_eq!(positions, gpu.read_positions(2).unwrap());

        // Append after an odd step exercises the opposite ping-pong buffer.
        graph
            .apply(&Event::Batch {
                nodes: vec![node("c", [0.0, 9.0])],
                edges: vec![],
            })
            .unwrap();
        gpu.sync_graph(&graph, false).unwrap();
        assert_eq!(positions, gpu.read_positions(2).unwrap());
        assert_eq!(gpu.read_positions(3).unwrap()[2], [0.0, 9.0]);
        gpu.step(7);
        assert!(gpu
            .read_positions(3)
            .unwrap()
            .iter()
            .flatten()
            .all(|v| v.is_finite()));

        // Non-aligned image widths exercise padded GPU readback rows.
        gpu.render(137, 91);
        let frame = gpu.read_rgba().unwrap();
        assert_eq!(frame.len(), 137 * 91 * 4);
        assert!(frame.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255));
        assert!(frame
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] > 100 || pixel[2] > 100));

        let camera_before = read_camera(&gpu);
        // A monitor-sized preview has independent camera history and cannot
        // resize or perturb the selected export output, even when sampled often.
        for dimensions in [(3840, 2160), (257, 911), (800, 600)] {
            gpu.render_preview(dimensions.0, dimensions.1);
            assert_eq!(gpu.preview_dimensions(), dimensions);
            assert_eq!(gpu.dimensions(), (137, 91));
            assert_eq!(read_camera(&gpu), camera_before);
        }
        assert_eq!(gpu.read_preview_rgba().unwrap().len(), 800 * 600 * 4);
        assert_eq!(gpu.read_rgba().unwrap(), frame);
        graph
            .apply(&Event::Batch {
                nodes: vec![node("distant", [1000.0, -900.0])],
                edges: vec![],
            })
            .unwrap();
        gpu.sync_graph(&graph, false).unwrap();
        gpu.render(137, 91);
        let camera_after = read_camera(&gpu);
        assert!(camera_after[3] > camera_before[3]);
        for (position, node) in gpu.read_positions(4).unwrap().iter().zip(&graph.nodes) {
            assert!((position[0] - camera_after[0]).abs() + node.radius <= camera_after[2]);
            assert!((position[1] - camera_after[1]).abs() + node.radius <= camera_after[3]);
        }

        gpu.sync_graph(&graph, true).unwrap();
        assert_eq!(
            gpu.read_positions(3).unwrap(),
            vec![[-3.0, 0.0], [3.0, 0.0], [0.0, 9.0]]
        );

        // A default two-node spring settles at the analytically known force
        // balance. This detects an incorrect default or unstable stronger force.
        let mut balanced = Graph::new(42);
        balanced
            .apply(
                &serde_json::from_value(serde_json::json!({
                    "op":"batch", "nodes":[
                        {"id":"left","position":[-3,0]}, {"id":"right","position":[3,0]}
                    ], "edges":[{"id":"pair","source":"left","target":"right"}]
                }))
                .unwrap(),
            )
            .unwrap();
        gpu.sync_graph(&balanced, true).unwrap();
        gpu.step(240);
        let pair = gpu.read_positions(2).unwrap();
        let expected_distance =
            (REPULSION / crate::model::DEFAULT_EDGE_STRENGTH - SOFTENING_SQUARED).sqrt();
        assert!((pair[1][0] - pair[0][0] - expected_distance).abs() < 0.001);
        assert!((pair[0][0] + pair[1][0]).abs() < 0.00001);
        assert!(expected_distance > 1.3 * (64.0_f32 - SOFTENING_SQUARED).sqrt());

        // Gradient pixels must follow the live endpoint colours, interpolate
        // in linear light, honour opacity and switch back to a solid colour.
        let mut gradient = Graph::new(42);
        gradient.apply(&serde_json::from_value(serde_json::json!({
            "op":"batch", "nodes":[
                {"id":"red","position":[-10,0],"color":"#ff0000","radius":0.25},
                {"id":"blue","position":[10,0],"color":"#0000ff","radius":0.25}
            ], "edges":[{"id":"gradient","source":"red","target":"blue","gradient":true,"color":"#ffffff"}]
        })).unwrap()).unwrap();
        gpu.sync_graph(&gradient, true).unwrap();
        gpu.render(1024, 128);
        let gradient_frame = gpu.read_rgba().unwrap();
        let gradient_camera = read_camera(&gpu);
        let sample = |frame: &[u8], world_x: f32| -> [u8; 3] {
            let x = (((world_x - gradient_camera[0]) / gradient_camera[2] + 1.0) * 512.0).floor()
                as usize;
            frame[(64 * 1024 + x) * 4..(64 * 1024 + x) * 4 + 3]
                .try_into()
                .unwrap()
        };
        let left = sample(&gradient_frame, -5.0);
        let middle = sample(&gradient_frame, 0.0);
        let right = sample(&gradient_frame, 5.0);
        assert!(
            left[0] > left[2] && right[2] > right[0],
            "Endpoints did not form a gradient: {left:?}, {right:?}"
        );
        assert!(
            (175..=200).contains(&middle[0]) && (175..=200).contains(&middle[2]) && middle[1] < 30,
            "Expected linear-light red/blue midpoint near #bc00bc, got {middle:?}"
        );
        validate_edge_width(&mut gpu);
        gradient
            .apply(&Event::SetNode {
                id: "blue".into(),
                color: Some("#00ff00".into()),
                radius: None,
            })
            .unwrap();
        gpu.sync_graph(&gradient, false).unwrap();
        gpu.render(1024, 128);
        let updated = gpu.read_rgba().unwrap();
        let right = sample(&updated, 5.0);
        assert!(
            right[1] > right[0] && right[2] < 30,
            "Gradient missed live node recolouring: {right:?}"
        );
        gradient
            .apply(&Event::SetEdge {
                id: "gradient".into(),
                color: Some("#ffffff80".into()),
                strength: None,
                gradient: None,
            })
            .unwrap();
        gpu.sync_graph(&gradient, false).unwrap();
        gpu.render(1024, 128);
        let translucent = gpu.read_rgba().unwrap();
        let opaque_middle = sample(&updated, 0.0);
        let translucent_middle = sample(&translucent, 0.0);
        assert!(
            translucent_middle[0] < opaque_middle[0] && translucent_middle[1] < opaque_middle[1]
        );
        gradient
            .apply(&Event::SetEdge {
                id: "gradient".into(),
                color: Some("#2244cc".into()),
                strength: None,
                gradient: Some(false),
            })
            .unwrap();
        gpu.sync_graph(&gradient, false).unwrap();
        gpu.render(1024, 128);
        let solid = gpu.read_rgba().unwrap();
        assert_eq!(sample(&solid, -5.0), [0x22, 0x44, 0xcc]);
        assert_eq!(sample(&solid, 5.0), [0x22, 0x44, 0xcc]);
        assert_eq!(
            gpu.read_positions(2).unwrap(),
            vec![[-10.0, 0.0], [10.0, 0.0]]
        );
        validate_edge_pixel_floor(&mut gpu);

        // With inertia, very stiff springs may cross their equilibrium, but
        // crossings must stay bounded and their oscillation must decay.
        let mut stiff = Graph::new(42);
        stiff
            .apply(&Event::Batch {
                nodes: vec![node("left", [-1.0, 0.0]), node("right", [1.0, 0.0])],
                edges: vec![EdgeSpec {
                    id: "stiff".into(),
                    source: "left".into(),
                    target: "right".into(),
                    color: "#60708b".into(),
                    strength: 1_000_000.0,
                    gradient: false,
                }],
            })
            .unwrap();
        gpu.sync_graph(&stiff, true).unwrap();
        assert_eq!(gpu.effective_timestep(), 0.5e-6);
        for _ in 0..160 {
            gpu.step(1);
            let positions = gpu.read_positions(2).unwrap();
            assert!(positions.iter().flatten().all(|value| value.is_finite()));
            assert!(positions.iter().all(|position| position[0].abs() <= 1.01));
        }
        assert!(gpu
            .read_positions(2)
            .unwrap()
            .iter()
            .all(|position| position[0].abs() < 0.0001));

        // Momentum survives a force-free tick instead of being discarded. An
        // isolated node has no force at all, making the damping observable.
        let mut coasting = Graph::new(42);
        coasting
            .apply(&Event::Batch {
                nodes: vec![node("coasting", [0.0, 0.0])],
                edges: vec![],
            })
            .unwrap();
        gpu.sync_graph(&coasting, true).unwrap();
        gpu.queue.write_buffer(
            &gpu.storage.momentum,
            0,
            bytemuck::cast_slice(&[[1.0_f32, 0.5]]),
        );
        gpu.step(1);
        let first_coast = gpu.read_positions(1).unwrap()[0];
        assert_eq!(first_coast, [MOMENTUM_RETENTION, MOMENTUM_RETENTION * 0.5]);
        gpu.sync_graph(&coasting, false).unwrap();
        gpu.step(1);
        let next_coast = gpu.read_positions(1).unwrap()[0];
        assert!((next_coast[0] - first_coast[0] - MOMENTUM_RETENTION.powi(2)).abs() < 0.00001);
        gpu.sync_graph(&coasting, true).unwrap();
        gpu.step(2);
        assert_eq!(gpu.read_positions(1).unwrap()[0], [0.0, 0.0]);

        // The cap constrains stored momentum as well as visible displacement;
        // it must not hide a velocity that continues pushing at maximum speed.
        gpu.queue.write_buffer(
            &gpu.storage.momentum,
            0,
            bytemuck::cast_slice(&[[100.0_f32, 0.0]]),
        );
        gpu.step(1);
        assert_eq!(gpu.read_positions(1).unwrap()[0], [MAX_DISPLACEMENT, 0.0]);
        gpu.step(1);
        assert_eq!(
            gpu.read_positions(1).unwrap()[0],
            [MAX_DISPLACEMENT * (1.0 + MOMENTUM_RETENTION), 0.0]
        );

        crate::births::validate_gpu_births(&mut gpu);

        // Connection sizing changes GPU styles and camera bounds, but the same
        // solver inputs must produce bit-identical positions with either view.
        let mut styled = Graph::new(42);
        styled
            .apply(&Event::Batch {
                nodes: vec![node("a", [-3.0, 0.0]), node("b", [3.0, 0.0])],
                edges: vec![
                    EdgeSpec {
                        id: "ab".into(),
                        source: "a".into(),
                        target: "b".into(),
                        color: "#60708b".into(),
                        strength: 1.0,
                        gradient: false,
                    },
                    EdgeSpec {
                        id: "parallel".into(),
                        source: "a".into(),
                        target: "b".into(),
                        color: "#60708b".into(),
                        strength: 0.0,
                        gradient: false,
                    },
                    EdgeSpec {
                        id: "self".into(),
                        source: "b".into(),
                        target: "b".into(),
                        color: "#60708b".into(),
                        strength: 0.0,
                        gradient: false,
                    },
                ],
            })
            .unwrap();
        for index in 0..6 {
            let id = format!("spoke:{index}");
            styled
                .apply(&Event::Batch {
                    nodes: vec![node(&id, [index as f32, -5.0])],
                    edges: ["a", "b"]
                        .into_iter()
                        .map(|source| EdgeSpec {
                            id: format!("{source}:{id}"),
                            source: source.into(),
                            target: id.clone(),
                            color: "#60708b".into(),
                            strength: 1.0,
                            gradient: false,
                        })
                        .collect(),
                })
                .unwrap();
        }
        gpu.sync_graph(&styled, true).unwrap();
        assert_eq!(gpu.incident_degrees, vec![7, 9, 2, 2, 2, 2, 2, 2]);
        assert_eq!(read_style_radii(&gpu), vec![1.0; 8]);
        gpu.step(8);
        let baseline_positions = gpu.read_positions(8).unwrap();
        let baseline_dt = gpu.effective_timestep();
        gpu.set_degree_sizing(true);
        assert_eq!(baseline_positions, gpu.read_positions(8).unwrap());
        assert_eq!(baseline_dt, gpu.effective_timestep());
        assert_eq!(
            read_style_radii(&gpu),
            vec![
                display_radius(1.0, 7),
                display_radius(1.0, 9),
                1.0,
                1.0,
                1.0,
                1.0,
                1.0,
                1.0
            ]
        );
        assert!(styled.nodes.iter().all(|node| node.radius == 1.0));
        gpu.sync_graph(&styled, true).unwrap();
        gpu.step(8);
        assert_eq!(baseline_positions, gpu.read_positions(8).unwrap());
        gpu.set_degree_sizing(false);
        assert_eq!(read_style_radii(&gpu), vec![1.0; 8]);
        gpu.set_degree_sizing(true);
        styled
            .apply(&Event::Batch {
                nodes: vec![node("c", [0.0, 7.0])],
                edges: vec![EdgeSpec {
                    id: "ac".into(),
                    source: "a".into(),
                    target: "c".into(),
                    color: "#60708b".into(),
                    strength: 1.0,
                    gradient: false,
                }],
            })
            .unwrap();
        gpu.sync_graph(&styled, false).unwrap();
        assert_eq!(baseline_positions, gpu.read_positions(8).unwrap());
        assert_eq!(gpu.incident_degrees, vec![8, 9, 2, 2, 2, 2, 2, 2, 1]);
        assert_eq!(
            read_style_radii(&gpu),
            vec![
                display_radius(1.0, 8),
                display_radius(1.0, 9),
                1.0,
                1.0,
                1.0,
                1.0,
                1.0,
                1.0,
                1.0
            ]
        );
        gpu.set_degree_sizing(false);

        // The complete-graph growth example exceeds the old 100,000-edge cap.
        // Exercise both ping-pong births and a dense upload on the real backend,
        // then verify actual dynamics and pixels without hardware speed claims.
        let mut dense = Graph::new(42);
        gpu.sync_graph(&dense, true).unwrap();
        for number in 1..=500 {
            let id = number.to_string();
            dense
                .apply(&Event::Batch {
                    nodes: vec![NodeSpec {
                        id: id.clone(),
                        label: None,
                        color: "#89b4fa".into(),
                        radius: 0.3,
                        position: None,
                    }],
                    edges: (1..number)
                        .map(|previous| EdgeSpec {
                            id: format!("{number}:{previous}"),
                            source: id.clone(),
                            target: previous.to_string(),
                            color: "#60708b18".into(),
                            strength: crate::model::DEFAULT_EDGE_STRENGTH,
                            gradient: false,
                        })
                        .collect(),
                })
                .unwrap();
            // Testing representative growth boundaries keeps software Vulkan
            // economical while retaining an append after an odd solver tick.
            if matches!(number, 16 | 499 | 500) {
                gpu.sync_graph(&dense, false).unwrap();
                gpu.step(1);
            }
        }
        assert_eq!(dense.nodes.len(), 500);
        assert_eq!(dense.edges.len(), 124_750);
        let mut degrees = vec![0; dense.nodes.len()];
        let mut pairs = std::collections::HashSet::with_capacity(dense.edges.len());
        for edge in &dense.edges {
            assert!(edge.source > edge.target);
            assert!(pairs.insert((edge.source, edge.target)));
            degrees[edge.source] += 1;
            degrees[edge.target] += 1;
        }
        assert!(degrees.iter().all(|degree| *degree == 499));
        assert_eq!(
            gpu.effective_timestep(),
            (0.5_f64 / (499.0 * f64::from(crate::model::DEFAULT_EDGE_STRENGTH))) as f32
        );
        gpu.step(32);
        let positions = gpu.read_positions(500).unwrap();
        assert!(positions.iter().flatten().all(|value| value.is_finite()));
        assert!(positions
            .iter()
            .zip(&dense.nodes)
            .any(|(position, node)| *position != node.position));
        gpu.render(320, 180);
        let frame = gpu.read_rgba().unwrap();
        assert_eq!(frame.len(), 320 * 180 * 4);
        let pixels = frame.as_chunks::<4>().0;
        assert!(pixels.iter().all(|pixel| pixel[3] == 255));
        assert!(pixels.iter().any(|pixel| pixel != &pixels[0]));
        let camera = read_camera(&gpu);
        let base_camera = camera;
        for (position, node) in positions.iter().zip(&dense.nodes) {
            assert!((position[0] - camera[0]).abs() + node.radius <= camera[2]);
            assert!((position[1] - camera[1]).abs() + node.radius <= camera[3]);
        }
        gpu.set_degree_sizing(true);
        gpu.render(320, 180);
        let displayed_radius = display_radius(0.3, 499);
        assert!(read_style_radii(&gpu)
            .iter()
            .all(|radius| *radius == displayed_radius));
        let camera = read_camera(&gpu);
        for position in &positions {
            assert!((position[0] - camera[0]).abs() + displayed_radius <= camera[2]);
            assert!((position[1] - camera[1]).abs() + displayed_radius <= camera[3]);
        }
        assert_eq!(positions, gpu.read_positions(500).unwrap());
        gpu.set_degree_sizing(false);
        assert!(read_style_radii(&gpu).iter().all(|radius| *radius == 0.3));
        gpu.render(320, 180);
        assert_eq!(
            read_camera(&gpu),
            base_camera,
            "An idle sizing toggle must refit in one render"
        );
        eprintln!(
            "Dense graph validation: 500 nodes, 124750 edges, finite dynamics and rendered frame"
        );
    }

    fn read_buffer(gpu: &super::GpuGraph, source: &eframe::wgpu::Buffer, bytes: u64) -> Vec<u8> {
        use eframe::wgpu;
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("growth test readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_buffer_to_buffer(source, 0, &readback, 0, bytes);
        gpu.queue.submit(Some(encoder.finish()));
        gpu.map_readback(&readback).unwrap()
    }

    fn validate_dynamic_storage(device_owner: &super::GpuGraph) {
        use super::*;
        use crate::model::{EdgeSpec, Event, NodeSpec};
        let mut gpu =
            GpuGraph::new(device_owner.device.clone(), device_owner.queue.clone()).unwrap();
        assert_eq!(
            gpu.storage.capacity,
            Capacity {
                nodes: 64,
                edges: 64
            }
        );
        let make_node = |i: usize| NodeSpec {
            id: i.to_string(),
            label: None,
            color: "#3bea2c".into(),
            radius: 0.5,
            position: Some([i as f32 * 3.0, 0.0]),
        };
        let make_edge = |i: usize| EdgeSpec {
            id: format!("edge-{i}"),
            source: "0".into(),
            target: "1".into(),
            color: "#ffffff33".into(),
            strength: 4.0,
            gradient: true,
        };
        let mut graph = Graph::new(42);
        graph
            .apply(&Event::Batch {
                nodes: vec![make_node(0), make_node(1)],
                edges: vec![make_edge(0)],
            })
            .unwrap();
        gpu.sync_graph(&graph, true).unwrap();
        gpu.step(3);
        gpu.render(260, 140);
        gpu.render_preview(500, 260);
        let states: Vec<_> = gpu
            .storage
            .positions
            .iter()
            .map(|buffer| read_buffer(&gpu, buffer, 16))
            .collect();
        let momentum = read_buffer(&gpu, &gpu.storage.momentum, 16);
        assert!(momentum.iter().any(|value| *value != 0));
        let output_camera = read_buffer(&gpu, &gpu.output.camera, 16);
        let preview_camera = read_buffer(&gpu, &gpu.preview.camera, 16);
        let prior_dt = gpu.effective_timestep();
        assert!(gpu.ensure_capacity(usize::MAX, 0).is_err());
        assert_eq!(gpu.node_count, 2);
        assert_eq!(gpu.edge_count, 1);
        assert_eq!(gpu.effective_timestep(), prior_dt);
        gpu.ensure_capacity(65, 65).unwrap();
        assert_eq!(
            gpu.storage.capacity,
            Capacity {
                nodes: 128,
                edges: 128
            }
        );
        for (i, expected) in states.iter().enumerate() {
            assert_eq!(&read_buffer(&gpu, &gpu.storage.positions[i], 16), expected);
        }
        assert_eq!(read_buffer(&gpu, &gpu.storage.momentum, 16), momentum);
        assert_eq!(read_buffer(&gpu, &gpu.output.camera, 16), output_camera);
        assert_eq!(read_buffer(&gpu, &gpu.preview.camera, 16), preview_camera);
        assert_eq!(gpu.dimensions(), (260, 140));
        assert_eq!(gpu.preview_dimensions(), (500, 260));
        gpu.step(1);
        let after_growth = gpu.read_positions(2).unwrap();
        gpu.sync_graph(&graph, true).unwrap();
        gpu.step(4);
        assert_eq!(
            gpu.read_positions(2).unwrap(),
            after_growth,
            "Growth must preserve momentum and force bindings"
        );
        let momentum = read_buffer(&gpu, &gpu.storage.momentum, 16);
        let nodes = (2..9_001)
            .map(|i| {
                let mut node = make_node(i);
                if i == 9_000 {
                    node.position = None;
                }
                node
            })
            .collect();
        let mut anchor = make_edge(1);
        anchor.source = "9000".into();
        anchor.target = "0".into();
        graph
            .apply(&Event::Batch {
                nodes,
                edges: vec![anchor],
            })
            .unwrap();
        gpu.sync_graph(&graph, false).unwrap();
        assert_eq!(gpu.node_count, 9_001);
        assert_eq!(gpu.read_positions(2).unwrap(), after_growth);
        assert_eq!(read_buffer(&gpu, &gpu.storage.momentum, 16), momentum);
        let positions = gpu.read_positions(9_001).unwrap();
        for axis in 0..2 {
            assert!(
                (positions[9_000][axis]
                    - after_growth[0][axis]
                    - graph.nodes[9_000].position[axis])
                    .abs()
                    < 0.00001
            );
        }
        graph
            .apply(&Event::Batch {
                nodes: vec![],
                edges: (2..250_001).map(make_edge).collect(),
            })
            .unwrap();
        gpu.sync_graph(&graph, false).unwrap();
        assert_eq!(gpu.edge_count, 250_001);
        assert_eq!(gpu.read_positions(9_001).unwrap(), positions);
        assert_eq!(read_buffer(&gpu, &gpu.storage.momentum, 16), momentum);
        gpu.set_degree_sizing(true);
        gpu.render(320, 180);
        gpu.render_preview(400, 240);
        for image in [gpu.read_rgba().unwrap(), gpu.read_preview_rgba().unwrap()] {
            assert!(image
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| *pixel != [0, 0, 0, 255]));
        }
        // Force exactly two workgroup rows on a small, cheap graph. Results
        // must match the ordinary one-row dispatch, including padded lanes.
        let mut rows = Graph::new(8);
        rows.apply(&Event::Batch {
            nodes: (0..385).map(make_node).collect(),
            edges: vec![],
        })
        .unwrap();
        gpu.sync_graph(&rows, true).unwrap();
        gpu.step(1);
        let one_row = gpu.read_positions(385).unwrap();
        gpu.sync_graph(&rows, true).unwrap();
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&gpu.physics_pipeline);
            pass.set_bind_group(0, &gpu.physics_groups[gpu.active], &[]);
            pass.dispatch_workgroups(2, 2, 1);
        }
        gpu.active = 1 - gpu.active;
        gpu.queue.submit(Some(encoder.finish()));
        assert_eq!(gpu.read_positions(385).unwrap(), one_row);
        eprintln!("Dynamic graph validation: 9001 nodes, 250001 edges, preserved positions/momentum/cameras, matching 2D dispatch");
    }

    fn read_style_radii(gpu: &super::GpuGraph) -> Vec<f32> {
        use eframe::wgpu;
        let size = (gpu.node_count * std::mem::size_of::<super::NodeStyle>()) as u64;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test style readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("test style transfer"),
            });
        encoder.copy_buffer_to_buffer(&gpu.storage.styles, 0, &buffer, 0, size);
        gpu.queue.submit(Some(encoder.finish()));
        gpu.map_readback(&buffer)
            .unwrap()
            .as_chunks::<32>()
            .0
            .iter()
            .map(|style| f32::from_le_bytes(style[16..20].try_into().unwrap()))
            .collect()
    }

    fn validate_edge_width(gpu: &mut super::GpuGraph) {
        // The caller's red-to-blue horizontal edge has opaque endpoints, so
        // adding its linear-light red and blue channels measures coverage.
        // Check both targets, different resolutions, and a subpixel edge on
        // the centre row of an odd-height image, where naive AA adds width.
        let positions = gpu.read_positions(2).unwrap();
        for (width, height, preview) in [(1024, 128, false), (768, 96, true), (64, 9, false)] {
            for multiplier in [0.25, 1.0, 8.0] {
                gpu.set_edge_width(multiplier);
                gpu.reset_camera();
                let (frame, camera) = if preview {
                    gpu.render_preview(width, height);
                    (
                        gpu.read_preview_rgba().unwrap(),
                        read_target_camera(gpu, &gpu.preview),
                    )
                } else {
                    gpu.render(width, height);
                    (gpu.read_rgba().unwrap(), read_camera(gpu))
                };
                let integrated_width: f32 = (0..height as usize)
                    .map(|y| {
                        let start = (y * width as usize + width as usize / 2) * 4;
                        let rgb = super::linear_color([
                            f32::from(frame[start]) / 255.0,
                            f32::from(frame[start + 1]) / 255.0,
                            f32::from(frame[start + 2]) / 255.0,
                            1.0,
                        ]);
                        assert!(rgb[1] < 0.001);
                        rgb[0] + rgb[2]
                    })
                    .sum();
                let expected = (0.18 * height as f32 / (2.0 * camera[3])).max(1.0) * multiplier;
                assert!(
                    (integrated_width - expected).abs() < expected * 0.012 + 0.03,
                    "Edge width {multiplier}× at {width}×{height}, preview={preview}: \
                     expected {expected} pixels, measured {integrated_width}"
                );
                assert_eq!(gpu.read_positions(2).unwrap(), positions);
            }
        }
        // Restoring the default must restore the original appearance, without
        // a simulation reset. Subsequent checks continue testing live gradients.
        gpu.set_edge_width(1.0);
        gpu.reset_camera();
        gpu.render(1024, 128);
    }

    fn validate_edge_pixel_floor(gpu: &mut super::GpuGraph) {
        use crate::model::{EdgeSpec, Event, Graph, NodeSpec};

        // Transparent anchors make the auto-fit extent independent of edge
        // orientation. Measure the entire stroke's linear-light coverage, so
        // diagonals and subpixel translations must retain the same thickness.
        // The small extent exercises world-space widths above the floor; the
        // large extent exercises the screen floor thousands of times farther
        // out. Targets have different resolutions and even/odd pixel phases.
        for (width, height, preview) in [(384, 256, false), (511, 383, true)] {
            for extent in [10.0_f32, 10_000.0] {
                for direction in [[1.0_f32, 0.0], [0.0, 1.0], [1.0, 1.0], [0.6, 0.8]] {
                    let norm = direction[0].hypot(direction[1]);
                    let direction = [direction[0] / norm, direction[1] / norm];
                    let pixel_size = 2.0 * (extent + 0.01) * 1.08 / height as f32;
                    let centre = [0.23 * pixel_size, -0.31 * pixel_size];
                    let a = [
                        centre[0] - 0.6 * extent * direction[0],
                        centre[1] - 0.6 * extent * direction[1],
                    ];
                    let b = [
                        centre[0] + 0.6 * extent * direction[0],
                        centre[1] + 0.6 * extent * direction[1],
                    ];
                    let node = |id: &str, position| NodeSpec {
                        id: id.into(),
                        label: None,
                        color: "#ffffff00".into(),
                        radius: 0.01,
                        position: Some(position),
                    };
                    let mut graph = Graph::new(42);
                    graph
                        .apply(&Event::Batch {
                            nodes: vec![
                                node("a", a),
                                node("b", b),
                                node("lower", [-extent, -extent]),
                                node("upper", [extent, extent]),
                            ],
                            edges: vec![EdgeSpec {
                                id: "stroke".into(),
                                source: "a".into(),
                                target: "b".into(),
                                color: "#ffffff80".into(),
                                strength: 4.0,
                                gradient: false,
                            }],
                        })
                        .unwrap();
                    gpu.sync_graph(&graph, true).unwrap();
                    let positions = gpu.read_positions(4).unwrap();
                    let radii = read_style_radii(gpu);
                    for multiplier in [0.25, 1.0, 4.0] {
                        gpu.set_edge_width(multiplier);
                        gpu.reset_camera();
                        let (frame, camera) = if preview {
                            gpu.render_preview(width, height);
                            (
                                gpu.read_preview_rgba().unwrap(),
                                read_target_camera(gpu, &gpu.preview),
                            )
                        } else {
                            gpu.render(width, height);
                            (gpu.read_rgba().unwrap(), read_camera(gpu))
                        };
                        let world_per_pixel = 2.0 * camera[3] / height as f32;
                        let expected = (0.18 / world_per_pixel).max(1.0) * multiplier;
                        let area: f32 = frame
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|pixel| {
                                assert_eq!(pixel[0], pixel[1]);
                                assert_eq!(pixel[1], pixel[2]);
                                assert_eq!(pixel[3], 255);
                                super::linear_color([f32::from(pixel[0]) / 255.0, 0.0, 0.0, 1.0])[0]
                            })
                            .sum();
                        let length_pixels = (b[0] - a[0]).hypot(b[1] - a[1]) / world_per_pixel;
                        let measured = area / (length_pixels * (128.0 / 255.0));
                        assert!(
                            (measured - expected).abs() < expected * 0.02 + 0.015,
                            "Stroke {multiplier}x at {width}x{height}, preview={preview}, \
                             extent={extent}, direction={direction:?}: expected {expected} \
                             physical pixels, measured {measured}"
                        );
                    }
                    assert_eq!(gpu.read_positions(4).unwrap(), positions);
                    assert_eq!(read_style_radii(gpu), radii);
                }
            }
        }
        gpu.set_edge_width(1.0);
        gpu.reset_camera();
        eprintln!("Edge visibility validation: physical-pixel floor, zoom-scaled widths, analytic diagonal AA, preview/export, opacity and unchanged simulation positions/radii");
    }

    fn read_camera(gpu: &super::GpuGraph) -> [f32; 4] {
        read_target_camera(gpu, &gpu.output)
    }

    fn read_target_camera(gpu: &super::GpuGraph, target: &super::FrameTarget) -> [f32; 4] {
        use eframe::wgpu;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test camera readback"),
            size: 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("test camera transfer"),
            });
        encoder.copy_buffer_to_buffer(&target.camera, 0, &buffer, 0, 16);
        gpu.queue.submit(Some(encoder.finish()));
        let bytes = gpu.map_readback(&buffer).unwrap();
        std::array::from_fn(|i| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
    }
}
