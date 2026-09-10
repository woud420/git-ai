CREATE TABLE jj_native_admissions (
    source_id TEXT NOT NULL,
    admission_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, admission_id),
    UNIQUE (source_id, generation),
    FOREIGN KEY (source_id) REFERENCES jj_native_registrations(source_id)
);
CREATE TABLE jj_native_admission_states (
    source_id TEXT PRIMARY KEY NOT NULL,
    admission_id TEXT NOT NULL,
    state BLOB NOT NULL,
    checksum TEXT NOT NULL,
    FOREIGN KEY (source_id, admission_id)
        REFERENCES jj_native_admissions(source_id, admission_id)
);
