//! Open-grid topology, connected births, colour modes and representation bounds.
use crate::model::{Event, Graph};
use crate::rules::compile_source;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const GRID: &str = include_str!("../examples/grid.js");

fn grid(dimensions: usize, range: usize, color_by: &str) -> Graph {
    let plan = compile_source(
        GRID,
        json!({"dimensions": dimensions, "range": range, "colorBy": color_by,
               "ticksPerNode": 7, "strength": 5, "finalTicks": 11}),
        42,
    )
    .unwrap();
    assert_eq!(plan.total_ticks, plan.node_count as u64 * 7 + 11);
    let (last, births) = plan.events.split_last().unwrap();
    assert_eq!(*last, Event::Wait { ticks: 11 });
    let (pairs, remainder) = births.as_chunks::<2>();
    assert!(remainder.is_empty());
    let mut graph = Graph::new(42);
    let mut born = BTreeSet::new();
    for pair in pairs {
        let Event::Batch { nodes, edges } = &pair[0] else {
            panic!("Each birth must batch one node with its available edges");
        };
        assert_eq!(nodes.len(), 1);
        let node = &nodes[0];
        assert!(node.position.is_none());
        assert_eq!(node.label.as_ref(), Some(&node.id));
        assert!(born.insert(node.id.clone()));
        for edge in edges {
            assert_eq!(edge.source, node.id);
            assert!(born.contains(&edge.target));
            assert_ne!(edge.source, edge.target);
            assert!(edge.gradient);
            assert_eq!(edge.strength, 5.0);
            assert_eq!(edge.color, "#ffffffcc");
        }
        if graph.nodes.is_empty() {
            assert!(edges.is_empty());
        } else {
            assert!(!edges.is_empty(), "{} was born disconnected", node.id);
        }
        graph.apply(&pair[0]).unwrap();
        assert_eq!(pair[1], Event::Wait { ticks: 7 });
    }
    assert_eq!(graph.nodes.len(), plan.node_count);
    assert_eq!(graph.edges.len(), plan.edge_count);
    graph
}

fn topology(graph: &Graph) -> BTreeSet<(String, String)> {
    let edges: BTreeSet<_> = graph
        .edges
        .iter()
        .map(|edge| {
            let a = graph.nodes[edge.source].id.clone();
            let b = graph.nodes[edge.target].id.clone();
            if a < b {
                (a, b)
            } else {
                (b, a)
            }
        })
        .collect();
    assert_eq!(edges.len(), graph.edges.len(), "Duplicate undirected edge");
    edges
}

fn connected(edges: &BTreeSet<(String, String)>, a: &str, b: &str) -> bool {
    let pair = if a < b { (a, b) } else { (b, a) };
    edges.contains(&(pair.0.into(), pair.1.into()))
}

fn assert_grid_neighbours(graph: &Graph, dimensions: usize, range: usize) {
    let edges = topology(graph);
    let mut degrees = vec![0; graph.nodes.len()];
    for edge in &graph.edges {
        degrees[edge.source] += 1;
        degrees[edge.target] += 1;
    }
    for (index, node) in graph.nodes.iter().enumerate() {
        let mut coordinates: Vec<usize> = node.id.split('-').map(|n| n.parse().unwrap()).collect();
        assert_eq!(coordinates.len(), dimensions);
        assert_eq!(
            degrees[index],
            coordinates
                .iter()
                .map(|value| usize::from(*value > 1) + usize::from(*value < range))
                .sum::<usize>()
        );
        for axis in 0..dimensions {
            let value = coordinates[axis];
            if value < range {
                coordinates[axis] = value + 1;
                let neighbour = coordinates
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join("-");
                assert!(connected(&edges, &node.id, &neighbour));
                coordinates[axis] = value;
            }
        }
        assert!(node.auto_position);
    }
}

#[test]
fn one_dimensional_grid_is_a_path_with_open_ends() {
    let graph = grid(1, 10, "first-coordinate");
    assert_eq!((graph.nodes.len(), graph.edges.len()), (10, 9));
    assert_eq!(
        graph
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>(),
        (1..=10).map(|n| n.to_string()).collect::<Vec<_>>()
    );
    assert_grid_neighbours(&graph, 1, 10);
    assert!(!connected(&topology(&graph), "10", "1"));
}

