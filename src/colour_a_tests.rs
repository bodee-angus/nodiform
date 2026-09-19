//! Colour controls must preserve the graph and assign new groups in rainbow order.
use crate::model::{Graph, Plan};
use crate::rules::compile_source;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const STARTER: &str = include_str!("../examples/starter.js");
const COMPLETE: &str = include_str!("../examples/complete-growth.js");
const HALF: &str = include_str!("../examples/half-neighbourhood.js");
const RING: &str = include_str!("../examples/ring.js");
const PERMUTATIONS: &str = include_str!("../examples/abc-permutations.js");

fn compile(source: &str, parameters: Value) -> (Plan, Graph) {
    let plan = compile_source(source, parameters, 137).unwrap();
    let mut graph = Graph::new(137);
    for event in &plan.events {
        graph.apply(event).unwrap();
    }
    (plan, graph)
}

fn palette(count: usize) -> Vec<[f32; 4]> {
    compile(
        "function build(graph, p) { graph.palette(p.count).forEach((color, i) => graph.add(i, {color})); }",
        json!({"count":count}),
    )
    .1
    .nodes
    .into_iter()
    .map(|node| node.color)
    .collect()
}

fn unchanged_graph(reference: &(Plan, Graph), actual: &(Plan, Graph)) {
    assert_eq!(reference.0.total_ticks, actual.0.total_ticks);
    assert_eq!(reference.1.nodes.len(), actual.1.nodes.len());
    for (left, right) in reference.1.nodes.iter().zip(&actual.1.nodes) {
        assert_eq!(left.id, right.id);
        assert_eq!(left.radius, right.radius);
    }
    assert_eq!(reference.1.edges.len(), actual.1.edges.len());
    for (left, right) in reference.1.edges.iter().zip(&actual.1.edges) {
        assert_eq!((left.source, left.target), (right.source, right.target));
        assert_eq!(left.strength, right.strength);
        assert_eq!(left.gradient, right.gradient);
        assert_eq!(left.color, right.color);
    }
}

#[test]
fn sequential_examples_default_to_one_rainbow_in_birth_order() {
    for (source, count) in [(STARTER, 8), (COMPLETE, 17), (HALF, 17), (RING, 17)] {
        let (_, graph) = compile(source, json!({"count":count}));
        assert_eq!(graph.nodes.len(), count);
        assert_eq!(
            graph
                .nodes
                .iter()
                .map(|node| node.color)
                .collect::<Vec<_>>(),
            palette(count)
        );
    }
}

#[test]
fn sequential_colour_choices_preserve_births_connections_and_timing() {
    for (source, alternating) in [
        (STARTER, "alternating"),
        (COMPLETE, "parity"),
        (HALF, "parity"),
        (RING, "alternating"),
    ] {
        let reference = compile(source, json!({"count":17, "finalTicks":31}));
        for mode in [alternating, "single"] {
            let actual = compile(source, json!({"count":17, "finalTicks":31, "colorBy":mode}));
            unchanged_graph(&reference, &actual);
            let colors = palette(if mode == "single" { 1 } else { 2 });
            for (index, node) in actual.1.nodes.iter().enumerate() {
                assert_eq!(node.color, colors[index % colors.len()]);
            }
        }
    }

    let reference = compile(HALF, json!({"count":17}));
    let grouped = compile(HALF, json!({"count":17, "colorBy":"neighbour-count"}));
    unchanged_graph(&reference, &grouped);
    let colors = palette(9);
    for (index, node) in grouped.1.nodes.iter().enumerate() {
        assert_eq!(node.color, colors[(index + 1) / 2]);
    }
}

#[test]
fn permutation_categories_follow_first_encounter_for_every_birth_order() {
    // Emoji sorts differently from the user's alphabet order, exposing accidental
    // palette allocation by alphabet index instead of category encounter order.
    for order in [
        "lexicographic",
        "reverse",
        "shuffle",
        "branch-walk",
        "connected-branch-walk",
    ] {
        for repeat in [false, true] {
            let params = json!({"alphabet":"A😀Ω", "maxLength":3, "repetitions":repeat,
                "order":order, "ticksPerNode":7, "finalTicks":31});
            let reference = compile(PERMUTATIONS, params.clone());
            for mode in [
                "starting-letter",
                "ending-letter",
                "word-length",
                "birth-order",
                "single",
            ] {
                let mut parameters = params.clone();
                parameters["colorBy"] = json!(mode);
                let actual = compile(PERMUTATIONS, parameters);
                unchanged_graph(&reference, &actual);
                let count = match mode {
                    "birth-order" => actual.1.nodes.len(),
                    "single" => 1,
                    _ => 3,
                };
                let colors = palette(count);
                let mut groups = BTreeMap::new();
                for (index, node) in actual.1.nodes.iter().enumerate() {
                    let group = match mode {
                        "starting-letter" => node.id.chars().next().unwrap().to_string(),
                        "ending-letter" => node.id.chars().next_back().unwrap().to_string(),
                        "word-length" => node.id.chars().count().to_string(),
                        "birth-order" => index.to_string(),
                        _ => String::new(),
                    };
                    let next = groups.len();
                    let color_index = *groups.entry(group).or_insert(next);
                    assert_eq!(
                        node.color, colors[color_index],
                        "{order}, {mode}, {}",
                        node.id
                    );
                }
                assert_eq!(groups.len(), count);
            }
        }
    }
}

#[test]
fn short_examples_only_allocate_colours_for_existing_groups() {
    for (source, mode) in [
        (COMPLETE, "parity"),
        (HALF, "parity"),
        (HALF, "neighbour-count"),
        (RING, "alternating"),
    ] {
        let (_, graph) = compile(source, json!({"count":1, "colorBy":mode}));
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].color, palette(1)[0]);
    }
    for repeat in [false, true] {
        let (_, graph) = compile(
            PERMUTATIONS,
            json!({"alphabet":"😀", "maxLength":4, "repetitions":repeat, "colorBy":"birth-order"}),
        );
        assert_eq!(graph.nodes.len(), if repeat { 4 } else { 1 });
        assert_eq!(
            graph
                .nodes
                .iter()
                .map(|node| node.color)
                .collect::<Vec<_>>(),
            palette(graph.nodes.len())
        );
    }
}
