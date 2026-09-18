use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[test]
fn file_workers_share_captured_paths_and_keep_replaced_snapshots_independent() {
    let repo = TestRepo::new();
    let paths: Vec<_> = (0..64).map(|i| format!("file-{i}.txt")).collect();
    for path in &paths {
        std::fs::write(repo.path().join(path), "known base\n").unwrap();
    }
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();
    repo.stage_all_and_commit("known base").unwrap();
    for path in &paths {
        repo.filename(path)
            .assert_committed_lines(crate::lines!["known base".human()]);
    }
    repo.git_ai(&["checkpoint", "human"]).unwrap();

    let mut working_log = repo.current_working_logs();
    working_log.set_dirty_files(Some(
        paths
            .iter()
            .map(|path| {
                (
                    repo.path().join(path).to_string_lossy().into_owned(),
                    Arc::from("known base\nAI addition\n"),
                )
            })
            .collect(),
    ));
    let workers: Vec<_> = paths.iter().map(|_| working_log.clone()).collect();
    let retained_paths: HashSet<_> = workers
        .iter()
        .flat_map(|worker| worker.dirty_files.as_ref().unwrap().keys())
        .map(|path| path.as_ptr())
        .collect();
    assert_eq!(
        retained_paths.len(),
        paths.len(),
        "file workers must retain one captured path index, not one complete copy per file"
    );

    working_log.set_dirty_files(Some(HashMap::from([(
        paths[0].clone(),
        Arc::from("later edit\n"),
    )])));
    assert_eq!(working_log.dirty_files.as_ref().unwrap().len(), 1);
    for (worker, path) in workers.iter().zip(&paths) {
        let captured = &worker.dirty_files.as_ref().unwrap()[path];
        assert_eq!(captured.as_ref(), "known base\nAI addition\n");
        std::fs::write(repo.path().join(path), captured.as_bytes()).unwrap();
    }
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
    repo.stage_all_and_commit("captured AI additions").unwrap();
    for path in paths {
        repo.filename(&path)
            .assert_committed_lines(crate::lines!["known base".human(), "AI addition".ai()]);
    }
}
