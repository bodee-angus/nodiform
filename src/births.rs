//! Resolve new births against live GPU positions without a readback or a stall.
//!
//! New nodes are visited in rule order. Only neighbours with a lower node index
//! can anchor a birth, including nodes born earlier at the same simulated time.
//! Edges added after a node has been uploaded never relocate that existing node.
use bytemuck::{Pod, Zeroable};
use eframe::wgpu::{self, util::DeviceExt};

use crate::model::{Graph, MAX_EDGES, MAX_NODES};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Birth {
    offset: u32,
    count: u32,
    automatic: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Parameters {
    first: u32,
    count: u32,
    padding: [u32; 2],
}

pub(crate) struct BirthInitializer {
    births: wgpu::Buffer,
    neighbours: wgpu::Buffer,
    parameters: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    groups: [wgpu::BindGroup; 2],
}

impl BirthInitializer {
    pub(crate) fn new(device: &wgpu::Device, positions: &[wgpu::Buffer; 2]) -> Self {
        let buffer = |label, size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let births = buffer("birth neighbourhoods", (MAX_NODES * 16) as u64);
        // Each unique undirected connection anchors only its later endpoint.
        let neighbours = buffer("birth neighbour indices", (MAX_EDGES * 4) as u64);
        let parameters = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("birth parameters"),
            contents: bytemuck::bytes_of(&Parameters::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let entries: Vec<_> = (0..5)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 4 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding >= 2,
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("birth layout"),
            entries: &entries,
        });
        let groups = std::array::from_fn(|active| {
            let buffers = [
                &positions[active],
                &positions[1 - active],
                &births,
                &neighbours,
                &parameters,
            ];
            let entries: Vec<_> = buffers
                .iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("birth bindings"),
                layout: &layout,
                entries: &entries,
            })
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("birth shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/births.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("birth pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("birth pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            births,
            neighbours,
            parameters,
            pipeline,
            groups,
        }
    }

    /// Call after the new nodes' hints/explicit positions have been uploaded to
    /// both buffers, and before the next physics or rendering submission.
    pub(crate) fn initialise(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        graph: &Graph,
        first: usize,
        active: usize,
    ) {
        if first >= graph.nodes.len() {
            return;
        }
        let (births, neighbours) = prepare(graph, first);
        queue.write_buffer(&self.births, 0, bytemuck::cast_slice(&births));
        if !neighbours.is_empty() {
            queue.write_buffer(&self.neighbours, 0, bytemuck::cast_slice(&neighbours));
        }
        queue.write_buffer(
            &self.parameters,
            0,
            bytemuck::bytes_of(&Parameters {
                first: first as u32,
                count: graph.nodes.len() as u32,
                padding: [0; 2],
            }),
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("place new graph nodes"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("resolve live birth positions"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.groups[active], &[]);
            // One invocation preserves birth order even when several new nodes
            // in the same rule batch depend on one another's positions.
            pass.dispatch_workgroups(1, 1, 1);
        }
        queue.submit(Some(encoder.finish()));
    }
}

fn prepare(graph: &Graph, first: usize) -> (Vec<Birth>, Vec<u32>) {
    let mut predecessors = vec![Vec::new(); graph.nodes.len()];
    for edge in &graph.edges {
        let newer = edge.source.max(edge.target);
        let older = edge.source.min(edge.target);
        if newer >= first && newer != older && graph.nodes[newer].auto_position {
            predecessors[newer].push(older as u32);
        }
    }
    let mut births = Vec::with_capacity(graph.nodes.len());
    let mut neighbours = Vec::new();
    for (node, mut indices) in graph.nodes.iter().zip(predecessors) {
        // Parallel and reciprocal edges still represent the same neighbour.
        // Strength does not affect the birth centroid, including zero strength.
        indices.sort_unstable();
        indices.dedup();
        births.push(Birth {
            offset: neighbours.len() as u32,
            count: indices.len() as u32,
            automatic: u32::from(node.auto_position),
            padding: 0,
        });
        neighbours.extend(indices);
    }
    (births, neighbours)
}

/// Called from the explicit software-Vulkan smoke test, which owns the device.
#[cfg(test)]
pub(crate) fn validate_gpu_births(gpu: &mut crate::gpu::GpuGraph) {
    use crate::model::Event;

    fn apply(graph: &mut Graph, json: &str) {
        graph
            .apply(&serde_json::from_str::<Event>(json).unwrap())
            .unwrap();
    }
    fn near(actual: [f32; 2], expected: [f32; 2]) {
        for axis in 0..2 {
            assert!(
                (actual[axis] - expected[axis]).abs() < 0.0001,
                "birth position {actual:?} differs from {expected:?}"
            );
        }
    }
    let mut graph = Graph::new(42);
    apply(
        &mut graph,
        r##"{
            "op":"batch",
            "nodes":[{"id":"A","position":[-20,5]},{"id":"B","position":[60,-5]}],
            "edges":[{"id":"AB","source":"A","target":"B","strength":1}]
        }"##,
    );
    gpu.sync_graph(&graph, true);
    // An odd number selects the other ping-pong buffer, and evolves the
    // anchors far enough to distinguish live coordinates from birth hints.
    gpu.step(7);
    let anchors = gpu.read_positions(2).unwrap();
    assert_ne!(anchors[0], graph.nodes[0].position);
    assert_ne!(anchors[1], graph.nodes[1].position);
    apply(
        &mut graph,
        r##"{
            "op":"batch",
            "nodes":[
                {"id":"C"}, {"id":"D","position":[100,50]},
                {"id":"E"}, {"id":"F"}, {"id":"G"},
                {"id":"H","position":[100,100]}, {"id":"I"}
            ],
            "edges":[
                {"id":"CA","source":"C","target":"A"},
                {"id":"AC","source":"A","target":"C"},
                {"id":"CA2","source":"C","target":"A"},
                {"id":"CB","source":"C","target":"B","strength":0},
                {"id":"CC","source":"C","target":"C"},
                {"id":"DC","source":"D","target":"C"},
                {"id":"FC","source":"F","target":"C"},
                {"id":"GH","source":"G","target":"H"},
                {"id":"IA","source":"I","target":"A"}
            ]
        }"##,
    );
    gpu.sync_graph(&graph, false);
    let born = gpu.read_positions(graph.nodes.len()).unwrap();
    assert_eq!(&born[..2], &anchors);
    let centre = [
        (anchors[0][0] + anchors[1][0]) * 0.5,
        (anchors[0][1] + anchors[1][1]) * 0.5,
    ];
    let c = [
        centre[0] + graph.nodes[2].position[0],
        centre[1] + graph.nodes[2].position[1],
    ];
    near(born[2], c);
    near(born[3], [100.0, 50.0]);
    near(born[4], graph.nodes[4].position);
    near(
        born[5],
        [
            c[0] + graph.nodes[5].position[0],
            c[1] + graph.nodes[5].position[1],
        ],
    );
    near(born[6], graph.nodes[6].position);
    near(born[7], [100.0, 100.0]);
    // A two-node system's centroid is conserved. This single-parent case
    // separately proves that a birth reads the moved GPU position, not the
    // parent's original hint that happens to have the same collective centre.
    near(
        born[8],
        [
            anchors[0][0] + graph.nodes[8].position[0],
            anchors[0][1] + graph.nodes[8].position[1],
        ],
    );

    // A new link after birth changes forces, but cannot teleport an old node.
    apply(
        &mut graph,
        r##"{"op":"batch","edges":[{"id":"EA","source":"E","target":"A"}]}"##,
    );
    gpu.sync_graph(&graph, false);
    assert_eq!(gpu.read_positions(graph.nodes.len()).unwrap(), born);

    // Reset resolves every birth afresh from explicit parent coordinates,
    // never by adding a centroid again to an already-resolved position.
    gpu.sync_graph(&graph, true);
    let reset = gpu.read_positions(graph.nodes.len()).unwrap();
    near(
        reset[2],
        [
            20.0 + graph.nodes[2].position[0],
            graph.nodes[2].position[1],
        ],
    );
    gpu.step(8);
    let replay = gpu.read_positions(graph.nodes.len()).unwrap();
    assert!(replay.iter().flatten().all(|value| value.is_finite()));
    gpu.sync_graph(&graph, true);
    gpu.step(8);
    assert_eq!(gpu.read_positions(graph.nodes.len()).unwrap(), replay);
}

