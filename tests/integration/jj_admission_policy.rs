use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Saved {
    source: String,
    initialization: String,
    baseline: String,
    heads: Vec<String>,
    admission: String,
}

pub fn install(case: &Case, config: &Config) {
    let context = case.context();
    let remote = if case.name.ends_with(":sentinel_install") {
        git_ai::diagnostic_sentinels::DEBUG_SELF_CHECK_REMOTE_URL
    } else {
        REMOTE
    };
    fs::write(
        context.git.common_dir.join("config"),
        format!("[remote \"admission-fixture\"]\nurl = {remote}\n"),
    )
    .unwrap();
    let (mut journal, saved, zero) = initial(case, config);
    let result = outcome(admit_now(case, &mut journal, config, &zero), false);
    let fixture = Saved {
        source: saved.source_id().to_owned(),
        initialization: saved.initialization_receipt_id().to_owned(),
        baseline: saved.baseline().receipt().baseline_id().to_owned(),
        heads: zero.admitted_head_ids().to_vec(),
        admission: result.admission().receipt().admission_id().to_owned(),
    };
    fs::write(
        case.test_home.join("admission-policy-saved.json"),
        serde_json::to_vec(&fixture).unwrap(),
    )
    .unwrap();
}

pub fn denied(case: &Case, config: &Config) {
    let saved: Saved = serde_json::from_slice(
        &fs::read(case.test_home.join("admission-policy-saved.json")).unwrap(),
    )
    .unwrap();
    let mut journal = case.open();
    let mut context = case.context();
    if case.name.ends_with(":canonical_denied") {
        fs::create_dir(case.root.join("allowed")).unwrap();
        context.workspace_root = case.root.join("allowed/..");
        assert_eq!(context.workspace_root.canonicalize().unwrap(), case.root);
    }
    capture_current_state(&context, deadline()).unwrap();
    let expected = NativeAdmissionExpectation {
        source_id: &saved.source,
        initialization_receipt_id: &saved.initialization,
        baseline_id: &saved.baseline,
        generation: 0,
        admitted_head_ids: &saved.heads,
    };
    failed(checked_status(
        case,
        &journal,
        &context,
        config,
        deadline(),
        &mut admission_budget(),
    ));
    failed(admit_checked(
        case,
        &mut journal,
        &context,
        config,
        expected,
        deadline(),
        &mut admission_budget(),
    ));
    // The historical SQL reader has no policy/current-source claim.
    assert_eq!(
        packet(case, &journal, &saved.source, &saved.admission)
            .receipt()
            .generation(),
        1
    );
}
