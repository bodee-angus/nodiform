//! Cross-layer checks of the shipped experiments and their final graph semantics.
use crate::model::{Event, Graph};
use crate::rules::compile_source;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const PERMUTATIONS: &str = include_str!("../examples/abc-permutations.js");
const PREVIOUS_HALF: &str = include_str!("../examples/half-neighbourhood.js");
const PRIME_FACTORS: &str = include_str!("../examples/prime-factors.js");
const PI_DIGIT_CHAIN: &str = include_str!("../examples/pi-digit-chain.js");
const DIVISOR_GRAPH: &str = include_str!("../examples/divisor-graph.js");

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
        let expected_radii: BTreeMap<_, _> = baseline
            .nodes
            .iter()
            .map(|node| (&node.id, node.radius))
            .collect();
        let mut group_colours = BTreeMap::new();
        for node in &graph.nodes {
            assert_eq!(node.radius, expected_radii[&node.id]);
            let first = node.id.chars().next().unwrap();
            assert_eq!(
                node.color,
                *group_colours.entry(first).or_insert(node.color)
            );
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
            for order in ["reverse", "shuffle", "branch-walk", "connected-branch-walk"] {
                let graph = permutation(alphabet, repeat, order, 137);
                assert_eq!(graph.nodes.len(), baseline.nodes.len());
                assert_eq!(topology(&graph), expected_edges);
                let mut group_colours = BTreeMap::new();
                for node in &graph.nodes {
                    let first = node.id.chars().next().unwrap();
                    assert_eq!(
                        node.color,
                        *group_colours.entry(first).or_insert(node.color)
                    );
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

fn imported_example_graph(plan: &crate::model::Plan) -> Graph {
    let mut graph = Graph::new(42);
    for event in &plan.events {
        graph.apply(event).unwrap();
    }
    graph
}

fn imported_example_palette(count: usize) -> Vec<[f32; 4]> {
    let plan = compile_source(
        "function* generate(N, p) { yield N.batch(N.palette(p.count).map((color, i) => N.node(String(i), {color}))); }",
        json!({"count": count}),
        42,
    )
    .unwrap();
    imported_example_graph(&plan)
        .nodes
        .into_iter()
        .map(|node| node.color)
        .collect()
}

#[test]
fn imported_examples_default_controls_compile_without_parameters() {
    for (source, nodes, edges, ticks) in [
        (PRIME_FACTORS, 500, 1_412, 3_240),
        (PI_DIGIT_CHAIN, 30, 39, 720),
        (DIVISOR_GRAPH, 20, 46, 720),
    ] {
        let plan = compile_source(source, json!({}), 42).unwrap();
        assert_eq!(
            (plan.node_count, plan.edge_count, plan.total_ticks),
            (nodes, edges, ticks)
        );
        assert_eq!(plan.events.last(), Some(&Event::Wait { ticks: 240 }));
        let graph = imported_example_graph(&plan);
        assert_eq!((graph.nodes.len(), graph.edges.len()), (nodes, edges));
    }
}

#[test]
fn prime_factor_births_scale_edges_by_exponent_and_use_vivid_categories() {
    let plan = compile_source(
        PRIME_FACTORS,
        json!({"count":16, "ticksPerNode":7, "finalTicks":11}),
        42,
    )
    .unwrap();
    assert_eq!(plan.total_ticks, 16 * 7 + 11);
    let (last, births) = plan.events.split_last().unwrap();
    assert_eq!(*last, Event::Wait { ticks: 11 });
    let (pairs, remainder) = births.as_chunks::<2>();
    assert!(remainder.is_empty());
    assert_eq!(pairs.len(), 16);
    let colours = imported_example_palette(3);
    assert_eq!(
        colours
            .iter()
            .map(|c| c.map(f32::to_bits))
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );
    let primes = [2, 3, 5, 7, 11, 13];
    let mut seen = BTreeSet::new();
    for (index, pair) in pairs.iter().enumerate() {
        let number = index + 1;
        let Event::Batch { nodes, edges } = &pair[0] else {
            panic!("A birth must include its factor connections");
        };
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, number.to_string());
        assert_eq!(nodes[0].label.as_deref(), Some(nodes[0].id.as_str()));
        assert_eq!(nodes[0].radius, 1.6);
        let category = if number == 1 {
            0
        } else if primes.contains(&number) {
            1
        } else {
            2
        };
        assert_eq!(
            crate::model::parse_color(&nodes[0].color).unwrap(),
            colours[category]
        );
        for edge in edges {
            let target: usize = edge.target.parse().unwrap();
            assert_eq!(edge.source, nodes[0].id);
            assert!(target < number);
            assert!(target == 1 || primes.contains(&target));
            assert!(
                seen.insert((number, target)),
                "Each distinct factor gets one edge"
            );
            assert!(edge.gradient);
            assert_eq!(edge.color, "#FFFFFFCC");
        }
        assert_eq!(pair[1], Event::Wait { ticks: 7 });
    }
    let graph = imported_example_graph(&plan);
    for (number, expected) in [
        ("12", BTreeMap::from([("1", 4.0), ("2", 8.0), ("3", 4.0)])),
        ("15", BTreeMap::from([("1", 4.0), ("3", 4.0), ("5", 4.0)])),
        ("16", BTreeMap::from([("1", 4.0), ("2", 16.0)])),
    ] {
        let actual: BTreeMap<_, _> = graph
            .edges
            .iter()
            .filter(|edge| graph.nodes[edge.source].id == number)
            .map(|edge| (graph.nodes[edge.target].id.as_str(), edge.strength))
            .collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn prime_factor_anchor_is_optional_and_base_strength_remains_configurable() {
    let plan = compile_source(
        PRIME_FACTORS,
        json!({"count":16, "connectToOne":false, "strength":2, "ticksPerNode":0, "finalTicks":0}),
        42,
    )
    .unwrap();
    let graph = imported_example_graph(&plan);
    assert_eq!(plan.total_ticks, 0);
    assert!(graph
        .edges
        .iter()
        .all(|edge| graph.nodes[edge.target].id != "1"));
    let sixteen: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| graph.nodes[edge.source].id == "16")
        .map(|edge| (graph.nodes[edge.target].id.as_str(), edge.strength))
        .collect();
    assert_eq!(sixteen, [("2", 8.0)]);
    for event in &plan.events {
        if let Event::Batch { nodes, edges } = event {
            if ["1", "2", "3", "5", "7", "11", "13"].contains(&nodes[0].id.as_str()) {
                assert!(edges.is_empty());
            }
        }
    }
    for anchor in [false, true] {
        let single =
            compile_source(PRIME_FACTORS, json!({"count":1, "connectToOne":anchor}), 42).unwrap();
        assert_eq!(
            (single.node_count, single.edge_count, single.total_ticks),
            (1, 0, 246)
        );
    }
}

#[test]
fn pi_digit_occurrences_follow_pi_and_connect_only_to_their_hub_and_predecessor() {
    let plan = compile_source(PI_DIGIT_CHAIN, json!({}), 42).unwrap();
    let expected = [
        "3-1", "1-1", "4-1", "1-2", "5-1", "9-1", "2-1", "6-1", "5-2", "3-2", "5-3", "8-1", "9-2",
        "7-1", "9-3", "3-3", "2-2", "3-4", "8-2", "4-2",
    ];
    let Event::Batch { nodes: hubs, edges } = &plan.events[0] else {
        panic!("All ten digit hubs must exist before the sequence starts");
    };
    assert!(edges.is_empty());
    assert_eq!(hubs.len(), 10);
    let colours = imported_example_palette(10);
    for (digit, hub) in hubs.iter().enumerate() {
        assert_eq!(hub.id, digit.to_string());
        assert_eq!(hub.label.as_deref(), Some(hub.id.as_str()));
        assert_eq!(hub.radius, 2.4);
        assert_eq!(
            crate::model::parse_color(&hub.color).unwrap(),
            colours[digit]
        );
    }
    let (pairs, remainder) = plan.events[1..plan.events.len() - 1].as_chunks::<2>();
    assert!(remainder.is_empty());
    assert_eq!(pairs.len(), expected.len());
    for (index, pair) in pairs.iter().enumerate() {
        let Event::Batch { nodes, edges } = &pair[0] else {
            panic!("Each occurrence must arrive with both available connections");
        };
        assert_eq!(nodes.len(), 1);
        let node = &nodes[0];
        assert_eq!(node.id, expected[index]);
        assert_eq!(node.label.as_deref(), Some(expected[index]));
        assert_eq!(node.radius, 1.6);
        let digit = node.id.as_bytes()[0] - b'0';
        assert_eq!(node.color, hubs[usize::from(digit)].color);
        let actual: BTreeSet<_> = edges.iter().map(|edge| edge.target.as_str()).collect();
        let mut targets = BTreeSet::from([hubs[usize::from(digit)].id.as_str()]);
        if index > 0 {
            targets.insert(expected[index - 1]);
        }
        assert_eq!(actual, targets);
        assert_eq!(edges.len(), targets.len());
        assert!(edges
            .iter()
            .all(|edge| edge.source == node.id && edge.gradient && edge.strength == 4.0));
        assert_eq!(pair[1], Event::Wait { ticks: 24 });
    }
    imported_example_graph(&plan);
}

#[test]
fn pi_uses_native_bigints_beyond_float_precision_and_preserves_configured_strengths() {
    use sha2::{Digest, Sha256};
    let plan = compile_source(
        PI_DIGIT_CHAIN,
        json!({"digits":1000, "ticksPerDigit":0, "hubStrength":2, "chainStrength":7, "finalTicks":11}),
        42,
    )
    .unwrap();
    assert_eq!(
        (plan.node_count, plan.edge_count, plan.total_ticks),
        (1_010, 1_999, 11)
    );
    let graph = imported_example_graph(&plan);
    let digits: String = graph
        .nodes
        .iter()
        .skip(10)
        .map(|node| node.id.chars().next().unwrap())
        .collect();
    assert!(digits.starts_with("3141592653589793238462643383279502884197169399375105820974944592307816406286208998628034825342117067"));
    // Independent reference: integer Machin formula, pi = 16 atan(1/5) - 4 atan(1/239),
    // calculated with 20 guard digits. Hash covers all 1,000 digits, including the 3.
    assert_eq!(
        format!("{:x}", Sha256::digest(digits.as_bytes())),
        "2f77ba99f311974f0d188c0b19710260c11c70d6f4d96d78570d4a59c3b0dbe0"
    );
    let hub_edges: Vec<_> = graph.edges.iter().filter(|edge| edge.target < 10).collect();
    let chain_edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.target >= 10)
        .collect();
    assert_eq!((hub_edges.len(), chain_edges.len()), (1_000, 999));
    assert!(hub_edges.iter().all(|edge| edge.strength == 2.0));
    assert!(chain_edges.iter().all(|edge| edge.strength == 7.0));
}

#[test]
fn divisor_graph_has_every_proper_divisor_without_duplicate_square_factors() {
    let plan = compile_source(DIVISOR_GRAPH, json!({}), 42).unwrap();
    let graph = imported_example_graph(&plan);
    let expected: BTreeSet<_> = (1..=20)
        .flat_map(|number| {
            (1..number)
                .filter(move |divisor| number % divisor == 0)
                .map(move |divisor| (number.to_string(), divisor.to_string()))
        })
        .collect();
    assert_eq!(topology(&graph), expected);
    assert_eq!(graph.edges.len(), expected.len());
    for (number, divisors) in [
        ("8", vec!["1", "2", "4"]),
        ("12", vec!["1", "2", "3", "4", "6"]),
        ("16", vec!["1", "2", "4", "8"]),
        ("20", vec!["1", "2", "4", "5", "10"]),
    ] {
        let actual: BTreeSet<_> = graph
            .edges
            .iter()
            .filter(|edge| graph.nodes[edge.source].id == number)
            .map(|edge| graph.nodes[edge.target].id.as_str())
            .collect();
        assert_eq!(actual, BTreeSet::from_iter(divisors));
    }
    assert!(graph
        .edges
        .iter()
        .all(|edge| edge.gradient && edge.strength == 4.0));
    let (pairs, remainder) = plan.events[..plan.events.len() - 1].as_chunks::<2>();
    assert!(remainder.is_empty());
    for (index, pair) in pairs.iter().enumerate() {
        let Event::Batch { nodes, edges } = &pair[0] else {
            panic!("Expected atomic birth");
        };
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, (index + 1).to_string());
        assert_eq!(nodes[0].label.as_deref(), Some(nodes[0].id.as_str()));
        assert!(edges.iter().all(|edge| edge.source == nodes[0].id));
        assert_eq!(pair[1], Event::Wait { ticks: 24 });
    }
}

#[test]
fn divisor_colours_use_the_requested_palette_size_in_rainbow_order() {
    for count in [1, 20, 73] {
        let plan = compile_source(
            DIVISOR_GRAPH,
            json!({"count":count, "ticksPerNode":3, "finalTicks":11, "strength":2}),
            42,
        )
        .unwrap();
        let graph = imported_example_graph(&plan);
        assert_eq!(plan.total_ticks, count as u64 * 3 + 11);
        assert_eq!(graph.nodes.len(), count);
        assert!(graph.edges.iter().all(|edge| edge.strength == 2.0));
        let expected: BTreeSet<_> = imported_example_palette(count)
            .iter()
            .map(|c| c.map(f32::to_bits))
            .collect();
        let actual: BTreeSet<_> = graph
            .nodes
            .iter()
            .map(|node| node.color.map(f32::to_bits))
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), count);
        // The angle in the RGB chromaticity plane independently preserves hue order.
        let hues: Vec<_> = graph
            .nodes
            .iter()
            .map(|node| {
                let [r, g, b, _] = node.color.map(f64::from);
                (3.0_f64.sqrt() * (g - b))
                    .atan2(2.0 * r - g - b)
                    .to_degrees()
                    .rem_euclid(360.0)
            })
            .collect();
        assert!(hues.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
