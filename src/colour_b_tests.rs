//! Example colour modes preserve graph semantics and assign new hues in order.
use crate::model::{Event, Graph, Plan};
use crate::rules::compile_source;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const MODULAR: &str = include_str!("../examples/modular-residues.js");
const PRIMES: &str = include_str!("../examples/prime-factors.js");
const PI: &str = include_str!("../examples/pi-digit-chain.js");
const DIVISORS: &str = include_str!("../examples/divisor-graph.js");
const TORUS: &str = include_str!("../examples/toroidal-grid.js");
const FIBONACCI: &str = include_str!("../examples/fibonacci-chain.js");

fn cases() -> Vec<(&'static str, Value, Vec<&'static str>)> {
    vec![
        (
            MODULAR,
            json!({"count": 24, "moduli": [7, 3, 5]}),
            vec!["residue", "birth-order", "parity"],
        ),
        (
            PRIMES,
            json!({"count": 32}),
            vec!["number-type", "birth-order", "factor-count"],
        ),
        (
            PI,
            json!({"digits": 24}),
            vec!["digit", "birth-order", "node-role"],
        ),
        (
            DIVISORS,
            json!({"count": 24}),
            vec!["birth-order", "parity", "number-type"],
        ),
        (
            TORUS,
            json!({"dimensions": 3, "range": 3}),
            vec![
                "first-coordinate",
                "last-coordinate",
                "coordinate-sum",
                "birth-order",
            ],
        ),
        (
            FIBONACCI,
            json!({"maxNumber": 34}),
            vec!["birth-order", "fibonacci-membership", "parity"],
        ),
    ]
}

fn graph(plan: &Plan) -> Graph {
    let mut graph = Graph::new(42);
    for event in &plan.events {
        graph.apply(event).unwrap();
    }
    graph
}

fn palette(count: usize) -> Vec<[f32; 4]> {
    let plan = compile_source(
        "function* generate(N, p) { yield N.batch(N.palette(p.count).map((color, i) => N.node(String(i), {color}))); }",
        json!({"count": count}), 42,
    ).unwrap();
    graph(&plan)
        .nodes
        .into_iter()
        .map(|node| node.color)
        .collect()
}

fn without_node_colours(mut plan: Plan) -> Plan {
    for event in &mut plan.events {
        if let Event::Batch { nodes, .. } = event {
            for node in nodes {
                node.color.clear();
            }
        }
    }
    plan
}

#[test]
fn every_colour_mode_preserves_birth_order_topology_strengths_and_waits() {
    for (source, parameters, modes) in cases() {
        let baseline =
            without_node_colours(compile_source(source, parameters.clone(), 42).unwrap());
        for mode in modes {
            let mut parameters = parameters.clone();
            parameters["colorBy"] = json!(mode);
            let actual = compile_source(source, parameters, 42).unwrap();
            graph(&actual); // Every edge still targets valid nodes at that event.
            assert_eq!(without_node_colours(actual), baseline, "{mode}");
        }
    }
}

#[test]
fn every_birth_order_mode_uses_the_full_palette_in_order_without_seed_variation() {
    for (source, mut parameters, _) in cases() {
        parameters["colorBy"] = json!("birth-order");
        let first = compile_source(source, parameters.clone(), 42).unwrap();
        let second = compile_source(source, parameters, 137).unwrap();
        let expected = palette(first.node_count);
        for plan in [&first, &second] {
            let colours: Vec<_> = graph(plan)
                .nodes
                .into_iter()
                .map(|node| node.color)
                .collect();
            assert_eq!(colours, expected);
        }
    }
}

#[test]
fn new_categories_receive_palette_colours_in_first_encounter_order() {
    for (source, parameters, modes) in cases() {
        for mode in modes {
            let mut parameters = parameters.clone();
            parameters["colorBy"] = json!(mode);
            let plan = compile_source(source, parameters, 42).unwrap();
            let mut seen = BTreeSet::new();
            let first_colours: Vec<_> = graph(&plan)
                .nodes
                .into_iter()
                .filter_map(|node| {
                    seen.insert(node.color.map(f32::to_bits))
                        .then_some(node.color)
                })
                .collect();
            assert_eq!(first_colours, palette(first_colours.len()), "{mode}");
        }
    }
}

#[test]
fn factor_count_and_number_type_modes_describe_the_underlying_arithmetic() {
    let plan = compile_source(PRIMES, json!({"count": 16, "colorBy": "factor-count"}), 42).unwrap();
    let colours = palette(5);
    let expected = [0, 1, 1, 2, 1, 2, 1, 3, 2, 2, 1, 3, 1, 2, 2, 4];
    for (node, category) in graph(&plan).nodes.iter().zip(expected) {
        assert_eq!(node.color, colours[category]);
    }
    for source in [PRIMES, DIVISORS] {
        let plan =
            compile_source(source, json!({"count": 16, "colorBy": "number-type"}), 42).unwrap();
        let colours = palette(3);
        let expected = [0, 1, 1, 2, 1, 2, 1, 2, 2, 2, 1, 2, 1, 2, 2, 2];
        for (node, category) in graph(&plan).nodes.iter().zip(expected) {
            assert_eq!(node.color, colours[category]);
        }
    }
}

