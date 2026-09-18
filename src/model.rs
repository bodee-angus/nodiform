//! Validated, ordered graph events. Positions here are birth hints, not solver state.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const MAX_NODES: usize = 8_192;
// A complete 500-node graph has 124,750 edges. Keep the bounded exact solver
// useful for dense experiments as well as sparse growth rules.
pub const MAX_EDGES: usize = 250_000;
pub const BIRTH_PLACEMENT_VERSION: &str = "live-neighbour-centroid-v2";
/// Pinned force parameters shared by the solver, defaults and recording metadata.
pub const FORCE_VERSION: &str = "nodiform-force-v3";
pub const REPULSION: f32 = 512.0;
pub const DEFAULT_EDGE_STRENGTH: f32 = 4.0;
pub const SOFTENING_SQUARED: f32 = 0.25;
pub const MAX_DISPLACEMENT: f32 = 2.0;
pub const BASE_TIMESTEP: f32 = 1.0 / 120.0;
/// Fraction of the previous tick's displacement retained by the damped solver.
pub const MOMENTUM_RETENTION: f32 = 0.85;
/// Domain limits keep finite user inputs within the GPU solver's numeric range.
pub const MAX_NUMERIC_MAGNITUDE: f32 = 1_000_000.0;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub color: [f32; 4],
    pub radius: f32,
    /// Explicit birth coordinates, or a small offset for automatic placement.
    /// The GPU owns the evolving coordinates and resolves automatic births
    /// against the live positions of already-present connected neighbours.
    pub position: [f32; 2],
    #[serde(default)]
    pub auto_position: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    pub id: String,
    pub source: usize,
    pub target: usize,
    pub color: [f32; 4],
    pub strength: f32,
    #[serde(default)]
    pub gradient: bool,
}

