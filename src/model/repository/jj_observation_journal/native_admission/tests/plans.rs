use super::super::read::{GENERATION_IDS_SQL, HIGHER_GENERATION_SQL};
use super::*;
use rusqlite::params;

#[test]
fn native_admission_model_actual_generation_probes_use_the_qualified_source_generation_index() {
    for default_nocase in [false, true] {
        let fixture = Fixture::new();
        let conn = fixture.conn();
        if default_nocase {
            conn.pragma_update(None, "foreign_keys", false).unwrap();
            conn.execute_batch("DROP TABLE jj_native_admissions;
              CREATE TABLE jj_native_admissions (
                source_id TEXT NOT NULL,admission_id TEXT NOT NULL,
                generation INTEGER COLLATE NOCASE NOT NULL,record BLOB NOT NULL,checksum TEXT NOT NULL,
                PRIMARY KEY(source_id,admission_id),UNIQUE(source_id,generation COLLATE BINARY),
                FOREIGN KEY(source_id) REFERENCES jj_native_registrations(source_id)
              );").unwrap();
            // This is an accepted schema shape, not a damaged-schema fallback.
            drop(fixture.journal());
        }
        for (sql, predicate) in [
            (GENERATION_IDS_SQL, "generation=?"),
            (HIGHER_GENERATION_SQL, "generation>?"),
        ] {
            let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
            let details = statement
                .query_map(params![vectors::SOURCE, 1], |row| row.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(details.len(), 1, "{details:?}");
            let plan = &details[0];
            assert!(
                plan.starts_with("SEARCH jj_native_admissions USING "),
                "{plan}"
            );
            assert!(
                plan.contains("INDEX ") && plan.contains("source_id=?") && plan.contains(predicate),
                "{plan}"
            );
            assert!(
                !plan.contains("SCAN") && !plan.contains("AUTOMATIC"),
                "{plan}"
            );
        }
    }
}

#[test]
fn native_admission_model_actual_generation_probes_keep_two_and_one_row_ceilings() {
    let fixture = Fixture::new();
    let conn = fixture.conn();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    conn.execute_batch("DROP TABLE jj_native_admissions;
      CREATE TABLE jj_native_admissions(source_id TEXT NOT NULL,admission_id TEXT NOT NULL,generation INTEGER NOT NULL,record BLOB NOT NULL,checksum TEXT NOT NULL,PRIMARY KEY(source_id,admission_id));
      CREATE INDEX damaged_admission_generation ON jj_native_admissions(source_id,generation);").unwrap();
    for (index, generation) in [1, 1, 1, 2, 3, 4].into_iter().enumerate() {
        conn.execute(
            "INSERT INTO jj_native_admissions VALUES (?1,?2,?3,X'01','fixture')",
            params![vectors::SOURCE, format!("{index:064x}"), generation],
        )
        .unwrap();
    }
    for (sql, expected) in [(GENERATION_IDS_SQL, 2), (HIGHER_GENERATION_SQL, 1)] {
        let mut statement = conn.prepare(sql).unwrap();
        let selected = statement
            .query_map(params![vectors::SOURCE, 1], |_| Ok(()))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(selected.len(), expected);
        assert!(
            statement
                .query_map(params!["ab".repeat(32), 1], |_| Ok(()))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .is_empty()
        );
    }
}
