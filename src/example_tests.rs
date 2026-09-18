//! Cross-layer checks of the shipped experiments and their final graph semantics.
use crate::model::{Event, Graph};
use crate::rules::compile_source;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const PERMUTATIONS: &str = include_str!("../examples/abc-permutations.js");
const PREVIOUS_HALF: &str = include_str!("../examples/half-neighbourhood.js");

fn permutation(alphabet: &str, repeat: bool, order: &str, seed: u32) -> Graph {
    permutation_at_depth(alphabet, 3, repeat, order, seed)
}

fn permutation_at_depth(
    alphabet: &str,
    max_length: usize,
    repeat: bool,
    order: &str,
    seed: u32,
) -> Graph {
    let plan = compile_source(
        PERMUTATIONS,
        json!({"alphabet": alphabet, "maxLength": max_length, "repetitions": repeat,
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
fn connected_branch_walk_visits_parents_first_with_alternating_child_directions() {
    let graph = permutation("ABC", false, "connected-branch-walk", 42);
    let ids: Vec<_> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "A", "AB", "ABC", "AC", "ACB", "B", "BA", "BAC", "BC", "BCA", "C", "CA", "CAB", "CB",
            "CBA"
        ]
    );

    // A reversed subtree still visits its prefix first, but its children keep
    // their alternating traversal direction. Four letters expose this choice.
    let graph = permutation_at_depth("ABCD", 4, false, "connected-branch-walk", 42);
    let first_branch: Vec<_> = graph
        .nodes
        .iter()
        .take_while(|node| node.id.starts_with('A'))
        .map(|node| node.id.as_str())
        .collect();
    assert_eq!(
        first_branch,
        [
            "A", "AB", "ABC", "ABCD", "ABD", "ABDC", "AC", "ACD", "ACDB", "ACB", "ACBD", "AD",
            "ADB", "ADBC", "ADC", "ADCB"
        ]
    );
}

#[test]
fn connected_branch_walk_keeps_every_birth_attached_without_changing_the_graph() {
    for (alphabet, repeat, depth) in [
        ("AB", false, 2),
        ("AB", true, 4),
        ("A😀Ω", false, 3),
        ("A😀Ω", true, 4),
        ("ABCD", false, 4),
        ("ABCD", true, 4),
    ] {
        let plan = compile_source(
            PERMUTATIONS,
            json!({"alphabet":alphabet, "maxLength":depth, "repetitions":repeat,
                   "order":"connected-branch-walk", "ticksPerNode":7, "finalTicks":11}),
            42,
        )
        .unwrap();
        assert_eq!(plan.total_ticks, plan.node_count as u64 * 7 + 11);
        let (final_wait, births) = plan.events.split_last().unwrap();
        assert_eq!(*final_wait, Event::Wait { ticks: 11 });
        let (pairs, remainder) = births.as_chunks::<2>();
        assert!(remainder.is_empty());
        assert_eq!(pairs.len(), plan.node_count);

        let mut born = BTreeSet::new();
        let mut connections = BTreeSet::new();
        let mut graph = Graph::new(42);
        for pair in pairs {
            let Event::Batch { nodes, edges } = &pair[0] else {
                panic!("Each birth must contain its node and available connections");
            };
            assert_eq!(nodes.len(), 1);
            let id = &nodes[0].id;
            if !born.is_empty() {
                assert!(
                    edges.iter().any(|edge| {
                        (&edge.source == id && born.contains(&edge.target))
                            || (&edge.target == id && born.contains(&edge.source))
                    }),
                    "{alphabet:?}, repetitions={repeat}, depth={depth}: {id} was born disconnected"
                );
            }
            assert!(born.insert(id.clone()), "duplicate node {id}");
            for edge in edges {
                assert!(born.contains(&edge.source));
                assert!(born.contains(&edge.target));
                assert!(connections.insert((edge.source.clone(), edge.target.clone())));
                let letters: Vec<_> = edge.source.chars().collect();
                let prefix: String = letters[..letters.len() - 1].iter().collect();
                let suffix: String = letters[1..].iter().collect();
                assert!(edge.target == prefix || edge.target == suffix);
                assert!(edge.gradient);
                assert_eq!(edge.strength, 4.0);
                assert_eq!(edge.color, "#ffffffcc");
            }
            graph.apply(&pair[0]).unwrap();
            assert_eq!(pair[1], Event::Wait { ticks: 7 });
        }

        let baseline = permutation_at_depth(alphabet, depth, repeat, "lexicographic", 137);
        assert_eq!(born.len(), baseline.nodes.len());
        assert_eq!(connections.len(), baseline.edges.len());
        assert_eq!(topology(&graph), topology(&baseline));
        let expected_styles: BTreeMap<_, _> = baseline
            .nodes
            .iter()
            .map(|node| (&node.id, (node.color, node.radius)))
            .collect();
        for node in &graph.nodes {
            assert_eq!((node.color, node.radius), expected_styles[&node.id]);
        }
    }
}

#[test]
fn connected_branch_walk_does_not_invent_edges_between_single_letter_nodes() {
    for repeat in [false, true] {
        let graph = permutation_at_depth("A😀Ω", 1, repeat, "connected-branch-walk", 42);
        assert_eq!(
            graph
                .nodes
                .iter()
                .map(|node| node.id.as_str())
                .collect::<Vec<_>>(),
            ["A", "😀", "Ω"]
        );
        assert!(graph.edges.is_empty());
    }

    let single = permutation_at_depth("A", 4, false, "connected-branch-walk", 42);
    assert_eq!((single.nodes.len(), single.edges.len()), (1, 0));
    let repeated = permutation_at_depth("😀", 4, true, "connected-branch-walk", 42);
    assert_eq!((repeated.nodes.len(), repeated.edges.len()), (4, 3));
    assert_eq!(
        topology(&repeated),
        BTreeSet::from([
            ("😀😀".into(), "😀".into()),
            ("😀😀😀".into(), "😀😀".into()),
            ("😀😀😀😀".into(), "😀😀😀".into())
        ])
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
            for order in ["reverse", "shuffle", "branch-walk", "connected-branch-walk"] {
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
fn permutations_can_exceed_the_previous_node_and_depth_limits() {
    let plan = compile_source(
        PERMUTATIONS,
        json!({"alphabet":"AB", "maxLength":13, "repetitions":true,
               "order":"connected-branch-walk", "ticksPerNode":0}),
        42,
    )
    .unwrap();
    assert_eq!(plan.node_count, 16_382);
    assert_eq!(plan.edge_count, 32_736);
    assert_eq!(plan.total_ticks, 0);
}

#[test]
fn nonrepeating_permutations_stop_at_the_available_alphabet() {
    // No empty layers are traversed above the available three letters.
    for order in [
        "lexicographic",
        "reverse",
        "shuffle",
        "branch-walk",
        "connected-branch-walk",
    ] {
        let plan = compile_source(
            PERMUTATIONS,
            json!({"alphabet":"ABC", "maxLength":9_007_199_254_740_991_u64, "order":order}),
            42,
        )
        .unwrap();
        assert_eq!((plan.node_count, plan.edge_count), (15, 24));
    }
}

#[test]
fn shipped_count_inputs_accept_more_than_the_old_node_limit() {
    for (source, edges) in [
        (include_str!("../examples/ring.js"), 8_193),
        (include_str!("../examples/modular-residues.js"), 32_756),
    ] {
        let plan = compile_source(source, json!({"count":8193, "ticksPerNode":0}), 42).unwrap();
        assert_eq!((plan.node_count, plan.edge_count), (8_193, edges));
    }
}

#[test]
fn complete_growth_can_exceed_its_previous_example_limit() {
    let plan = compile_source(
        include_str!("../examples/complete-growth.js"),
        json!({"count":710, "interval":0}),
        42,
    )
    .unwrap();
    assert_eq!((plan.node_count, plan.edge_count), (710, 251_695));
}

#[test]
fn previous_half_example_matches_the_requested_first_six_births() {
    let plan = compile_source(PREVIOUS_HALF, json!({"count":6, "interval":7}), 42).unwrap();
    assert_eq!(
        (plan.node_count, plan.edge_count, plan.total_ticks),
        (6, 9, 42)
    );
    let expected: [&[&str]; 6] = [
        &[],
        &["1"],
        &["2"],
        &["3", "2"],
        &["4", "3"],
        &["5", "4", "3"],
    ];
    let (pairs, remainder) = plan.events.as_chunks::<2>();
    let mut events = pairs.iter();
    for (index, targets) in expected.iter().enumerate() {
        let pair = events.next().unwrap();
        let Event::Batch { nodes, edges } = &pair[0] else {
            panic!("Each birth must start with its node and edges");
        };
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, (index + 1).to_string());
        assert!(edges.iter().all(|edge| edge.source == nodes[0].id));
        assert_eq!(
            edges
                .iter()
                .map(|edge| edge.target.as_str())
                .collect::<Vec<_>>(),
            *targets
        );
        assert!(matches!(pair[1], Event::Wait { ticks: 7 }));
    }
    assert!(events.next().is_none());
    assert!(remainder.is_empty());
}

#[test]
fn previous_half_default_has_only_unique_recent_predecessor_edges() {
    let plan = compile_source(PREVIOUS_HALF, json!({}), 42).unwrap();
    assert_eq!((plan.node_count, plan.edge_count), (500, 62_500));
    assert_eq!(plan.total_ticks, 500 * 6);
    let mut seen = BTreeSet::new();
    let mut waits = 0;
    let mut births = 0;
    for event in &plan.events {
        match event {
            Event::Batch { nodes, edges } => {
                births += 1;
                assert_eq!(nodes.len(), 1);
                assert_eq!(nodes[0].id, births.to_string());
                assert_eq!(edges.len(), births / 2);
                for edge in edges {
                    let source: usize = edge.source.parse().unwrap();
                    let target: usize = edge.target.parse().unwrap();
                    assert_eq!(source, births);
                    assert!(target < source && target >= source - source / 2);
                    assert!(seen.insert((source, target)), "duplicate connection");
                    assert!(edge.gradient);
                    assert_eq!(edge.strength, crate::model::DEFAULT_EDGE_STRENGTH);
                }
            }
            Event::Wait { ticks } => {
                assert_eq!(*ticks, 6);
                waits += 1;
            }
            _ => panic!("Example should emit only births and waits"),
        }
    }
    assert_eq!((births, waits, seen.len()), (500, 500, 62_500));
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