fn default_node_color() -> String {
    "#89b4fa".into()
}
fn default_edge_color() -> String {
    "#60708b99".into()
}
fn default_radius() -> f32 {
    1.6
}
fn default_strength() -> f32 {
    DEFAULT_EDGE_STRENGTH
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NodeSpec {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_node_color")]
    pub color: String,
    #[serde(default = "default_radius")]
    pub radius: f32,
    #[serde(default)]
    pub position: Option<[f32; 2]>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EdgeSpec {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default = "default_edge_color")]
    pub color: String,
    #[serde(default = "default_strength")]
    pub strength: f32,
    #[serde(default)]
    pub gradient: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Batch {
        #[serde(default)]
        nodes: Vec<NodeSpec>,
        #[serde(default)]
        edges: Vec<EdgeSpec>,
    },
    Wait {
        ticks: u32,
    },
    SetNode {
        id: String,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        radius: Option<f32>,
    },
    SetEdge {
        id: String,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        strength: Option<f32>,
        #[serde(default)]
        gradient: Option<bool>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Plan {
    pub events: Vec<Event>,
    /// Sum of explicit Wait ticks; other events consume no simulated time.
    pub total_ticks: u64,
    pub node_count: usize,
    pub edge_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    seed: u32,
    #[serde(skip)]
    node_indices: HashMap<String, usize>,
    #[serde(skip)]
    edge_indices: HashMap<String, usize>,
}

impl Graph {
    pub fn new(seed: u32) -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            seed,
            node_indices: HashMap::new(),
            edge_indices: HashMap::new(),
        }
    }

    /// A rejected event leaves the graph entirely unchanged.
    pub fn apply(&mut self, event: &Event) -> Result<(), String> {
        match event {
            Event::Wait { .. } => Ok(()),
            Event::Batch { nodes, edges } => self.apply_batch(nodes, edges),
            Event::SetNode { id, color, radius } => {
                let &index = self
                    .node_indices
                    .get(id)
                    .ok_or_else(|| format!("Unknown node '{id}'"))?;
                let color = color.as_deref().map(parse_color).transpose()?;
                if let Some(radius) = radius {
                    validate_radius(*radius)?;
                }
                let node = &mut self.nodes[index];
                if let Some(color) = color {
                    node.color = color;
                }
                if let Some(radius) = radius {
                    node.radius = *radius;
                }
                Ok(())
            }
            Event::SetEdge {
                id,
                color,
                strength,
                gradient,
            } => {
                let &index = self
                    .edge_indices
                    .get(id)
                    .ok_or_else(|| format!("Unknown edge '{id}'"))?;
                let color = color.as_deref().map(parse_color).transpose()?;
                if let Some(strength) = strength {
                    validate_strength(*strength)?;
                }
                let edge = &mut self.edges[index];
                if let Some(color) = color {
                    edge.color = color;
                }
                if let Some(strength) = strength {
                    edge.strength = *strength;
                }
                if let Some(gradient) = gradient {
                    edge.gradient = *gradient;
                }
                Ok(())
            }
        }
    }

    fn apply_batch(&mut self, nodes: &[NodeSpec], edges: &[EdgeSpec]) -> Result<(), String> {
        if nodes.len() > MAX_NODES.saturating_sub(self.nodes.len()) {
            return Err(format!(
                "This exact-solver build allows at most {MAX_NODES} nodes"
            ));
        }
        if edges.len() > MAX_EDGES.saturating_sub(self.edges.len()) {
            return Err(format!("This build allows at most {MAX_EDGES} edges"));
        }
        let mut new_nodes = Vec::with_capacity(nodes.len());
        let mut new_edges = Vec::with_capacity(edges.len());
        let mut staged_nodes = HashMap::with_capacity(nodes.len());
        let mut staged_edges = HashSet::with_capacity(edges.len());

        for spec in nodes {
            validate_id(&spec.id)?;
            if self.node_indices.contains_key(&spec.id) || staged_nodes.contains_key(&spec.id) {
                return Err(format!("Duplicate node ID '{}'", spec.id));
            }
            validate_radius(spec.radius)?;
            let label = spec.label.clone().unwrap_or_else(|| spec.id.clone());
            if label.len() > 1_024 {
                return Err("Node labels must be at most 1024 bytes".into());
            }
            let position = spec
                .position
                .unwrap_or_else(|| birth_position(self.seed, &spec.id));
            if !position
                .iter()
                .all(|v| v.is_finite() && v.abs() <= MAX_NUMERIC_MAGNITUDE)
            {
                return Err(format!(
                    "Node '{}' birth coordinates must be finite and within ±1000000",
                    spec.id
                ));
            }
            let color = parse_color(&spec.color)?;
            staged_nodes.insert(spec.id.clone(), self.nodes.len() + new_nodes.len());
            new_nodes.push(Node {
                id: spec.id.clone(),
                label,
                color,
                radius: spec.radius,
                position,
                auto_position: spec.position.is_none(),
            });
        }

        for spec in edges {
            validate_id(&spec.id)?;
            if self.edge_indices.contains_key(&spec.id) || !staged_edges.insert(spec.id.clone()) {
                return Err(format!("Duplicate edge ID '{}'", spec.id));
            }
            validate_strength(spec.strength)?;
            let lookup = |id: &str| {
                self.node_indices
                    .get(id)
                    .or_else(|| staged_nodes.get(id))
                    .copied()
                    .ok_or_else(|| format!("Edge '{}' refers to missing node '{id}'", spec.id))
            };
            new_edges.push(Edge {
                id: spec.id.clone(),
                source: lookup(&spec.source)?,
                target: lookup(&spec.target)?,
                color: parse_color(&spec.color)?,
                strength: spec.strength,
                gradient: spec.gradient,
            });
        }

        // Commit only after every node and edge has passed validation.
        for node in new_nodes {
            self.node_indices.insert(node.id.clone(), self.nodes.len());
            self.nodes.push(node);
        }
        for edge in new_edges {
            self.edge_indices.insert(edge.id.clone(), self.edges.len());
            self.edges.push(edge);
        }
        Ok(())
    }

    /// Components of the attraction graph. Zero-weight edges do not bind components.
    pub fn component_count(&self) -> usize {
        let mut parents: Vec<usize> = (0..self.nodes.len()).collect();
        let mut count = parents.len();
        fn root(parents: &mut [usize], mut node: usize) -> usize {
            while parents[node] != node {
                parents[node] = parents[parents[node]];
                node = parents[node];
            }
            node
        }
        for edge in &self.edges {
            if edge.strength == 0.0 {
                continue;
            }
            let left = root(&mut parents, edge.source);
            let right = root(&mut parents, edge.target);
            if left != right {
                parents[right] = left;
                count -= 1;
            }
        }
        count
    }
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        Err("IDs must contain 1–256 bytes and no control characters".into())
    } else {
        Ok(())
    }
}

