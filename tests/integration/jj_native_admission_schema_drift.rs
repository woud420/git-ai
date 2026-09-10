use super::*;

fn reject_admission(schema: &str) {
    reject_unchanged(&manual_v4(schema, STATES));
}
fn reject_state(schema: &str) {
    reject_unchanged(&manual_v4(ADMISSIONS, schema));
}

#[test]
fn jj_admission_schema_missing_tables_are_not_repaired_at_v4() {
    for (admissions, states) in [("", STATES), (ADMISSIONS, "")] {
        reject_unchanged(&manual_v4(admissions, states));
    }
}

#[test]
fn jj_admission_schema_column_drift_is_rejected_without_rewriting_old_records() {
    for (from, to) in [
        ("generation INTEGER NOT NULL", "generation TEXT NOT NULL"),
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
        (
            "checksum TEXT NOT NULL,",
            "checksum TEXT NOT NULL, extra TEXT,",
        ),
        ("record BLOB NOT NULL,", ""),
    ] {
        reject_admission(&altered(ADMISSIONS, from, to));
    }
    for (from, to) in [
        ("state BLOB NOT NULL", "state TEXT NOT NULL"),
        ("checksum TEXT NOT NULL", "checksum TEXT"),
        (
            "checksum TEXT NOT NULL",
            "checksum TEXT NOT NULL DEFAULT 'fixture'",
        ),
        (
            "state BLOB NOT NULL",
            "state BLOB GENERATED ALWAYS AS (X'00') STORED NOT NULL",
        ),
        (
            "checksum TEXT NOT NULL,",
            "checksum TEXT NOT NULL, extra TEXT,",
        ),
        ("checksum TEXT NOT NULL,", ""),
    ] {
        reject_state(&altered(STATES, from, to));
    }
}

#[test]
fn jj_admission_schema_primary_key_drift_is_rejected() {
    for replacement in [
        "PRIMARY KEY (admission_id, source_id)",
        "PRIMARY KEY (source_id)",
        "PRIMARY KEY (source_id COLLATE NOCASE, admission_id)",
        "PRIMARY KEY (source_id, admission_id DESC)",
    ] {
        reject_admission(&altered(
            ADMISSIONS,
            "PRIMARY KEY (source_id, admission_id)",
            replacement,
        ));
    }
    reject_admission(&altered(
        ADMISSIONS,
        ",\n    PRIMARY KEY (source_id, admission_id)",
        "",
    ));
    reject_state(&altered(
        STATES,
        "source_id TEXT PRIMARY KEY NOT NULL",
        "source_id TEXT NOT NULL",
    ));
    for replacement in [
        "source_id TEXT COLLATE NOCASE PRIMARY KEY NOT NULL",
        "source_id TEXT PRIMARY KEY DESC NOT NULL",
    ] {
        reject_state(&altered(
            STATES,
            "source_id TEXT PRIMARY KEY NOT NULL",
            replacement,
        ));
    }
}

#[test]
fn jj_admission_schema_unique_index_keys_and_order_are_exact() {
    for replacement in [
        "UNIQUE (generation, source_id)",
        "UNIQUE (source_id)",
        "UNIQUE (source_id, generation, admission_id)",
        "UNIQUE (source_id COLLATE NOCASE, generation)",
        "UNIQUE (source_id, generation DESC)",
    ] {
        reject_admission(&altered(
            ADMISSIONS,
            "UNIQUE (source_id, generation)",
            replacement,
        ));
    }
    reject_admission(&altered(
        ADMISSIONS,
        ",\n    UNIQUE (source_id, generation)",
        "",
    ));
}

#[test]
fn jj_admission_schema_explicit_index_cannot_be_partial_nonunique_or_expression() {
    let explicit = explicit_generation_index();
    for (from, to) in [
        ("CREATE UNIQUE INDEX", "CREATE INDEX"),
        (
            "(source_id, generation);",
            "(source_id, generation) WHERE generation>0;",
        ),
        (
            "(source_id, generation);",
            "(source_id, generation) WHERE 1;",
        ),
        (
            "(source_id, generation);",
            "(lower(source_id), generation);",
        ),
        (
            "(source_id, generation);",
            "(source_id, generation COLLATE NOCASE);",
        ),
    ] {
        reject_admission(&altered(&explicit, from, to));
    }
}