#[test]
fn toroidal_colour_modes_reveal_coordinate_bands_and_diagonals() {
    let colours = palette(4);
    for mode in ["first-coordinate", "last-coordinate", "coordinate-sum"] {
        let plan = compile_source(
            TORUS,
            json!({"dimensions": 2, "range": 4, "colorBy": mode}),
            42,
        )
        .unwrap();
        for (index, node) in graph(&plan).nodes.iter().enumerate() {
            let first = index / 4;
            let last = index % 4;
            let category = match mode {
                "first-coordinate" => first,
                "last-coordinate" => last,
                _ => (first + last) % 4,
            };
            assert_eq!(node.color, colours[category]);
        }
    }
}

#[test]
fn pi_role_mode_distinguishes_all_hubs_from_all_occurrences() {
    let plan = compile_source(PI, json!({"digits": 20, "colorBy": "node-role"}), 42).unwrap();
    let colours = palette(2);
    for (index, node) in graph(&plan).nodes.iter().enumerate() {
        assert_eq!(node.color, colours[usize::from(index >= 10)]);
    }
}

#[test]
fn fibonacci_chain_adds_requested_chords_after_all_births_and_counts_both_wait_types() {
    let plan = compile_source(
        FIBONACCI,
        json!({
            "maxNumber": 20, "ticksPerNode": 7, "ticksPerEdge": 13,
            "finalTicks": 17, "strength": 2
        }),
        42,
    )
    .unwrap();
    assert_eq!(
        (plan.node_count, plan.edge_count, plan.total_ticks),
        (21, 23, 21 * 7 + 3 * 13 + 17)
    );
    let graph = graph(&plan);
    assert_eq!(
        graph
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>(),
        (0..=20).map(|n| n.to_string()).collect::<Vec<_>>()
    );
    let (births, remainder) = plan.events.split_at(42);
    for (index, pair) in births.chunks_exact(2).enumerate() {
        let Event::Batch { nodes, edges } = &pair[0] else {
            panic!("Expected birth");
        };
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, index.to_string());
        assert_eq!(edges.len(), usize::from(index > 0));
        if index > 0 {
            assert_eq!(edges[0].source, index.to_string());
            assert_eq!(edges[0].target, (index - 1).to_string());
        }
        assert_eq!(pair[1], Event::Wait { ticks: 7 });
    }
    for (pair, (source, target)) in
        remainder[..6]
            .chunks_exact(2)
            .zip([("3", "5"), ("5", "8"), ("8", "13")])
    {
        let Event::Batch { nodes, edges } = &pair[0] else {
            panic!("Expected extra connection");
        };
        assert!(nodes.is_empty());
        assert_eq!(edges.len(), 1);
        assert_eq!(
            (edges[0].source.as_str(), edges[0].target.as_str()),
            (source, target)
        );
        assert_eq!(pair[1], Event::Wait { ticks: 13 });
    }
    assert_eq!(remainder.last(), Some(&Event::Wait { ticks: 17 }));
    assert!(graph
        .edges
        .iter()
        .all(|edge| edge.gradient && edge.strength == 2.0));
}

#[test]
fn fibonacci_boundaries_omit_loops_and_duplicate_chain_edges() {
    for (maximum, extras) in [
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 0),
        (4, 0),
        (5, 1),
        (8, 2),
        (20, 3),
        (21, 4),
        (34, 5),
    ] {
        let plan = compile_source(
            FIBONACCI,
            json!({"maxNumber": maximum, "ticksPerNode": 0, "ticksPerEdge": 0, "finalTicks": 0}),
            42,
        )
        .unwrap();
        assert_eq!(
            (plan.node_count, plan.edge_count, plan.total_ticks),
            (maximum + 1, maximum + extras, 0)
        );
        let graph = graph(&plan);
        let unique: BTreeSet<_> = graph
            .edges
            .iter()
            .map(|edge| {
                assert_ne!(edge.source, edge.target);
                (edge.source.min(edge.target), edge.source.max(edge.target))
            })
            .collect();
        assert_eq!(unique.len(), graph.edges.len());
    }
    let plan = compile_source(
        FIBONACCI,
        json!({"maxNumber": 13, "colorBy": "fibonacci-membership"}),
        42,
    )
    .unwrap();
    let colours = palette(2);
    for (index, node) in graph(&plan).nodes.iter().enumerate() {
        assert_eq!(
            node.color,
            colours[usize::from(![0, 1, 2, 3, 5, 8, 13].contains(&index))]
        );
    }
}

#[test]
fn unknown_colour_modes_and_unrepresentable_fibonacci_counts_report_errors() {
    for (source, mut parameters, _) in cases() {
        parameters["colorBy"] = json!("unknown");
        let error = compile_source(source, parameters, 42).unwrap_err();
        assert!(error.contains("Colour by"), "{error}");
    }
    for maximum in [
        json!(-1),
        json!(1.5),
        json!(9_007_199_254_740_991_u64),
        json!(9_007_199_254_740_992_u64),
    ] {
        let error = compile_source(FIBONACCI, json!({"maxNumber": maximum}), 42).unwrap_err();
        assert!(error.contains("safe"), "{error}");
    }
}
