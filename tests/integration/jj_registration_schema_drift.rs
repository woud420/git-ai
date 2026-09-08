use super::*;

fn altered(schema: &str, from: &str, to: &str) -> String {
    assert_eq!(schema.matches(from).count(), 1, "ambiguous fixture edit");
    schema.replacen(from, to, 1)
}

fn reject_registration(schema: &str) {
    assert_rejected_unchanged(&manual_v3(schema, WORKSPACES));
}

fn reject_workspace(schema: &str) {
    assert_rejected_unchanged(&manual_v3(REGISTRATIONS, schema));
}

fn explicit_registration_index() -> String {
    let table = altered(
        REGISTRATIONS,
        "source_root_key TEXT NOT NULL UNIQUE",
        "source_root_key TEXT NOT NULL",
    );
    format!(
        "{table}\nCREATE UNIQUE INDEX registration_root_guard
         ON jj_native_registrations(source_root_key);"
    )
}

fn explicit_workspace_indexes() -> String {
    let table = altered(WORKSPACES, ",\n    UNIQUE (locator_key)", "");
    let table = altered(&table, ",\n    UNIQUE (source_id, workspace_root_key)", "");
    format!(
        "{table}\nCREATE UNIQUE INDEX workspace_locator
         ON jj_native_workspaces(locator_key);
         CREATE UNIQUE INDEX workspace_root_guard
         ON jj_native_workspaces(source_id, workspace_root_key);"
    )
}

#[test]
fn jj_registration_schema_equivalent_explicit_unique_indexes_are_supported() {
    let fixture = manual_v3(
        &explicit_registration_index(),
        &explicit_workspace_indexes(),
    );
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let before = complete_snapshot(&conn);
    let registration_indexes = index_shapes(&conn, "jj_native_registrations");
    let workspace_indexes = index_shapes(&conn, "jj_native_workspaces");
    assert_eq!(
        registration_indexes
            .iter()
            .filter(|shape| shape.origin == "c")
            .count(),
        1
    );
    assert_eq!(
        workspace_indexes
            .iter()
            .filter(|shape| shape.origin == "c")
            .count(),
        2
    );
    drop(fixture.open());
    assert_eq!(complete_snapshot(&conn), before);
}

#[test]
fn jj_registration_schema_missing_new_tables_are_not_repaired_at_v3() {
    for (registrations, workspaces) in [("", WORKSPACES), (REGISTRATIONS, "")] {
        assert_rejected_unchanged(&manual_v3(registrations, workspaces));
    }
}

#[test]
fn jj_registration_schema_column_drift_rejects_without_changing_old_records() {
    for (from, to) in [
        ("record BLOB NOT NULL", "record TEXT NOT NULL"),
        ("checksum TEXT NOT NULL", "checksum TEXT"),
        (
            "checksum TEXT NOT NULL",
            "checksum TEXT NOT NULL DEFAULT 'fixture'",
        ),
        (
            "checksum TEXT NOT NULL",
            "checksum TEXT GENERATED ALWAYS AS ('fixture') STORED NOT NULL",
        ),
        ("    checksum TEXT NOT NULL,\n", ""),
        (
            "checksum TEXT NOT NULL,",
            "checksum TEXT NOT NULL,\n    unexpected TEXT NOT NULL,",
        ),
        (
            "record BLOB NOT NULL,\n    checksum TEXT NOT NULL",
            "checksum TEXT NOT NULL,\n    record BLOB NOT NULL",
        ),
    ] {
        reject_registration(&altered(REGISTRATIONS, from, to));
        reject_workspace(&altered(WORKSPACES, from, to));
    }
}

