use super::*;

pub fn before_open(case: &Case) {
    let kind = case.name.rsplit(':').next().unwrap();
    let missing = case.test_home.join("missing-cli-journal-parent");
    let path = missing.join("journal.sqlite");
    let code = match kind {
        "empty" => {
            // Invalid metadata would otherwise stop discovery before the journal is considered.
            fs::write(case.root.join(".jj/working_copy/type"), b"unsupported").unwrap();
            "collection_disabled"
        }
        "git" => {
            fs::rename(case.root.join(".jj"), case.root.join("saved-jj-layout")).unwrap();
            "not_jj_workspace"
        }
        "malformed" => {
            fs::write(case.root.join(".jj/working_copy/type"), b"unsupported").unwrap();
            "context_unavailable"
        }
        "missing" => "journal_unavailable",
        _ => unreachable!(),
    };
    no_creation(
        case,
        command(case, &status_args(&path), &case.root),
        code,
        &missing,
    );
    if kind == "missing" {
        let args = receipt_args(&path, &"0".repeat(64), &"0".repeat(64));
        no_creation(
            case,
            command(case, &args, &case.test_home),
            "journal_unavailable",
            &missing,
        );
    }
}

pub fn unregistered(case: &Case, _config: &Config) {
    let _journal = case.open();
    checked(
        case,
        command(case, &status_args(&case.journal_path), &case.root),
        Some("admission_unavailable"),
    );
    assert!(!case.namespace().exists());
}

pub fn policy(case: &Case) {
    checked(
        case,
        command(case, &status_args(&case.journal_path), &case.root),
        Some("admission_unavailable"),
    );
}

pub fn native(case: &Case, config: &Config) {
    // Reuse the already calibrated packet+state repair: all scalar hashes and joins
    // remain consistent, while the native operation bytes are invalid.
    super::super::storage::corrupt(case, config);
    let (source, id): (String, String) = case
        .sql()
        .query_row(
            "SELECT source_id,admission_id FROM jj_native_admission_states",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    checked(
        case,
        command(case, &status_args(&case.journal_path), &case.root),
        Some("admission_unavailable"),
    );
    let args = receipt_args(&case.journal_path, &source, &id);
    checked(
        case,
        command(case, &args, &case.test_home),
        Some("admission_unavailable"),
    );
}
