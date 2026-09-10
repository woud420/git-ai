use super::{Fixture, GraphOrder, root};
use crate::operations::jj::ancestry::JjHeadClosure;

fn closures(order: &GraphOrder, expected: &[(&str, &[&str], bool)]) {
    let actual: &[JjHeadClosure] = &order.head_closures;
    assert_eq!(actual.len(), expected.len());
    assert!(
        actual
            .windows(2)
            .all(|pair| pair[0].head_id() < pair[1].head_id())
    );
    for (closure, (head, reached, reaches_root)) in actual.iter().zip(expected) {
        assert_eq!(closure.head_id(), *head);
        let actual: Vec<_> = closure
            .reached_baseline_ids()
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(actual, *reached, "head {head}");
        assert_eq!(closure.reaches_root(), *reaches_root, "head {head}");
    }
}

#[test]
fn topology_head_closures_keep_distinct_anchor_and_root_components_separate() {
    let fixture = Fixture::new(
        &["Z", "M", "A"],
        &[("Z", &[&root()]), ("M", &["T2"]), ("A", &["T1"])],
        &["unused", "T2", "T1"],
    );
    let order = fixture.success(&["A", "M", "Z"], &["T1", "T2"], true);
    closures(
        &order,
        &[
            ("A", &["T1"], false),
            ("M", &["T2"], false),
            ("Z", &[], true),
        ],
    );
    assert_eq!(fixture.heads, ["Z", "M", "A"]);
}

#[test]
fn topology_head_closures_compose_shared_ancestors_and_redundant_heads() {
    let fixture = Fixture::new(
        &["R", "M", "L"],
        &[
            ("M", &["L", "R"]),
            ("R", &["S", &root()]),
            ("L", &["S", "T2"]),
            ("S", &["T1"]),
        ],
        &["T2", "T1"],
    );
    let order = fixture.success(&["S", "L", "R", "M"], &["T1", "T2"], true);
    assert_eq!(order.operation_indices, [3, 2, 1, 0]);
    closures(
        &order,
        &[
            ("L", &["T1", "T2"], false),
            ("M", &["T1", "T2"], true),
            ("R", &["T1"], true),
        ],
    );
}

#[test]
fn topology_head_closures_assign_each_direct_cutoff_only_its_own_terminal() {
    let fixture = Fixture::new(&["T2", "T1"], &[], &["T2", "unused", "T1"]);
    let order = fixture.success(&[], &["T1", "T2"], false);
    closures(&order, &[("T1", &["T1"], false), ("T2", &["T2"], false)]);

    let mixed = Fixture::new(&["T2", "A", "T1"], &[("A", &["T2"])], &["T2", "T1"]);
    let order = mixed.success(&["A"], &["T1", "T2"], false);
    closures(
        &order,
        &[
            ("A", &["T2"], false),
            ("T1", &["T1"], false),
            ("T2", &["T2"], false),
        ],
    );
}

#[test]
fn topology_head_closures_keep_root_distinct_after_all_32_anchor_bits() {
    let anchors: Vec<_> = (0..32).map(|index| format!("t{index:02}")).collect();
    let mut reversed = anchors.clone();
    reversed.reverse();
    let fixture = Fixture {
        heads: vec!["Z".to_owned(), "M".to_owned(), "A".to_owned()],
        nodes: vec![
            ("M".to_owned(), vec!["A".to_owned(), "Z".to_owned()]),
            ("Z".to_owned(), vec![root()]),
            ("A".to_owned(), reversed.clone()),
        ],
        baseline: reversed,
    };
    assert_eq!(fixture.nodes[2].1.len(), 32);
    let reached: Vec<_> = anchors.iter().map(String::as_str).collect();
    let order = fixture.success(&["A", "Z", "M"], &reached, true);
    closures(
        &order,
        &[
            ("A", &reached, false),
            ("M", &reached, true),
            ("Z", &[], true),
        ],
    );
}

#[test]
fn topology_head_closures_cover_32_distinct_heads_before_head_limit_refusal() {
    let heads: Vec<_> = (0..32).map(|index| format!("h{index:02}")).collect();
    let anchors: Vec<_> = (0..32).map(|index| format!("t{index:02}")).collect();
    let mut fixture = Fixture {
        heads: heads.iter().rev().cloned().collect(),
        nodes: heads
            .iter()
            .zip(&anchors)
            .rev()
            .map(|(head, anchor)| (head.clone(), vec![anchor.clone()]))
            .collect(),
        baseline: anchors.iter().rev().cloned().collect(),
    };
    let head_refs: Vec<_> = heads.iter().map(String::as_str).collect();
    let anchor_refs: Vec<_> = anchors.iter().map(String::as_str).collect();
    let order = fixture.success(&head_refs, &anchor_refs, false);
    assert_eq!(order.head_closures.len(), 32);
    for ((closure, head), anchor) in order.head_closures.iter().zip(&heads).zip(&anchors) {
        assert_eq!(closure.head_id(), head);
        assert_eq!(closure.reached_baseline_ids(), std::slice::from_ref(anchor));
        assert!(!closure.reaches_root());
    }
    fixture.heads.push("h32".to_owned());
    fixture
        .nodes
        .push(("h32".to_owned(), vec![anchors[0].clone()]));
    fixture.rejects("head or cutoff count");
}

#[test]
fn topology_head_closures_do_not_widen_the_cut_for_inaccessible_evidence() {
    let direct = Fixture::new(&["T"], &[], &["T"]);
    closures(&direct.success(&[], &["T"], false), &[("T", &["T"], false)]);
    Fixture::new(&["T"], &[("U", &[&root()])], &["T"]).rejects("detached");
    Fixture::new(&["A"], &[], &["T"]).rejects("missing");
    Fixture::new(&["A", "Z"], &[("A", &["T"]), ("Z", &["missing"])], &["T"]).rejects("missing");
    Fixture::new(&["A", "Z"], &[("A", &["T"]), ("Z", &["Z"])], &["T"]).rejects("cycle");
}