#[test]
fn jj_admission_schema_extra_indexes_are_not_part_of_the_contract() {
    for (table, column) in [
        ("jj_native_admissions", "record"),
        ("jj_native_admission_states", "state"),
    ] {
        for unique in ["", "UNIQUE "] {
            let fixture = manual_v4(ADMISSIONS, STATES);
            let conn = open_with_memory_limits(&fixture.path).unwrap();
            conn.execute_batch(&format!("CREATE {unique}INDEX extra ON {table}({column});"))
                .unwrap();
            reject_unchanged(&fixture);
        }
    }
}

#[test]
fn jj_admission_schema_foreign_key_targets_order_and_actions_are_exact() {
    for replacement in [
        "FOREIGN KEY (source_id) REFERENCES jj_sources(source_id)",
        "FOREIGN KEY (source_id) REFERENCES jj_native_registrations(source_root_key)",
        "FOREIGN KEY (source_id) REFERENCES jj_native_registrations(source_id) ON DELETE CASCADE",
        "FOREIGN KEY (source_id) REFERENCES jj_native_registrations(source_id) ON UPDATE CASCADE",
    ] {
        reject_admission(&altered(
            ADMISSIONS,
            "FOREIGN KEY (source_id) REFERENCES jj_native_registrations(source_id)",
            replacement,
        ));
    }
    reject_admission(&altered(
        ADMISSIONS,
        ",\n    FOREIGN KEY (source_id) REFERENCES jj_native_registrations(source_id)",
        "",
    ));
    for replacement in [
        "REFERENCES jj_native_admissions(admission_id, source_id)",
        "REFERENCES jj_native_baselines(source_id, baseline_id)",
        "REFERENCES jj_native_admissions(source_id, admission_id) ON DELETE CASCADE",
        "REFERENCES jj_native_admissions(source_id, admission_id) ON UPDATE CASCADE",
    ] {
        reject_state(&altered(
            STATES,
            "REFERENCES jj_native_admissions(source_id, admission_id)",
            replacement,
        ));
    }
    reject_state(&altered(
        STATES,
        ",\n    FOREIGN KEY (source_id, admission_id)\n        REFERENCES jj_native_admissions(source_id, admission_id)",
        "",
    ));
    reject_state(&altered(
        STATES,
        "FOREIGN KEY (source_id, admission_id)",
        "FOREIGN KEY (admission_id, source_id)",
    ));
    reject_state(&altered(
        STATES,
        "\n);",
        ",\n    FOREIGN KEY (source_id) REFERENCES jj_sources(source_id)\n);",
    ));
}

#[test]
fn jj_admission_schema_binary_primary_index_must_bind_its_foreign_key() {
    let admissions = altered(
        ADMISSIONS,
        "admission_id TEXT NOT NULL",
        "admission_id TEXT COLLATE NOCASE NOT NULL",
    );
    let admissions = altered(
        &admissions,
        "PRIMARY KEY (source_id, admission_id)",
        "PRIMARY KEY (source_id, admission_id COLLATE BINARY)",
    );
    let fixture = manual_v4(&admissions, STATES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    // Every qualified pragma shape still matches; SQLite must bind the FK too.
    shape(&conn);
    let before = full_snapshot(&conn);
    let error = match conn.prepare(
        "INSERT INTO jj_native_admission_states(source_id,admission_id,state,checksum) VALUES (?1,?2,?3,?4)"
    ) {
        Ok(_) => panic!("parent collation fixture unexpectedly bound its foreign key"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("foreign key mismatch"));
    assert_eq!(full_snapshot(&conn), before);
    reject_unchanged(&fixture);
}