#[cfg(test)]
mod tests {
    use crate::model::Event;

    use super::*;

    #[test]
    fn birth_neighbours_follow_node_order_and_ignore_duplicate_or_self_edges() {
        let mut graph = Graph::new(42);
        let event: Event = serde_json::from_str(
            r##"{
                "op":"batch",
                "nodes":[{"id":"A"},{"id":"B"},{"id":"C"},{"id":"D"}],
                "edges":[
                    {"id":"1","source":"A","target":"C"},
                    {"id":"2","source":"C","target":"A"},
                    {"id":"3","source":"A","target":"C"},
                    {"id":"4","source":"B","target":"C","strength":0},
                    {"id":"5","source":"C","target":"C"},
                    {"id":"6","source":"C","target":"D"}
                ]
            }"##,
        )
        .unwrap();
        graph.apply(&event).unwrap();
        let (births, neighbours) = prepare(&graph, 0);
        assert_eq!(
            births.iter().map(|b| b.count).collect::<Vec<_>>(),
            [0, 0, 2, 1]
        );
        assert_eq!(neighbours, [0, 1, 2]);
        assert!(births.iter().all(|birth| birth.automatic == 1));
        // Once C exists, a later sync only places D. Existing nodes never
        // acquire a new automatic birth merely because their edges changed.
        let (births, neighbours) = prepare(&graph, 3);
        assert_eq!(
            births.iter().map(|b| b.count).collect::<Vec<_>>(),
            [0, 0, 0, 1]
        );
        assert_eq!(neighbours, [2]);
    }

    #[test]
    fn explicit_positions_bypass_automatic_placement_but_can_anchor_later_births() {
        let mut graph = Graph::new(1);
        let event: Event = serde_json::from_str(
            r##"{
                "op":"batch",
                "nodes":[{"id":"A"},{"id":"B","position":[20,-5]},{"id":"C"}],
                "edges":[
                    {"id":"1","source":"A","target":"B"},
                    {"id":"2","source":"B","target":"C"}
                ]
            }"##,
        )
        .unwrap();
        graph.apply(&event).unwrap();
        let (births, neighbours) = prepare(&graph, 0);
        assert_eq!(births[1].automatic, 0);
        assert_eq!(births[1].count, 0);
        assert_eq!(graph.nodes[1].position, [20.0, -5.0]);
        assert_eq!(births[2].count, 1);
        assert_eq!(neighbours, [1]);
    }
}
