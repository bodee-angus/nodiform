//! Cross-layer checks of the shipped experiments and their final graph semantics.
use crate::model::{Event, Graph};
use crate::rules::compile_source;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const PERMUTATIONS: &str = include_str!("../examples/abc-permutations.js");

fn permutation(alphabet: &str, repeat: bool, order: &str, seed: u32) -> Graph {
    let plan = compile_source(
        PERMUTATIONS,
        json!({"alphabet": alphabet, "maxLength": 3, "repetitions": repeat,
               "order": order, "ticksPerNode": 3, "finalTicks": 0}),
        seed,
    )
    .unwrap();
    assert_eq!(plan.total_ticks, plan.node_count as u64 * 3);
    let mut graph = Graph::new(seed);
    for event in &plan.events {
        graph.apply(event).unwrap();
    }
    graph
}

fn topology(graph: &Graph) -> BTreeSet<(String, String)> {
    graph
        .edges
        .iter()
        .map(|edge| {
            (
                graph.nodes[edge.source].id.clone(),
                graph.nodes[edge.target].id.clone(),
            )
        })
        .collect()
}

#[test]
fn branch_walk_preserves_the_requested_birth_sequence_and_complete_topology() {
    let graph = permutation("ABC", false, "branch-walk", 42);
    let ids: Vec<_> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "A", "AB", "ABC", "ACB", "AC", "B", "BA", "BAC", "BCA", "BC", "C", "CA", "CAB", "CBA",
            "CB"
        ]
    );
    assert_eq!(graph.edges.len(), 24);
    let edges = topology(&graph);
    assert!(edges.contains(&("ABC".into(), "AB".into())));
    assert!(edges.contains(&("ABC".into(), "BC".into())));
    assert!(!edges.contains(&("ABC".into(), "AC".into())));
    assert_eq!(
        edges,
        topology(&permutation("ABC", false, "lexicographic", 42))
    );
}

#[test]
fn every_birth_order_preserves_topology_and_first_letter_colours() {
    for alphabet in ["ABC", "A😀Ω"] {
        for repeat in [false, true] {
            let baseline = permutation(alphabet, repeat, "lexicographic", 42);
            let expected_edges = topology(&baseline);
            let expected_colours: BTreeMap<_, _> = baseline
                .nodes
                .iter()
                .map(|node| (node.id.clone(), node.color))
                .collect();
            for order in ["reverse", "shuffle", "branch-walk"] {
                let graph = permutation(alphabet, repeat, order, 137);
                assert_eq!(graph.nodes.len(), baseline.nodes.len());
                assert_eq!(topology(&graph), expected_edges);
                for node in &graph.nodes {
                    assert_eq!(node.color, expected_colours[&node.id]);
                    let first = node.id.chars().next().unwrap().to_string();
                    assert_eq!(node.color, expected_colours[&first]);
                }
            }
        }
    }
}

#[test]
fn alphabet_palette_does_not_repeat_the_old_five_colour_cycle() {
    let graph = permutation("ABCDEF", false, "branch-walk", 42);
    let unique: BTreeSet<_> = graph
        .nodes
        .iter()
        .filter(|node| node.id.len() == 1)
        .map(|node| node.color.map(f32::to_bits))
        .collect();
    assert_eq!(unique.len(), 6);
    assert!(graph.edges.iter().all(|edge| edge.gradient));
}

#[test]
fn oversized_permutations_are_rejected_before_building_a_factorial_plan() {
    let error = compile_source(
        PERMUTATIONS,
        json!({"alphabet":"ABCDEFGHIJKL", "maxLength":10}),
        42,
    )
    .unwrap_err();
    assert!(error.contains("8192"), "{error}");
    assert!(!error.contains("interrupted"), "{error}");
}

#[test]
fn shipped_examples_use_the_new_strength_defaults_without_changing_dense_counts() {
    for (source, counts) in [
        (include_str!("../examples/starter.js"), (8, 7)),
        (include_str!("../examples/ring.js"), (80, 80)),
        (
            include_str!("../examples/complete-growth.js"),
            (500, 124_750),
        ),
    ] {
        let plan = compile_source(source, json!({}), 42).unwrap();
        assert!(plan
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Batch { edges, .. } => Some(edges),
                _ => None,
            })
            .flatten()
            .all(|edge| edge.strength == crate::model::DEFAULT_EDGE_STRENGTH));
        assert_eq!((plan.node_count, plan.edge_count), counts);
    }
}
