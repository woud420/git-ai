use super::*;
use std::os::unix::fs::symlink;

#[test]
fn jj_capture_constructible_locators_cannot_substitute_another_source() {
    for case in 0..10 {
        let fixture = Fixture::new(true);
        let foreign = fixture.root.join("foreign workspace");
        let foreign_repo = layout(&foreign, false).canonicalize().unwrap();
        write_evidence_to(&foreign_repo, &merge());
        fs::write(foreign_repo.join("op_heads/heads").join(MERGE_ID), []).unwrap();
        fs::write(
            foreign.join(".jj/working_copy/checkout"),
            checkout_bytes(MERGE_ID, "default"),
        )
        .unwrap();
        let foreign_context = discover(&foreign).unwrap();
        assert_eq!(
            checked_capture(&fixture, &foreign_context, deadline())
                .unwrap()
                .head_ids(),
            [MERGE_ID]
        );
        let mut context = fixture.context();
        match case {
            0 => context.schema_version = 999,
            1 => context.capability = "already_verified",
            2 => context.vcs = "git",
            3 => context.jj = None,
            4 => context.workspace_root = "relative-workspace".into(),
            5 => context.jj.as_mut().unwrap().repo_dir = foreign_repo,
            6 => context.jj.as_mut().unwrap().store_dir = foreign_repo.join("store"),
            7 => context.git.git_dir = foreign_repo.join("store/git"),
            8 => context.git.common_dir = foreign_repo.join("store/git"),
            9 => context.colocated = false,
            _ => unreachable!(),
        }
        rejected(&fixture, &context, None);
    }
}

#[test]
fn jj_capture_revalidates_every_backend_after_discovery() {
    for path in [
        ".jj/working_copy/type",
        ".jj/repo/store/type",
        ".jj/repo/op_store/type",
        ".jj/repo/op_heads/type",
    ] {
        for value in [b"future-backend".as_slice(), b"local\n".as_slice()] {
            let fixture = Fixture::new(true);
            let context = fixture.context();
            fs::write(fixture.root.join(path), value).unwrap();
            rejected(&fixture, &context, None);
        }
    }
}

