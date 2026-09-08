use super::*;
use git_ai::model::jj_observation::{
    MAX_JJ_OBSERVATION_BATCH_BYTES, MAX_JJ_OBSERVATION_OPERATION_BYTES,
    MAX_JJ_OBSERVATION_OPERATIONS,
};

pub fn run(case: &Case, config: &Config) {
    let (journal, saved) = install(case, config);
    let kind = case.name.rsplit(':').next().unwrap();
    for over in [false, true] {
        let count = MAX_JJ_OBSERVATION_OPERATIONS + usize::from(over);
        let records = match kind {
            "pairs" => native::chain(count),
            "pairs_with_baseline" => native::chain(count - 1),
            "raw" => native::raw_chain(over),
            "raw_with_baseline" => native::raw_with_baseline(over),
            "predecessors" => native::predecessor_records(over),
            "views" => native::view_records(over),
            other => panic!("unknown boundary fixture {other}"),
        };
        let baseline_head = kind.ends_with("with_baseline");
        let head = &records.last().unwrap().operation_id;
        let heads = if baseline_head {
            vec![MERGE_ID, head]
        } else {
            vec![head.as_str()]
        };
        if kind.starts_with("pairs") {
            assert_eq!(
                records.len() + usize::from(baseline_head),
                256 + usize::from(over)
            );
        }
        if kind.starts_with("raw") {
            let raw = native::raw_size(&records)
                + if baseline_head {
                    native::raw_size(&[merge()])
                } else {
                    0
                };
            assert_eq!(raw, MAX_JJ_OBSERVATION_BATCH_BYTES + usize::from(over));
            assert_eq!(records.len() + usize::from(baseline_head), 256);
        }
        if kind == "predecessors" || kind == "views" {
            let mut semantic = 0usize;
            for record in &records {
                let proof = verify_evidence(JJ_OBSERVATION_READER_PROFILE, record).unwrap();
                semantic += if kind == "views" {
                    proof.view().commit_references
                } else {
                    proof
                        .operation()
                        .commit_predecessors
                        .as_ref()
                        .unwrap()
                        .values()
                        .map(|edges| 1 + edges.len())
                        .sum::<usize>()
                };
            }
            assert_eq!(semantic, 4096 + usize::from(over));
            if kind == "views" {
                assert_eq!(records[0].view_id, records[1].view_id);
            }
        }
        write_records(case, &records);
        case.heads(&heads);
        // MINIMAL descendants need not contain this workspace: its checkout
        // remains the registered MERGE and is verified against that own view.
        assert_eq!(
            capture_current_state(&case.context(), deadline())
                .unwrap()
                .checkout()
                .operation_id,
            MERGE_ID
        );
        if over {
            let error = rejected(case, &journal, config);
            assert!(
                error.to_string().to_ascii_lowercase().contains("limit"),
                "{error}"
            );
        } else {
            let result = collect(case, &journal, config);
            assert_result(&result, &saved, &heads, &records, &[MERGE_ID], false);
        }
    }
}

pub fn envelope(case: &Case, config: &Config) {
    let (journal, _) = install(case, config);
    let head = native::late_branch();
    write_records(case, &[first(), left(), head.clone()]);
    case.heads(&[&head.operation_id]);
    assert_ne!(head.view_id, MINIMAL_ID);
    for over in [false, true] {
        let mut demanded = left();
        demanded.view_bytes = vec![
            0;
            MAX_JJ_OBSERVATION_OPERATION_BYTES
                - demanded.operation_bytes.len()
                + usize::from(over)
        ];
        assert_eq!(
            native::raw_size(&[demanded.clone()]),
            MAX_JJ_OBSERVATION_OPERATION_BYTES + usize::from(over)
        );
        fs::write(view_path(case, MINIMAL_ID), &demanded.view_bytes).unwrap();
        capture_current_state(&case.context(), deadline()).unwrap();
        let error = rejected(case, &journal, config);
        if over {
            assert!(
                error.to_string().to_ascii_lowercase().contains("limit"),
                "{error}"
            );
        } else {
            let native_error = match verify_evidence(JJ_OBSERVATION_READER_PROFILE, &demanded) {
                Ok(_) => panic!("malformed envelope unexpectedly decoded"),
                Err(error) => error,
            };
            assert!(
                error.to_string().contains(&native_error.to_string()),
                "exact envelope did not reach native validation: {error}"
            );
        }
    }
}