#[test]
fn jj_registration_schema_primary_key_membership_and_order_are_required() {
    reject_registration(&altered(
        REGISTRATIONS,
        "source_id TEXT PRIMARY KEY NOT NULL",
        "source_id TEXT NOT NULL",
    ));
    for replacement in [
        "",
        "    PRIMARY KEY (source_id),\n",
        "    PRIMARY KEY (workspace_name, source_id),\n",
    ] {
        reject_workspace(&altered(
            WORKSPACES,
            "    PRIMARY KEY (source_id, workspace_name),\n",
            replacement,
        ));
    }
}

#[test]
fn jj_registration_schema_each_required_unique_guard_is_present() {
    reject_registration(&altered(
        REGISTRATIONS,
        "source_root_key TEXT NOT NULL UNIQUE",
        "source_root_key TEXT NOT NULL",
    ));
    for constraint in [
        ",\n    UNIQUE (locator_key)",
        ",\n    UNIQUE (source_id, workspace_root_key)",
    ] {
        reject_workspace(&altered(WORKSPACES, constraint, ""));
    }
}

#[test]
fn jj_registration_schema_unique_index_replacements_require_full_unique_keys() {
    let registrations = explicit_registration_index();
    for (from, to) in [
        ("CREATE UNIQUE INDEX", "CREATE INDEX"),
        (
            "ON jj_native_registrations(source_root_key);",
            "ON jj_native_registrations(source_root_key) WHERE source_id != '';",
        ),
        (
            "ON jj_native_registrations(source_root_key);",
            "ON jj_native_registrations(source_root_key, baseline_id);",
        ),
        (
            "ON jj_native_registrations(source_root_key);",
            "ON jj_native_registrations(lower(source_root_key));",
        ),
    ] {
        reject_registration(&altered(&registrations, from, to));
    }

    let workspaces = explicit_workspace_indexes();
    for index_name in ["workspace_locator", "workspace_root_guard"] {
        reject_workspace(&altered(
            &workspaces,
            &format!("CREATE UNIQUE INDEX {index_name}"),
            &format!("CREATE INDEX {index_name}"),
        ));
    }
    for columns in ["locator_key", "source_id, workspace_root_key"] {
        reject_workspace(&altered(
            &workspaces,
            &format!("ON jj_native_workspaces({columns});"),
            &format!("ON jj_native_workspaces({columns}) WHERE workspace_name != '';"),
        ));
    }
}

#[test]
fn jj_registration_schema_key_order_and_direction_remain_exact() {
    reject_registration(&altered(
        &explicit_registration_index(),
        "(source_root_key);",
        "(source_root_key DESC);",
    ));
    let workspaces = explicit_workspace_indexes();
    for (from, to) in [
        ("(locator_key);", "(locator_key DESC);"),
        (
            "(source_id, workspace_root_key);",
            "(workspace_root_key, source_id);",
        ),
        (
            "(source_id, workspace_root_key);",
            "(source_id, workspace_root_key DESC);",
        ),
    ] {
        reject_workspace(&altered(&workspaces, from, to));
    }
}

#[test]
fn jj_registration_schema_primary_and_unique_keys_use_binary_collation() {
    reject_registration(&altered(
        REGISTRATIONS,
        "source_id TEXT PRIMARY KEY NOT NULL",
        "source_id TEXT COLLATE NOCASE PRIMARY KEY NOT NULL",
    ));
    reject_workspace(&altered(
        WORKSPACES,
        "workspace_name TEXT NOT NULL",
        "workspace_name TEXT COLLATE NOCASE NOT NULL",
    ));
    reject_registration(&altered(
        &explicit_registration_index(),
        "(source_root_key);",
        "(source_root_key COLLATE NOCASE);",
    ));
    let workspaces = explicit_workspace_indexes();
    for (from, to) in [
        ("(locator_key);", "(locator_key COLLATE NOCASE);"),
        (
            "(source_id, workspace_root_key);",
            "(source_id COLLATE NOCASE, workspace_root_key);",
        ),
        (
            "(source_id, workspace_root_key);",
            "(source_id, workspace_root_key COLLATE NOCASE);",
        ),
    ] {
        reject_workspace(&altered(&workspaces, from, to));
    }
}