#[test]
fn jj_capture_revalidates_bounded_pointers_and_git_directory_shapes() {
    for case in 0..11 {
        let fixture = Fixture::new(true);
        let context = fixture.context();
        let store = fixture.repo_dir.join("store");
        match case {
            0 => fs::write(store.join("git_target"), []).unwrap(),
            1 => fs::write(store.join("git_target"), "missing-target").unwrap(),
            2 => fs::write(store.join("git_target"), [0xff]).unwrap(),
            3 => fs::write(store.join("git_target"), vec![b'x'; 16 * 1024 + 1]).unwrap(),
            4 => {
                fs::remove_file(fixture.root.join(".git/HEAD")).unwrap();
                fs::create_dir(fixture.root.join(".git/HEAD")).unwrap();
            }
            5 => {
                fs::rename(
                    fixture.root.join(".git/objects"),
                    fixture.root.join("saved objects"),
                )
                .unwrap();
                fs::write(fixture.root.join(".git/objects"), "not a directory").unwrap();
            }
            6 => fs::write(fixture.root.join(".git/commondir"), "missing-common").unwrap(),
            7 => {
                fs::write(store.join("git_target"), "git-file").unwrap();
                fs::write(store.join("git-file"), "not-a-gitdir-pointer").unwrap();
            }
            8 => {
                fs::rename(&fixture.repo_dir, fixture.root.join("saved repository")).unwrap();
                fs::write(fixture.root.join(".jj/repo"), []).unwrap();
            }
            9 => {
                fs::rename(&fixture.repo_dir, fixture.root.join("saved repository")).unwrap();
                fs::write(fixture.root.join(".jj/repo"), vec![b'x'; 16 * 1024 + 1]).unwrap();
            }
            10 => {
                fs::write(
                    store.join("git_target"),
                    "missing-component/../../../../.git",
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        rejected(&fixture, &context, None);
    }
}

#[test]
fn jj_capture_refuses_symlink_components_in_bound_metadata_paths() {
    for path in [
        ".jj",
        ".jj/repo/op_store",
        ".jj/repo/op_heads/heads",
        ".git",
    ] {
        let fixture = Fixture::new(true);
        let context = fixture.context();
        let original = fixture.root.join(path);
        let saved = fixture.root.join("replaced directory");
        fs::rename(&original, &saved).unwrap();
        symlink(&saved, &original).unwrap();
        rejected(&fixture, &context, None);
    }
}

#[test]
fn jj_capture_rejects_empty_root_malformed_and_excess_raw_head_sets() {
    let cases = vec![
        Vec::<String>::new(),
        vec!["00".repeat(64)],
        vec!["abc".to_owned()],
        vec![MERGE_ID.to_uppercase()],
        vec!["g".repeat(128)],
        vec![MERGE_ID.to_owned(), "unexpected.tmp".to_owned()],
        (1..=33).map(|n| format!("{n:0128x}")).collect(),
    ];
    for names in cases {
        let fixture = Fixture::new(true);
        let context = fixture.context();
        let names: Vec<_> = names.iter().map(String::as_str).collect();
        fixture.set_heads(&names);
        rejected(&fixture, &context, None);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let fixture = Fixture::new(true);
        let context = fixture.context();
        fs::write(
            fixture
                .heads_dir()
                .join(std::ffi::OsStr::from_bytes(b"\xff")),
            [],
        )
        .unwrap();
        rejected(&fixture, &context, None);
    }
}

#[test]
fn jj_capture_head_markers_must_be_empty_regular_files() {
    for case in 0..3 {
        let fixture = Fixture::new(true);
        let context = fixture.context();
        let marker = fixture.heads_dir().join(MERGE_ID);
        fs::remove_file(&marker).unwrap();
        match case {
            0 => fs::write(marker, "nonempty head marker").unwrap(),
            1 => fs::create_dir(marker).unwrap(),
            2 => {
                let target = fixture.root.join("empty marker target");
                fs::write(&target, []).unwrap();
                symlink(target, marker).unwrap();
            }
            _ => unreachable!(),
        }
        rejected(&fixture, &context, None);
    }
}

#[test]
fn jj_capture_authenticates_exact_requested_operation_and_view_files() {
    for case in 0..6 {
        let fixture = Fixture::new(true);
        let context = fixture.context();
        match case {
            0 => fs::remove_file(fixture.operation_path(MERGE_ID)).unwrap(),
            1 => fs::write(fixture.operation_path(MERGE_ID), [0]).unwrap(),
            2 => fs::write(fixture.operation_path(MERGE_ID), first().operation_bytes).unwrap(),
            3 => fs::remove_file(fixture.view_path(RICH_ID)).unwrap(),
            4 => fs::write(fixture.view_path(RICH_ID), [0]).unwrap(),
            5 => fs::write(fixture.view_path(RICH_ID), first().view_bytes).unwrap(),
            _ => unreachable!(),
        }
        rejected(&fixture, &context, None);
    }
}

#[test]
fn jj_capture_checkout_has_no_fallback_for_missing_malformed_or_unjoined_context() {
    for case in 0..7 {
        let fixture = Fixture::new(true);
        let context = fixture.context();
        match case {
            0 => fs::remove_file(fixture.checkout_path()).unwrap(),
            1 => fs::write(fixture.checkout_path(), [0]).unwrap(),
            2 => fs::write(fixture.checkout_path(), vec![0; 16 * 1024 + 1]).unwrap(),
            3 => fixture.write_checkout(&"00".repeat(64), "default"),
            4 => fixture.write_checkout(MERGE_ID, ""),
            5 => fixture.write_checkout(FIRST_ID, "default"),
            6 => {
                fixture.write_evidence(&first());
                fixture.write_checkout(FIRST_ID, "default");
                fs::remove_file(fixture.view_path(MINIMAL_ID)).unwrap();
            }
            _ => unreachable!(),
        }
        rejected(&fixture, &context, Some("checkout"));
    }
}

#[test]
fn jj_capture_checks_locator_envelope_and_absolute_deadline_limits() {
    for case in 0..4 {
        let fixture = Fixture::new(true);
        let mut context = fixture.context();
        match case {
            0 => context.workspace_root = format!("/{}", "/".repeat(64 * 1024)).into(),
            1 => {
                context.jj.as_mut().unwrap().repo_dir = context.workspace_root.join("a".repeat(256))
            }
            2 => fs::write(fixture.operation_path(MERGE_ID), vec![0; 1024 * 1024 + 1]).unwrap(),
            3 => fs::write(fixture.view_path(RICH_ID), vec![0; 1024 * 1024]).unwrap(),
            _ => unreachable!(),
        }
        rejected(&fixture, &context, Some("limit"));
    }
    let fixture = Fixture::new(true);
    let context = fixture.context();
    let error = match checked_capture(&fixture, &context, Instant::now() - Duration::from_secs(1)) {
        Ok(_) => panic!("expired capture returned evidence"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("deadline"));
}
