use super::*;
use crate::repos::test_file::ExpectedLineExt;
use git_ai::model::authorship_log_serialization::AUTHORSHIP_LOG_VERSION;
use git_ai::operations::git::notes_api::{read_authorship_v3, read_authorship_v3_batch};

#[test]
fn batched_notes_match_single_reads_for_invalid_utf8_versions_and_missing_notes() {
    let repo = TestRepo::new();
    let mut shas = Vec::new();
    for index in 0..6 {
        let contents = format!("AI line {index}");
        std::fs::write(repo.path().join("file.txt"), &contents).unwrap();
        repo.git_ai(&["checkpoint", "mock_ai", "file.txt"]).unwrap();
        shas.push(repo.stage_all_and_commit(&contents).unwrap().commit_sha);
        repo.filename("file.txt")
            .assert_committed_lines(vec![contents.ai()]);
    }
    let original = repo
        .git_og(&["notes", "--ref=ai", "show", &shas[0]])
        .unwrap();
    let mut invalid_utf8 = original.as_bytes().to_vec();
    let position = original.find("mock_ai").unwrap();
    invalid_utf8[position] = 0xff;
    for (sha, content) in [
        (
            &shas[1],
            original
                .replace(AUTHORSHIP_LOG_VERSION, "authorship/999.0.0")
                .into_bytes(),
        ),
        (&shas[2], invalid_utf8),
        (&shas[3], b"not an authorship log".to_vec()),
        (&shas[5], original.as_bytes().to_vec()),
    ] {
        let file = repo.path().join("replacement-note");
        std::fs::write(&file, content).unwrap();
        repo.git_og(&[
            "notes",
            "--ref=ai",
            "add",
            "-f",
            "-F",
            file.to_str().unwrap(),
            sha,
        ])
        .unwrap();
    }
    repo.git_og(&["notes", "--ref=ai", "remove", &shas[4]])
        .unwrap();
    shas.push("0000000000000000000000000000000000000000".to_owned());
    shas.push(shas[0].clone());
    let git_repo = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
    let batch = read_authorship_v3_batch(&git_repo, &shas).unwrap();
    assert_eq!(batch.len(), 2, "only the valid notes survive");
    for sha in &shas {
        let single = read_authorship_v3(&git_repo, sha).ok();
        assert_eq!(
            batch.get(sha).map(|log| log.serialize_to_string().unwrap()),
            single.map(|log| log.serialize_to_string().unwrap()),
            "batch/single parity for {sha}",
        );
    }
    assert_eq!(batch[&shas[5]].metadata.base_commit_sha, shas[5]);
    assert!(read_authorship_v3_batch(&git_repo, &[]).unwrap().is_empty());
}
