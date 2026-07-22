use super::attestation::build_attestations_from_attributions;
use super::types::VirtualAttributions;
use crate::model::attribution_tracker::{Attribution, LineAttribution};
use crate::model::authorship_log::{HumanRecord, SessionRecord};
use crate::model::working_log::AgentId;
use crate::operations::git::test_utils::TmpRepo;
use std::collections::{BTreeMap, HashMap};

#[test]
fn test_authorship_log_with_metadata_flattens_and_copies_metadata() {
    let temp_repo = TmpRepo::new().expect("create test repository");
    let repo = temp_repo.gitai_repo().clone();
    let mut va = VirtualAttributions::new(repo, "base".into(), HashMap::new(), HashMap::new(), 0);
    let session = SessionRecord {
        agent_id: AgentId {
            tool: "codex".into(),
            id: "first".into(),
            model: "gpt-5".into(),
        },
        human_author: None,
        custom_attributes: None,
    };
    let first = session.to_prompt_record();
    let mut later = first.clone();
    later.agent_id.id = "later".into();
    let prompts = BTreeMap::from([("z".into(), later), ("a".into(), first)]);
    va.prompts.insert("p".into(), prompts);
    let human = HumanRecord {
        author: "Human".into(),
    };
    va.humans.insert("h".into(), human);
    va.sessions.insert("s".into(), session);

    let metadata = va.authorship_log_with_metadata().metadata;
    assert_eq!(metadata.base_commit_sha, "base");
    assert_eq!(metadata.prompts["p"].agent_id.id, "first");
    assert_eq!(metadata.humans["h"].author, "Human");
    assert_eq!(metadata.sessions["s"].agent_id.id, "first");
}

/// Regression (#11): the attestation emit order must be deterministic.
/// `attributions` is a HashMap and per-file entries are grouped in a
/// HashMap<author_id, ...>, so naive iteration emits files and entries in a
/// process-randomised order, making byte-identical commits produce
/// different note bytes. build_attestations_from_attributions must sort
/// files by path and entries by hash.
#[test]
fn test_build_attestations_is_deterministically_sorted() {
    // Many files + many authors per file so that, were the order taken from
    // HashMap iteration, it would be astronomically unlikely to already be
    // sorted at both levels.
    let mut attributions: HashMap<String, (Vec<Attribution>, Vec<LineAttribution>)> =
        HashMap::new();
    let files = [
        "zeta.rs", "mid.rs", "alpha.rs", "beta.rs", "yarn.rs", "delta.rs", "gamma.rs", "omega.rs",
    ];
    let authors = [
        "s_zzz", "h_aaa", "s_mmm", "h_qqq", "s_bbb", "h_ttt", "s_ddd",
    ];
    for (fi, file) in files.iter().enumerate() {
        let mut line_attrs = Vec::new();
        for (ai, author) in authors.iter().enumerate() {
            let line = (fi * authors.len() + ai + 1) as u32;
            line_attrs.push(LineAttribution::new(line, line, author.to_string(), None));
        }
        attributions.insert(file.to_string(), (Vec::new(), line_attrs));
    }

    let result = build_attestations_from_attributions(&attributions);

    // Files are sorted by path.
    let got_files: Vec<&str> = result.iter().map(|f| f.file_path.as_str()).collect();
    let mut want_files = got_files.clone();
    want_files.sort_unstable();
    assert_eq!(got_files, want_files, "files must be sorted by path");

    // Entries within each file are sorted by hash.
    for fa in &result {
        let got: Vec<&str> = fa.entries.iter().map(|e| e.hash.as_str()).collect();
        let mut want = got.clone();
        want.sort_unstable();
        assert_eq!(
            got, want,
            "entries in {} must be sorted by hash",
            fa.file_path
        );
    }

    // And the whole thing is stable across repeated builds.
    let again = build_attestations_from_attributions(&attributions);
    assert_eq!(result, again, "output must be stable across builds");
}
