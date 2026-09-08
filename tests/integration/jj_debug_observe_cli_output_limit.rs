use super::*;

#[path = "jj_head_closure_output_fixture.rs"]
mod fixture;

#[test]
fn jj_head_closures_cli_maximum_lineage_and_escaped_workspace_fit_bounded_output() {
    observe_case("head_closure_output", true);
}

pub(super) fn run(case: &Case, config: &Config) {
    let workspace = fixture::workspace();
    let anchors = fixture::records(true);
    let heads = fixture::records(false);
    write_records(case, &anchors);
    case.heads(&fixture::ANCHORS);
    checkout(case, fixture::ANCHORS[0], &workspace);
    let (journal, registered) = install(case, config);
    assert_eq!(registered.workspace_name(), workspace);
    assert_eq!(
        registered.baseline().receipt().captured_head_ids().len(),
        32
    );
    let zero = status(case, &journal, config);
    let remembered = Remembered::from_status(&zero);
    write_records(case, &heads);
    case.heads(&fixture::HEADS);
    checkout(case, fixture::HEADS[0], &workspace);
    let before = state(case);
    for disposition in ["admitted", "already_admitted"] {
        let mut process = observer(case, &finite_args(&case.journal_path, &remembered, 1, 250));
        let value = process.next();
        assert!(value.get("error").is_none(), "{value}");
        process.finish(true);
        let size = serde_json::to_vec(&value).unwrap().len() + 1;
        assert!(
            size > 240 * 1024,
            "large valid native result was not exercised"
        );
        assert!(size <= 256 * 1024);
        assert_eq!(value["workspace"]["name"], workspace);
        assert_closures(&value);
        let current = status(case, &journal, config);
        let packet = next_packet(case, &journal, registered.source_id(), &value);
        assert_eq!(value, changed_json(&packet, &current, 1, disposition));
        assert_eq!(current.cursor().generation(), 1);
        assert!(state(case) == before);
    }
    let current = status(case, &journal, config);
    let id = current.latest_receipt().unwrap().admission_id();
    let value = super::super::support::checked(
        case,
        command(
            case,
            &receipt_args(&case.journal_path, registered.source_id(), id),
            &case.root,
        ),
        None,
    );
    assert_closures(&value);
    let size = serde_json::to_vec(&value).unwrap().len() + 1;
    assert!(size > 128 * 1024 && size <= 256 * 1024);
}

fn checkout(case: &Case, id: &str, workspace: &str) {
    fs::write(
        case.root.join(".jj/working_copy/checkout"),
        crate::jj_capture::support::checkout_bytes(id, workspace),
    )
    .unwrap();
}

fn assert_closures(value: &Json) {
    let mut anchors = fixture::ANCHORS.to_vec();
    anchors.sort();
    let mut heads = fixture::HEADS.to_vec();
    heads.sort();
    let expected: Vec<_> = heads
        .into_iter()
        .map(|head| {
            json!({
                "head_id": head, "reached_baseline_ids": anchors, "reaches_root": false
            })
        })
        .collect();
    assert_eq!(value["admission"]["head_closures"], json!(expected));
    assert_eq!(value["admission"]["reached_baseline_ids"], json!(anchors));
    assert_eq!(value["admission"]["operation_count"], 32);
    assert_eq!(value["admission"]["reaches_root"], false);
}
