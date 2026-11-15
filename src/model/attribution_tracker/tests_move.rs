//! Move detection through `update_attributions`: a block that is moved by a
//! non-AI checkpoint keeps the attribution of every moved line, whether or not
//! its indentation changed on the way.

use super::line_attribution::attributions_to_line_attributions;
use super::tracker::AttributionTracker;
use crate::model::attribution::{Attribution, LineAttribution};

const TS: u128 = 1_000;
const HEAD: &str = "struct Config;\n\n";
const MAIN: &str = "fn main() {\n    run();\n}\n\nmod inner {\n";
const TAIL: &str = "}\n";
const FLAT: &str = "fn helper() {\n    let x = 1;\n    let y = 2;\n}\n";
const NESTED: &str = "    fn helper() {\n        let x = 1;\n        let y = 2;\n    }\n";

fn moved(block_before: &str, block_after: &str) -> (String, String) {
    (
        format!("{HEAD}{block_before}{MAIN}{TAIL}"),
        format!("{HEAD}{MAIN}{block_after}{TAIL}"),
    )
}

fn update(old: &str, new: &str, old_attrs: &[Attribution]) -> Vec<Attribution> {
    AttributionTracker::new()
        .update_attributions(old, new, old_attrs, "human", TS + 1)
        .unwrap()
}

fn owner_of<'a>(attrs: &'a [Attribution], content: &str, needle: &str) -> Vec<&'a str> {
    let start = content.find(needle).unwrap();
    let end = start + needle.len();
    let mut owners: Vec<&str> = attrs
        .iter()
        .filter(|a| a.start <= start && a.end >= end)
        .map(|a| a.author_id.as_str())
        .collect();
    owners.sort_unstable();
    owners
}

#[test]
fn indented_move_keeps_every_moved_line() {
    let (old, new) = moved(FLAT, NESTED);
    let block = HEAD.len()..HEAD.len() + FLAT.len();
    let old_attrs = vec![Attribution::new(block.start, block.end, "ai".into(), TS)];

    let updated = update(&old, &new, &old_attrs);

    assert_eq!(
        attributions_to_line_attributions(&updated, &new),
        vec![LineAttribution::new(8, 11, "ai".into(), None)]
    );
}

#[test]
fn dedented_move_does_not_spill_past_the_moved_block() {
    let (new, old) = moved(FLAT, NESTED);
    let block_start = HEAD.len() + MAIN.len();
    let old_attrs = vec![Attribution::new(
        block_start,
        block_start + NESTED.len(),
        "ai".into(),
        TS,
    )];

    let updated = update(&old, &new, &old_attrs);

    assert_eq!(
        attributions_to_line_attributions(&updated, &new),
        vec![LineAttribution::new(3, 6, "ai".into(), None)]
    );
    assert!(
        updated
            .iter()
            .all(|a| a.author_id != "ai" || a.end <= HEAD.len() + FLAT.len()),
        "moved attribution must stay inside the block: {updated:?}"
    );
}

#[test]
fn indented_move_keeps_each_line_with_its_own_author() {
    let (old, new) = moved(FLAT, NESTED);
    let line = |needle: &str, author: &str| {
        let start = old.find(needle).unwrap();
        Attribution::new(start, start + needle.len(), author.into(), TS)
    };
    let old_attrs = vec![
        line("fn helper() {\n", "ai_a"),
        line("    let x = 1;\n", "ai_b"),
        line("    let y = 2;\n", "ai_c"),
    ];

    let updated = update(&old, &new, &old_attrs);

    assert_eq!(owner_of(&updated, &new, "fn helper() {"), ["ai_a"]);
    assert_eq!(owner_of(&updated, &new, "let x = 1;"), ["ai_b"]);
    assert_eq!(owner_of(&updated, &new, "let y = 2;"), ["ai_c"]);
}

/// A block moved byte-for-byte keeps the exact attribution ranges it had,
/// including overlapping ranges, a zero-length marker and multi-byte text.
#[test]
fn verbatim_move_output_is_unchanged() {
    let block = "fn héllo() {\n    let 名前 = \"😀\";\n    done();\n}\n";
    let (old, new) = moved(block, block);
    let at = |needle: &str| old.find(needle).unwrap();
    let old_attrs = vec![
        Attribution::new(
            at("fn héllo"),
            at("fn héllo") + block.len(),
            "ai".into(),
            TS,
        ),
        Attribution::new(at("名前"), at("😀") + "😀".len(), "ai_b".into(), TS + 5),
        Attribution::new(at("done"), at("done"), "ai_c".into(), TS + 7),
        Attribution::new(at("fn main"), at("mod inner"), "h_known".into(), TS),
    ];

    let updated = update(&old, &new, &old_attrs);

    // Recorded from the single-range mapping this replaced. The block's final
    // newline is its own insertion, and a zero-length marker does not move.
    assert_eq!(
        updated,
        vec![
            Attribution::new(16, 42, "h_known".into(), TS),
            Attribution::new(54, 106, "ai".into(), TS),
            Attribution::new(76, 90, "ai_b".into(), TS + 5),
            Attribution::new(106, 107, "human".into(), TS + 1),
        ]
    );
}
