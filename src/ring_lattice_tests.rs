//! Cyclic predecessor topology, sequential births, colour modes and input bounds.
use crate::model::{Event, Graph};
use crate::rules::compile_source;
use serde_json::json;
use std::collections::BTreeSet;

const RING_LATTICE: &str = include_str!("../examples/ring-lattice.js");

fn lattice(count: usize, previous_count: usize, color_by: &str) -> Graph {
    let plan = compile_source(
        RING_LATTICE,
        json!({"count": count, "previousCount": previous_count, "colorBy": color_by,
               "ticksPerNode": 7, "strength": 5, "finalTicks": 11}),
        42,
    )
    .unwrap();
    assert_eq!(plan.node_count, count);
    assert_eq!(plan.total_ticks, count as u64 * 7 + 11);
    let (last, births) = plan.events.split_last().unwrap();
    assert_eq!(*last, Event::Wait { ticks: 11 });
    let (pairs, remainder) = births.as_chunks::<2>();
    assert!(remainder.is_empty());
    assert_eq!(pairs.len(), count);
    let mut graph = Graph::new(42);
    for (index, pair) in pairs.iter().enumerate() {
        let Event::Batch { nodes, edges } = &pair[0] else {
            panic!("Each birth must batch one node with its available edges");
        };
        assert_eq!(nodes.len(), 1);
        let node = &nodes[0];
        assert_eq!(node.id, (index + 1).to_string());
        assert_eq!(node.label.as_ref(), Some(&node.id));
        assert!(node.position.is_none());
        for edge in edges {
            let source = edge.source.parse::<usize>().unwrap();
            let target = edge.target.parse::<usize>().unwrap();
            assert!((1..=index + 1).contains(&source));
            assert!((1..=index + 1).contains(&target));
            assert_ne!(source, target);
            assert_eq!(
                source.max(target),
                index + 1,
                "Edge arrived after its endpoints"
            );
            assert!(edge.gradient);
            assert_eq!(edge.strength, 5.0);
            assert_eq!(edge.color, "#ffffffcc");
        }
        if index == 0 || previous_count == 0 {
            assert!(edges.is_empty());
        } else {
            assert!(!edges.is_empty(), "{} was born disconnected", node.id);
        }
        graph.apply(&pair[0]).unwrap();
        assert_eq!(pair[1], Event::Wait { ticks: 7 });
    }
    assert_eq!(graph.edges.len(), plan.edge_count);
    assert!(graph.nodes.iter().all(|node| node.auto_position));
    graph
}

fn topology(graph: &Graph) -> BTreeSet<(usize, usize)> {
    let pairs: BTreeSet<_> = graph
        .edges
        .iter()
        .map(|edge| {
            let a = graph.nodes[edge.source].id.parse::<usize>().unwrap();
            let b = graph.nodes[edge.target].id.parse::<usize>().unwrap();
            assert_ne!(a, b);
            (a.min(b), a.max(b))
        })
        .collect();
    assert_eq!(pairs.len(), graph.edges.len(), "Duplicate undirected edge");
    pairs
}

#[test]
fn requested_ten_node_lattice_adds_wrap_edges_as_endpoints_are_born() {
    let graph = lattice(10, 2, "birth-order");
    assert_eq!(graph.edges.len(), 20);
    let edges = topology(&graph);
    let expected_previous: [&[usize]; 10] = [
        &[],
        &[1],
        &[1, 2],
        &[2, 3],
        &[3, 4],
        &[4, 5],
        &[5, 6],
        &[6, 7],
        &[1, 7, 8],
        &[1, 2, 8, 9],
    ];
    for (index, expected) in expected_previous.iter().enumerate() {
        let number = index + 1;
        let actual: Vec<_> = edges
            .iter()
            .filter_map(|&(a, b)| (b == number).then_some(a))
            .collect();
        assert_eq!(actual.as_slice(), *expected, "Birth {number}");
        assert_eq!(
            edges
                .iter()
                .filter(|&&(a, b)| a == number || b == number)
                .count(),
            4,
            "Final degree of node {number}"
        );
    }
}