#[test]
fn two_dimensional_grid_preserves_birth_order_without_wrapping() {
    let graph = grid(2, 10, "first-coordinate");
    assert_eq!((graph.nodes.len(), graph.edges.len()), (100, 180));
    for (index, node) in graph.nodes.iter().enumerate() {
        assert_eq!(node.id, format!("{}-{}", index / 10 + 1, index % 10 + 1));
    }
    assert_grid_neighbours(&graph, 2, 10);
    let edges = topology(&graph);
    assert!(connected(&edges, "10-10", "9-10"));
    assert!(connected(&edges, "10-10", "10-9"));
    assert!(!connected(&edges, "10-10", "1-10"));
    assert!(!connected(&edges, "10-10", "10-1"));
}

#[test]
fn higher_dimensional_grids_match_counts_and_boundary_degrees() {
    for (dimensions, range) in [(3, 3), (4, 3), (3, 5)] {
        let graph = grid(dimensions, range, "first-coordinate");
        assert_eq!(graph.nodes.len(), range.pow(dimensions as u32));
        assert_eq!(
            graph.edges.len(),
            dimensions * (range - 1) * range.pow(dimensions as u32 - 1)
        );
        assert_grid_neighbours(&graph, dimensions, range);
    }
}

#[test]
fn range_one_is_a_singleton_and_range_two_is_a_hypercube() {
    for dimensions in [1, 2, 16, 128] {
        let graph = grid(dimensions, 1, "first-coordinate");
        assert_eq!((graph.nodes.len(), graph.edges.len()), (1, 0));
        assert_eq!(graph.nodes[0].id, vec!["1"; dimensions].join("-"));
    }
    for dimensions in [1, 2, 3, 8] {
        let graph = grid(dimensions, 2, "first-coordinate");
        assert_eq!(graph.nodes.len(), 1 << dimensions);
        assert_eq!(graph.edges.len(), dimensions * (1 << (dimensions - 1)));
        assert_grid_neighbours(&graph, dimensions, 2);
    }
}

fn palette(count: usize) -> Vec<[f32; 4]> {
    let plan = compile_source(
        "function* generate(N, p) { yield N.batch(N.palette(p.count).map((color, i) => N.node(String(i), {color})), []); }",
        json!({"count": count}),
        42,
    )
    .unwrap();
    let mut graph = Graph::new(42);
    for event in &plan.events {
        graph.apply(event).unwrap();
    }
    graph.nodes.iter().map(|node| node.color).collect()
}

#[test]
fn every_colour_mode_uses_palette_order_and_preserves_topology() {
    let baseline = grid(2, 4, "first-coordinate");
    let expected_topology = topology(&baseline);
    for mode in [
        "first-coordinate",
        "last-coordinate",
        "coordinate-sum",
        "birth-order",
    ] {
        let graph = grid(2, 4, mode);
        assert_eq!(topology(&graph), expected_topology);
        let colors = palette(if mode == "birth-order" { 16 } else { 4 });
        let mut categories = BTreeMap::new();
        for (index, node) in graph.nodes.iter().enumerate() {
            let a = index / 4;
            let b = index % 4;
            let category = match mode {
                "last-coordinate" => b,
                "coordinate-sum" => (a + b) % 4,
                "birth-order" => index,
                _ => a,
            };
            let next_color = categories.len();
            let color_index = *categories.entry(category).or_insert(next_color);
            assert_eq!(node.color, colors[color_index], "{mode}: {}", node.id);
        }
        assert_eq!(categories.len(), colors.len());
    }
}

#[test]
fn grid_rejects_invalid_inputs_and_actual_representation_limits() {
    for parameters in [
        json!({"dimensions": 0}),
        json!({"dimensions": -1}),
        json!({"dimensions": 1.5}),
        json!({"range": 0}),
        json!({"range": -1}),
        json!({"range": 1.5}),
        json!({"range": 9007199254740992_u64}),
        json!({"colorBy": "unknown"}),
    ] {
        assert!(
            compile_source(GRID, parameters.clone(), 42).is_err(),
            "{parameters}"
        );
    }
    let too_many = compile_source(GRID, json!({"dimensions": 53, "range": 2}), 42).unwrap_err();
    assert!(too_many.contains("safe integer"), "{too_many}");
    for dimensions in [129_u64, 9007199254740991] {
        let too_long =
            compile_source(GRID, json!({"dimensions": dimensions, "range": 1}), 42).unwrap_err();
        assert!(too_long.contains("256-byte"), "{too_long}");
    }
    let defaults = compile_source(GRID, json!({}), 42).unwrap();
    assert_eq!(
        (
            defaults.node_count,
            defaults.edge_count,
            defaults.total_ticks
        ),
        (100, 180, 840)
    );
}