fn validate_radius(radius: f32) -> Result<(), String> {
    if !radius.is_finite() || radius <= 0.0 || radius > MAX_NUMERIC_MAGNITUDE {
        Err("Node radius must be finite, greater than zero, and at most 1000000".into())
    } else {
        Ok(())
    }
}

fn validate_strength(strength: f32) -> Result<(), String> {
    if !strength.is_finite() || !(0.0..=MAX_NUMERIC_MAGNITUDE).contains(&strength) {
        Err("Edge strength must be finite and between 0 and 1000000".into())
    } else {
        Ok(())
    }
}

pub fn parse_color(text: &str) -> Result<[f32; 4], String> {
    let hex = text.strip_prefix('#').unwrap_or(text);
    if (hex.len() != 6 && hex.len() != 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("Invalid colour '{text}': use #RRGGBB or #RRGGBBAA"));
    }
    let mut color = [1.0; 4];
    for (i, channel) in color.iter_mut().enumerate().take(hex.len() / 2) {
        *channel = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16)
            .map_err(|_| format!("Invalid colour '{text}'"))? as f32
            / 255.0;
    }
    Ok(color)
}

/// FNV-1a of UTF-8 ID plus seed, followed by two integer avalanches.
/// Samples a 2×2 world-unit square. The GPU adds this deterministic offset to
/// the neighbours' current centroid, or the origin if no neighbours exist.
/// A small offset avoids coincident births with no direction for repulsion.
fn birth_position(seed: u32, id: &str) -> [f32; 2] {
    let mut hash = 2_166_136_261u32 ^ seed;
    for byte in id.bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(16_777_619);
    }
    fn mix(mut n: u32) -> u32 {
        n ^= n >> 16;
        n = n.wrapping_mul(0x7feb_352d);
        n ^= n >> 15;
        n = n.wrapping_mul(0x846c_a68b);
        n ^ (n >> 16)
    }
    let component = |n: u32| ((mix(n) >> 8) as f32 / 16_777_216.0 - 0.5) * 2.0;
    [component(hash), component(hash ^ 0x9e37_79b9)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str) -> NodeSpec {
        NodeSpec {
            id: id.into(),
            label: None,
            color: default_node_color(),
            radius: 1.0,
            position: None,
        }
    }
    fn edge(id: &str, source: &str, target: &str) -> EdgeSpec {
        EdgeSpec {
            id: id.into(),
            source: source.into(),
            target: target.into(),
            color: default_edge_color(),
            strength: 1.0,
            gradient: false,
        }
    }
    #[test]
    fn edge_defaults_preserve_solid_appearance_and_raise_new_attraction() {
        let edge: EdgeSpec =
            serde_json::from_str(r##"{"id":"e","source":"a","target":"b"}"##).unwrap();
        assert_eq!(edge.strength, DEFAULT_EDGE_STRENGTH);
        assert!(!edge.gradient);
        let old: EdgeSpec =
            serde_json::from_str(r##"{"id":"e","source":"a","target":"b","strength":1}"##).unwrap();
        assert_eq!(old.strength, 1.0, "Explicit old strengths remain unchanged");
        assert!(serde_json::from_str::<EdgeSpec>(
            r##"{"id":"e","source":"a","target":"b","gradient":"yes"}"##
        )
        .is_err());
    }

    #[test]
    fn gradients_are_optional_mutable_and_updates_are_atomic() {
        let mut graph = Graph::new(42);
        let mut link = edge("e", "a", "b");
        link.gradient = true;
        graph
            .apply(&Event::Batch {
                nodes: vec![node("a"), node("b")],
                edges: vec![link],
            })
            .unwrap();
        assert!(graph.edges[0].gradient);
        let before = graph.edges[0].clone();
        assert!(graph
            .apply(&Event::SetEdge {
                id: "e".into(),
                color: Some("invalid".into()),
                strength: Some(2.0),
                gradient: Some(false),
            })
            .is_err());
        assert_eq!(graph.edges[0], before);
        graph
            .apply(&Event::SetEdge {
                id: "e".into(),
                color: None,
                strength: None,
                gradient: Some(false),
            })
            .unwrap();
        assert!(!graph.edges[0].gradient);
        assert_eq!(graph.edges[0].strength, before.strength);
        assert_eq!(graph.edges[0].color, before.color);
        let legacy: Event =
            serde_json::from_str(r##"{"op":"set_edge","id":"e","strength":2}"##).unwrap();
        graph.apply(&legacy).unwrap();
        assert!(!graph.edges[0].gradient);
        assert_eq!(graph.edges[0].strength, 2.0);
    }

    #[test]
    fn no_partial_batch_on_missing_endpoint() {
        let mut graph = Graph::new(7);
        let event = Event::Batch {
            nodes: vec![node("a")],
            edges: vec![edge("e", "a", "missing")],
        };
        assert!(graph.apply(&event).is_err());
        assert!(graph.nodes.is_empty());
        assert!(graph.edges.is_empty());
        graph
            .apply(&Event::Batch {
                nodes: vec![node("a")],
                edges: vec![],
            })
            .unwrap();
    }
    #[test]
    fn negative_strength_rejects_whole_batch() {
        let mut graph = Graph::new(0);
        let mut invalid = edge("e", "a", "b");
        invalid.strength = -1.0;
        assert!(graph
            .apply(&Event::Batch {
                nodes: vec![node("a"), node("b")],
                edges: vec![invalid]
            })
            .is_err());
        assert!(graph.nodes.is_empty());
    }
    #[test]
    fn same_batch_endpoints_and_duplicate_ids() {
        let mut graph = Graph::new(0);
        graph
            .apply(&Event::Batch {
                nodes: vec![node("a"), node("b")],
                edges: vec![edge("e", "a", "b")],
            })
            .unwrap();
        assert_eq!(graph.edges[0].source, 0);
        assert_eq!(graph.edges[0].target, 1);
        assert_eq!(graph.component_count(), 1);
        assert!(graph
            .apply(&Event::Batch {
                nodes: vec![node("c"), node("c")],
                edges: vec![]
            })
            .is_err());
        assert_eq!(graph.nodes.len(), 2);
    }

    #[test]
    fn edge_capacity_accepts_boundary_and_rejects_next_batch_atomically() {
        let mut graph = Graph::new(7);
        // Parallel edges are valid: use them to exercise the exact capacity
        // without conflating an edge limit with a particular node count.
        graph
            .apply(&Event::Batch {
                nodes: vec![node("a"), node("b")],
                edges: (0..MAX_EDGES)
                    .map(|index| edge(&format!("e:{index}"), "a", "b"))
                    .collect(),
            })
            .unwrap();
        assert_eq!(graph.edges.len(), MAX_EDGES);
        let error = graph
            .apply(&Event::Batch {
                nodes: vec![node("c")],
                edges: vec![edge("overflow", "b", "c")],
            })
            .unwrap_err();
        assert!(error.contains(&MAX_EDGES.to_string()));
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), MAX_EDGES);
        assert!(!graph.node_indices.contains_key("c"));
        assert!(!graph.edge_indices.contains_key("overflow"));
        graph
            .apply(&Event::SetEdge {
                id: format!("e:{}", MAX_EDGES - 1),
                color: None,
                strength: Some(0.25),
                gradient: None,
            })
            .unwrap();
        assert_eq!(graph.edges.last().unwrap().strength, 0.25);
    }
    #[test]
    fn zero_weight_edges_do_not_bind_components() {
        let mut graph = Graph::new(0);
        graph
            .apply(&Event::Batch {
                nodes: vec![node("a"), node("b")],
                edges: vec![edge("e", "a", "b")],
            })
            .unwrap();
        graph
            .apply(&Event::SetEdge {
                id: "e".into(),
                color: None,
                strength: Some(0.0),
                gradient: None,
            })
            .unwrap();
        assert_eq!(graph.component_count(), 2);
    }
    #[test]
    fn property_updates_are_transactional() {
        let mut graph = Graph::new(0);
        graph
            .apply(&Event::Batch {
                nodes: vec![node("a")],
                edges: vec![],
            })
            .unwrap();
        let before = graph.nodes.clone();
        assert!(graph
            .apply(&Event::SetNode {
                id: "a".into(),
                color: Some("#ffffff".into()),
                radius: Some(-2.0)
            })
            .is_err());
        assert_eq!(before, graph.nodes);
    }
    #[test]
    fn birth_jitter_is_seeded_and_order_independent() {
        assert_eq!(birth_position(42, "a"), birth_position(42, "a"));
        assert_ne!(birth_position(42, "a"), birth_position(43, "a"));
        assert_ne!(birth_position(42, "a"), birth_position(42, "b"));
        let mut left = Graph::new(42);
        let mut right = Graph::new(42);
        left.apply(&Event::Batch {
            nodes: vec![node("a"), node("b")],
            edges: vec![],
        })
        .unwrap();
        right
            .apply(&Event::Batch {
                nodes: vec![node("b"), node("a")],
                edges: vec![],
            })
            .unwrap();
        assert_eq!(left.nodes[0].position, right.nodes[1].position);
        assert!(left.nodes.iter().all(|node| node.auto_position));
        assert!(left
            .nodes
            .iter()
            .flat_map(|node| node.position)
            .all(|coordinate| (-1.0..1.0).contains(&coordinate)));
    }
    #[test]
    fn explicit_birth_positions_are_distinguished_from_auto_offsets() {
        let mut graph = Graph::new(42);
        let mut explicit = node("explicit");
        explicit.position = Some([0.0, 0.0]);
        graph
            .apply(&Event::Batch {
                nodes: vec![node("automatic"), explicit],
                edges: vec![],
            })
            .unwrap();
        assert!(graph.nodes[0].auto_position);
        assert!(!graph.nodes[1].auto_position);
        assert_eq!(graph.nodes[1].position, [0.0, 0.0]);
    }
    #[test]
    fn colors_and_finite_values_are_validated() {
        assert_eq!(
            parse_color("#ff000080").unwrap(),
            [1.0, 0.0, 0.0, 128.0 / 255.0]
        );
        assert!(parse_color("#🚫000").is_err());
        let mut graph = Graph::new(0);
        let mut invalid = node("a");
        invalid.position = Some([f32::NAN, 0.0]);
        assert!(graph
            .apply(&Event::Batch {
                nodes: vec![invalid],
                edges: vec![]
            })
            .is_err());
    }
    #[test]
    fn finite_but_excessive_gpu_values_are_rejected() {
        let mut graph = Graph::new(0);
        let mut excessive = node("a");
        excessive.position = Some([f32::MAX, 0.0]);
        assert!(graph
            .apply(&Event::Batch {
                nodes: vec![excessive],
                edges: vec![]
            })
            .is_err());
        let mut excessive = node("a");
        excessive.radius = MAX_NUMERIC_MAGNITUDE + 1.0;
        assert!(graph
            .apply(&Event::Batch {
                nodes: vec![excessive],
                edges: vec![]
            })
            .is_err());
        let mut excessive = edge("e", "a", "b");
        excessive.strength = f32::MAX;
        assert!(graph
            .apply(&Event::Batch {
                nodes: vec![node("a"), node("b")],
                edges: vec![excessive]
            })
            .is_err());
        assert!(graph.nodes.is_empty());
        graph
            .apply(&Event::Batch {
                nodes: vec![node("a"), node("b")],
                edges: vec![edge("e", "a", "b")],
            })
            .unwrap();
        assert!(graph
            .apply(&Event::SetNode {
                id: "a".into(),
                color: None,
                radius: Some(f32::MAX)
            })
            .is_err());
        assert!(graph
            .apply(&Event::SetEdge {
                id: "e".into(),
                color: None,
                strength: Some(f32::MAX),
                gradient: None,
            })
            .is_err());
        assert_eq!(graph.nodes[0].radius, 1.0);
        assert_eq!(graph.edges[0].strength, 1.0);
        assert!(validate_radius(MAX_NUMERIC_MAGNITUDE).is_ok());
        assert!(validate_strength(MAX_NUMERIC_MAGNITUDE).is_ok());
    }
}
