use super::{GraphOrder, TopologyNode, order_to_baseline};

mod support;
use support::{Fixture, root};

#[test]
fn topology_rejects_self_cycle() {
    let fixture = Fixture::new(&["A"], &[("A", &["A"])], &["T"]);
    fixture.rejects("cycle");
}

#[test]
fn topology_rejects_multi_node_cycle() {
    let fixture = Fixture::new(
        &["A"],
        &[("A", &["B"]), ("B", &["C"]), ("C", &["A"])],
        &["T"],
    );
    fixture.rejects("cycle");
}

#[test]
fn topology_does_not_return_partial_success_after_a_terminal_then_cycle() {
    let fixture = Fixture::new(
        &["M"],
        &[("M", &["T", "A"]), ("A", &["B"]), ("B", &["A"])],
        &["T"],
    );
    fixture.rejects("cycle");
}

#[test]
fn topology_expands_diamond_shared_parent_once_and_preserves_parent_order() {
    let fixture = Fixture::new(
        &["M"],
        &[
            ("M", &["L", "R"]),
            ("R", &["A"]),
            ("L", &["A"]),
            ("A", &["T"]),
        ],
        &["T"],
    );
    let order = fixture.success(&["A", "L", "R", "M"], &["T"], false);
    assert_eq!(order.operation_indices, [3, 2, 1, 0]);

    let reversed = Fixture::new(
        &["M"],
        &[
            ("M", &["R", "L"]),
            ("R", &["A"]),
            ("L", &["A"]),
            ("A", &["T"]),
        ],
        &["T"],
    );
    reversed.success(&["A", "R", "L", "M"], &["T"], false);
}

#[test]
fn topology_root_sorting_makes_input_presentation_irrelevant() {
    let first = Fixture::new(
        &["R", "L"],
        &[("R", &["A"]), ("L", &["A"]), ("A", &["T"])],
        &["T"],
    );
    first.success(&["A", "L", "R"], &["T"], false);
    let second = Fixture::new(
        &["L", "R"],
        &[("A", &["T"]), ("L", &["A"]), ("R", &["A"])],
        &["T"],
    );
    second.success(&["A", "L", "R"], &["T"], false);
    assert_eq!(first.heads, ["R", "L"]);
}

#[test]
fn topology_rejects_missing_head_and_missing_merge_parent() {
    Fixture::new(&["A"], &[], &["T"]).rejects("missing");
    Fixture::new(&["M"], &[("M", &["T", "A"])], &["T"]).rejects("missing");
}

#[test]
fn topology_rejects_detached_record_even_when_it_is_independently_closed() {
    let fixture = Fixture::new(&["A"], &[("D", &["T"]), ("A", &["T"])], &["T"]);
    fixture.rejects("detached");
}

#[test]
fn topology_stops_at_exact_terminal_without_accepting_records_below_it() {
    let terminal_only = Fixture::new(&["H"], &[], &["H"]);
    terminal_only.success(&[], &["H"], false);

    // A caller cannot make B reachable by asserting that the stopped H once
    // named it. Baseline parents are deliberately absent from this helper API.
    let below_terminal = Fixture::new(&["H"], &[("B", &[&root()])], &["H"]);
    below_terminal.rejects("detached");
}

#[test]
fn topology_does_not_turn_unknown_historical_parent_into_a_baseline_terminal() {
    let missing = Fixture::new(&["D"], &[("D", &["B"])], &["H"]);
    missing.rejects("missing");

    let closed = Fixture::new(&["D"], &[("D", &["B"]), ("B", &[&root()])], &["H"]);
    closed.success(&["B", "D"], &[], true);
}

#[test]
fn topology_preserves_redundant_baseline_cut_and_reports_only_reached_terminals() {
    Fixture::new(&["B"], &[], &["A", "B"]).success(&[], &["B"], false);
    Fixture::new(&["B", "A"], &[], &["B", "A"]).success(&[], &["A", "B"], false);
}

#[test]
fn topology_reports_mixed_root_and_baseline_closure_exactly() {
    let fixture = Fixture::new(
        &["M"],
        &[("M", &["L", "R"]), ("R", &["F"]), ("F", &[&root()])],
        &["L"],
    );
    fixture.success(&["F", "R", "M"], &["L"], true);
}

#[test]
fn topology_accepts_full_bounded_chain_depth_with_parent_first_indices() {
    let mut fixture = Fixture {
        heads: vec!["n255".to_owned()],
        nodes: Vec::new(),
        baseline: vec!["T".to_owned()],
    };
    for index in (0..256).rev() {
        let parent = if index == 0 {
            "T".to_owned()
        } else {
            format!("n{:03}", index - 1)
        };
        fixture.nodes.push((format!("n{index:03}"), vec![parent]));
    }
    let expected: Vec<_> = (0..256).map(|index| format!("n{index:03}")).collect();
    let expected_refs: Vec<_> = expected.iter().map(String::as_str).collect();
    let order = fixture.success(&expected_refs, &["T"], false);
    assert_eq!(order.operation_indices, (0..256).rev().collect::<Vec<_>>());
}

#[test]
fn topology_enforces_fixed_node_head_cut_and_parent_limits() {
    let mut too_many_nodes = Fixture::new(&["n256"], &[], &["T"]);
    for index in (0..257).rev() {
        let parent = if index == 0 {
            "T".to_owned()
        } else {
            format!("n{:03}", index - 1)
        };
        too_many_nodes
            .nodes
            .push((format!("n{index:03}"), vec![parent]));
    }
    too_many_nodes.rejects("limit");

    let labels: Vec<_> = (0..33).map(|index| format!("h{index:02}")).collect();
    let mut heads = Fixture {
        heads: labels[..32].to_vec(),
        nodes: Vec::new(),
        baseline: vec!["T".to_owned()],
    };
    heads.nodes = heads
        .heads
        .iter()
        .map(|id| (id.clone(), vec!["T".to_owned()]))
        .collect();
    let expected: Vec<_> = heads.heads.iter().map(String::as_str).collect();
    heads.success(&expected, &["T"], false);
    heads.heads.push(labels[32].clone());
    heads.nodes.push((labels[32].clone(), vec!["T".to_owned()]));
    heads.rejects("limit");

    let mut cut = Fixture {
        heads: vec![labels[0].clone()],
        nodes: Vec::new(),
        baseline: labels[..32].to_vec(),
    };
    cut.success(&[], &[labels[0].as_str()], false);
    cut.baseline.push(labels[32].clone());
    cut.rejects("limit");

    let mut parents = Fixture {
        heads: vec!["A".to_owned()],
        nodes: vec![("A".to_owned(), labels[..31].to_vec())],
        baseline: labels[..32].to_vec(),
    };
    parents.nodes[0].1.push(root());
    let reached: Vec<_> = labels[..31].iter().map(String::as_str).collect();
    parents.success(&["A"], &reached, true);
    parents.nodes[0].1.push(labels[31].clone());
    parents.rejects("limit");
}

mod head_closures;
