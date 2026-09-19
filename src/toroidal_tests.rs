//! Wrapping-grid topology, birth ordering and degenerate ranges.
use crate::model::{Event, Graph};
use crate::rules::compile_source;
use serde_json::json;
use std::collections::BTreeSet;

const GRID: &str = include_str!("../examples/toroidal-grid.js");

fn grid(dimensions: usize, range: usize) -> Graph {
    let plan = compile_source(
        GRID,
        json!({"dimensions": dimensions, "range": range,
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
            assert!(born.contains(&edge.source));
            assert!(born.contains(&edge.target));
            assert_ne!(edge.source, edge.target);
            assert!(edge.gradient);
            assert_eq!(edge.strength, 5.0);
            assert_eq!(edge.color, "#ffffffcc");
        }
        if graph.nodes.is_empty() {
            assert!(edges.is_empty());
        } else {
            assert!(!edges.is_empty(), "{0} was born disconnected", node.id);
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

fn assert_degrees(graph: &Graph, expected: usize) {
    let mut degrees = vec![0; graph.nodes.len()];
    for edge in &graph.edges {
        degrees[edge.source] += 1;
        degrees[edge.target] += 1;
    }
    assert!(degrees.iter().all(|degree| *degree == expected));
}

#[test]
fn one_dimensional_grid_is_the_requested_ten_node_cycle() {
    let graph = grid(1, 10);
    assert_eq!((graph.nodes.len(), graph.edges.len()), (10, 10));
    assert_eq!(
        graph
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>(),
        (1..=10).map(|n| n.to_string()).collect::<Vec<_>>()
    );
    let edges = topology(&graph);
    for n in 1..10 {
        assert!(connected(&edges, &n.to_string(), &(n + 1).to_string()));
    }
    assert!(connected(&edges, "10", "1"));
    assert_degrees(&graph, 2);
}

#[test]
fn two_dimensions_wrap_each_axis_and_keep_lexicographic_births() {
    let graph = grid(2, 10);
    assert_eq!((graph.nodes.len(), graph.edges.len()), (100, 200));
    let edges = topology(&graph);
    for (index, node) in graph.nodes.iter().enumerate() {
        let a = index / 10 + 1;
        let b = index % 10 + 1;
        assert_eq!(node.id, format!("{a}-{b}"));
        assert!(connected(&edges, &node.id, &format!("{}-{b}", a % 10 + 1)));
        assert!(connected(&edges, &node.id, &format!("{a}-{}", b % 10 + 1)));
    }
    assert!(connected(&edges, "1-1", "1-2"));
    assert!(connected(&edges, "1-1", "2-1"));
    assert!(connected(&edges, "10-10", "1-10"));
    assert!(connected(&edges, "10-10", "10-1"));
    assert_degrees(&graph, 4);
}

#[test]
fn three_dimensions_add_wrapping_neighbours_without_setting_positions() {
    let graph = grid(3, 3);
    assert_eq!((graph.nodes.len(), graph.edges.len()), (27, 81));
    let edges = topology(&graph);
    for node in &graph.nodes {
        let mut coordinates: Vec<usize> = node.id.split('-').map(|n| n.parse().unwrap()).collect();
        for axis in 0..3 {
            let original = coordinates[axis];
            coordinates[axis] = original % 3 + 1;
            let neighbour = coordinates
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join("-");
            assert!(connected(&edges, &node.id, &neighbour));
            coordinates[axis] = original;
        }
        assert!(node.auto_position);
    }
    assert_degrees(&graph, 6);
}

#[test]
fn degenerate_ranges_omit_self_loops_and_duplicate_wrap_connections() {
    for dimensions in [1, 2, 16, 128] {
        let graph = grid(dimensions, 1);
        assert_eq!((graph.nodes.len(), graph.edges.len()), (1, 0));
        assert_eq!(graph.nodes[0].id, vec!["1"; dimensions].join("-"));
    }
    for dimensions in [1, 2, 3, 11] {
        let graph = grid(dimensions, 2);
        let count = 1 << dimensions;
        assert_eq!(graph.nodes.len(), count);
        assert_eq!(graph.edges.len(), count * dimensions / 2);
        topology(&graph);
        assert_degrees(&graph, dimensions);
    }
}

#[test]
fn colours_use_the_palette_in_bands_along_the_first_coordinate() {
    let palette_plan = compile_source(
        "function* generate(N) { yield N.batch(N.palette(10).map((color, i) => N.node(String(i), {color})), []); }",
        json!({}),
        42,
    )
    .unwrap();
    let mut palette_graph = Graph::new(42);
    for event in &palette_plan.events {
        palette_graph.apply(event).unwrap();
    }
    let graph = grid(2, 10);
    for (index, node) in graph.nodes.iter().enumerate() {
        assert_eq!(node.color, palette_graph.nodes[index / 10].color);
    }
    let unique: BTreeSet<_> = graph
        .nodes
        .iter()
        .map(|node| node.color.map(f32::to_bits))
        .collect();
    assert_eq!(unique.len(), 10);
}

#[test]
fn invalid_dimensions_ranges_and_unrepresentable_sizes_fail_before_generation() {
    for parameters in [
        json!({"dimensions": 0}),
        json!({"dimensions": -1}),
        json!({"dimensions": 1.5}),
        json!({"range": 0}),
        json!({"range": -1}),
        json!({"range": 1.5}),
        json!({"range": 9007199254740992_u64}),
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
        (100, 200, 840)
    );
}
