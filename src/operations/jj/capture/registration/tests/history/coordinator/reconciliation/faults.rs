use super::*;

#[test]
fn reconciliation_private_deadline_at_each_unchanged_phase_releases_without_progress() {
    for with_prior in [false, true] {
        for (index, at) in UNCHANGED_PHASES.into_iter().enumerate() {
            let (case, mut journal, expected, _) = setup(with_prior);
            let sql = sql_snapshot(&case.db, true);
            let files = filesystem(&case.fixture);
            let mut hooks = Hook::new(|phase| {
                if phase == AdmissionPhase::UnchangedSnapshotVerified {
                    writer_reservation(&case.db, true);
                }
            });
            hooks.expires = Some(at);
            let error = refuse(reconcile(&case, &mut journal, &expected, &mut hooks));
            assert!(error.to_string().contains("deadline"), "{error}");
            assert_eq!(hooks.phases, UNCHANGED_PHASES[..=index]);
            assert!(sql_snapshot(&case.db, true) == sql);
            assert!(filesystem(&case.fixture) == files);
            writer_reservation(&case.db, false);
        }
    }
}

fn final_fault(mutate: impl Fn(&Case)) {
    let (case, mut journal, expected, _) = setup(true);
    let sql = sql_snapshot(&case.db, true);
    let mut injected_files = None;
    let mut hooks = Hook::new(|phase| {
        if phase == AdmissionPhase::UnchangedSnapshotVerified {
            writer_reservation(&case.db, true);
            mutate(&case);
            injected_files = Some(filesystem(&case.fixture));
        }
    });
    refuse(reconcile(&case, &mut journal, &expected, &mut hooks));
    assert_eq!(hooks.phases, UNCHANGED_PHASES);
    drop(hooks);
    assert!(sql_snapshot(&case.db, true) == sql);
    assert!(filesystem(&case.fixture) == injected_files.unwrap());
    writer_reservation(&case.db, false);
}

#[test]
fn reconciliation_private_final_seal_or_ancestor_replacement_refuses_with_prior_intact() {
    final_fault(|case| {
        let seal = case.fixture.repo.join("git-ai/registration");
        let bytes = fs::read(&seal).unwrap();
        let old = fs::metadata(&seal).unwrap().ino();
        fs::rename(&seal, seal.with_extension("retained")).unwrap();
        fs::write(&seal, bytes).unwrap();
        fs::set_permissions(&seal, fs::Permissions::from_mode(0o600)).unwrap();
        assert_ne!(fs::metadata(&seal).unwrap().ino(), old);
    });
    final_fault(|case| {
        let path = &case.fixture.ancestor;
        fs::rename(path, path.with_extension("retained")).unwrap();
        fs::create_dir(path).unwrap();
    });
}

#[test]
fn reconciliation_private_final_metadata_changes_refuse_with_prior_intact() {
    for (relative, replacement) in [
        ("store/type", "future_git"),
        ("op_store/type", "future_op_store"),
        ("op_heads/type", "future_op_heads"),
        ("store/git_target", "../../../.git/."),
    ] {
        final_fault(|case| fs::write(case.fixture.repo.join(relative), replacement).unwrap());
    }
    final_fault(|case| {
        fs::write(
            case.fixture.root.join(".git/HEAD"),
            b"ref: refs/heads/changed\n",
        )
        .unwrap();
    });
}