#[test]
fn cyclic_distance_oracle_covers_odd_even_and_saturated_lattices() {
    for count in 1..=8 {
        for previous_count in 0..=count + 2 {
            let graph = lattice(count, previous_count, "birth-order");
            let actual = topology(&graph);
            let expected: BTreeSet<_> = (1..=count)
                .flat_map(|a| (a + 1..=count).map(move |b| (a, b)))
                .filter(|&(a, b)| (b - a).min(count - (b - a)) <= previous_count)
                .collect();
            assert_eq!(
                actual, expected,
                "count={count}, previousCount={previous_count}"
            );
            assert_eq!(
                graph.edges.len(),
                count * (2 * previous_count).min(count - 1) / 2,
                "Each node has the same degree, with antipodal edges counted once"
            );
        }
    }
    // A valid, very large connection count must saturate without iterating to it.
    let graph = lattice(6, 9_007_199_254_740_991, "birth-order");
    assert_eq!((graph.nodes.len(), topology(&graph).len()), (6, 15));
}

#[test]
fn node_count_can_exceed_the_former_limit() {
    let plan = compile_source(
        RING_LATTICE,
        json!({"count": 8193, "previousCount": 1, "ticksPerNode": 0, "finalTicks": 0}),
        42,
    )
    .unwrap();
    assert_eq!(
        (plan.node_count, plan.edge_count, plan.total_ticks),
        (8193, 8193, 0)
    );
    let mut graph = Graph::new(42);
    for event in &plan.events {
        graph.apply(event).unwrap();
    }
    let edges = topology(&graph);
    assert!(edges.contains(&(1, 8193)));
    assert!(edges
        .iter()
        .all(|&(a, b)| b == a + 1 || (a, b) == (1, 8193)));
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
fn colour_modes_follow_palette_order_without_changing_connections() {
    for count in [1, 2, 9, 10] {
        let baseline = lattice(count, 2, "birth-order");
        let expected_topology = topology(&baseline);
        for mode in ["birth-order", "ring-distance"] {
            let graph = lattice(count, 2, mode);
            assert_eq!(topology(&graph), expected_topology);
            let colors = palette(if mode == "birth-order" {
                count
            } else {
                count / 2 + 1
            });
            for (index, node) in graph.nodes.iter().enumerate() {
                let category = if mode == "birth-order" {
                    index
                } else {
                    index.min(count - index)
                };
                assert_eq!(node.color, colors[category], "{mode}: {}", node.id);
            }
        }
    }
}

#[test]
fn default_timing_includes_every_birth_and_final_settling() {
    let plan = compile_source(RING_LATTICE, json!({}), 42).unwrap();
    assert_eq!(
        (plan.node_count, plan.edge_count, plan.total_ticks),
        (10, 20, 480)
    );
    assert_eq!(plan.events.last(), Some(&Event::Wait { ticks: 240 }));
    assert_eq!(
        plan.events
            .iter()
            .filter(|event| **event == Event::Wait { ticks: 24 })
            .count(),
        10
    );
}

#[test]
fn zero_tick_births_preserve_wrap_connections_and_settling() {
    let plan = compile_source(
        RING_LATTICE,
        json!({"count": 10, "previousCount": 2, "ticksPerNode": 0, "finalTicks": 123}),
        42,
    )
    .unwrap();
    assert_eq!(
        (plan.node_count, plan.edge_count, plan.total_ticks),
        (10, 20, 123)
    );
    assert_eq!(plan.events.last(), Some(&Event::Wait { ticks: 123 }));
    let mut graph = Graph::new(42);
    for event in &plan.events {
        graph.apply(event).unwrap();
    }
    assert_eq!(topology(&graph), topology(&lattice(10, 2, "birth-order")));
}

#[test]
fn invalid_counts_colours_and_timing_are_rejected() {
    for parameters in [
        json!({"count": 0}),
        json!({"count": -1}),
        json!({"count": 1.5}),
        json!({"count": 9007199254740992_u64}),
        json!({"previousCount": -1}),
        json!({"previousCount": 1.5}),
        json!({"previousCount": 9007199254740992_u64}),
        json!({"colorBy": "unknown"}),
        json!({"ticksPerNode": -1}),
        json!({"ticksPerNode": 4294967296_u64}),
        json!({"finalTicks": -1}),
        json!({"finalTicks": 4294967296_u64}),
        json!({"strength": -1}),
    ] {
        assert!(
            compile_source(RING_LATTICE, parameters.clone(), 42).is_err(),
            "{parameters}"
        );
    }
}