#[test]
fn jj_registration_schema_extra_indexes_do_not_silently_change_constraints() {
    for extra in [
        "CREATE INDEX extra_registration ON jj_native_registrations(baseline_id);",
        "CREATE UNIQUE INDEX extra_registration ON jj_native_registrations(baseline_id);",
    ] {
        reject_registration(&format!("{REGISTRATIONS}\n{extra}"));
    }
    for extra in [
        "CREATE INDEX extra_workspace ON jj_native_workspaces(workspace_name);",
        "CREATE UNIQUE INDEX extra_workspace ON jj_native_workspaces(workspace_root_key);",
    ] {
        reject_workspace(&format!("{WORKSPACES}\n{extra}"));
    }
}

#[test]
fn jj_registration_schema_foreign_key_targets_order_actions_and_count_are_exact() {
    let registration_reference = "REFERENCES jj_native_baselines(source_id, baseline_id)";
    for replacement in [
        "REFERENCES jj_native_sources(source_id, baseline_id)",
        "REFERENCES jj_native_baselines(baseline_id, source_id)",
        "REFERENCES jj_native_baselines(source_id, baseline_id) ON UPDATE CASCADE",
        "REFERENCES jj_native_baselines(source_id, baseline_id) ON DELETE CASCADE",
    ] {
        reject_registration(&altered(REGISTRATIONS, registration_reference, replacement));
    }
    reject_registration(&altered(
        REGISTRATIONS,
        ",\n    FOREIGN KEY (source_id, baseline_id)\n        REFERENCES jj_native_baselines(source_id, baseline_id)",
        "",
    ));
    reject_registration(&altered(
        REGISTRATIONS,
        "FOREIGN KEY (source_id, baseline_id)",
        "FOREIGN KEY (baseline_id, source_id)",
    ));
    reject_registration(&altered(
        REGISTRATIONS,
        "\n);",
        ",\n    FOREIGN KEY (source_id) REFERENCES jj_sources(source_id)\n);",
    ));

    let workspace_reference = "REFERENCES jj_native_registrations(source_id)";
    for replacement in [
        "",
        "REFERENCES jj_native_sources(source_id)",
        "REFERENCES jj_native_registrations(source_root_key)",
        "REFERENCES jj_native_registrations(source_id) ON UPDATE CASCADE",
        "REFERENCES jj_native_registrations(source_id) ON DELETE CASCADE",
    ] {
        reject_workspace(&altered(WORKSPACES, workspace_reference, replacement));
    }
    reject_workspace(&altered(
        WORKSPACES,
        "\n);",
        ",\n    FOREIGN KEY (workspace_name) REFERENCES jj_native_registrations(source_root_key)\n);",
    ));
}

#[test]
fn jj_registration_schema_parent_column_collation_cannot_invalidate_a_binary_fk_index() {
    let registrations = altered(
        REGISTRATIONS,
        "source_id TEXT PRIMARY KEY NOT NULL",
        "source_id TEXT COLLATE NOCASE NOT NULL",
    );
    let registrations = altered(
        &registrations,
        "    FOREIGN KEY (source_id, baseline_id)",
        "    PRIMARY KEY (source_id COLLATE BINARY),\n    FOREIGN KEY (source_id, baseline_id)",
    );
    let fixture = manual_v3(&registrations, WORKSPACES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    assert_v3_shape(&conn);
    let before = complete_snapshot(&conn);
    let error = match conn.prepare(
        "INSERT INTO jj_native_workspaces
         (source_id, workspace_name, locator_key, workspace_root_key, record, checksum)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    ) {
        Ok(_) => panic!("parent-column collation fixture unexpectedly bound its foreign key"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("foreign key mismatch"));
    assert_eq!(complete_snapshot(&conn), before);
    assert_rejected_unchanged(&fixture);
}
