use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::authorship_log::{LineRange, PromptRecord};
use git_ai::model::authorship_log_serialization::{
    AttestationEntry, AuthorshipLog, FileAttestation,
};
use git_ai::model::working_log::AgentId;
use git_ai::operations::commands::blame::GitAiBlameOptions;
use git_ai::operations::git::notes_api::write_note;
use git_ai::operations::git::repository as GitAiRepository;

// Helper function to extract author names from blame output
fn extract_authors(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            // Extract author name from blame line format
            // Format: sha (author date line) code
            if let Some(start) = line.find('(') {
                line[start..]
                    .find(' ')
                    .map(|end| line[start + 1..start + end].trim().to_string())
            } else {
                None
            }
        })
        .collect()
}

// Helper function to normalize blame output for comparison
// This replaces author names with consistent placeholders to avoid drift from author names
fn normalize_for_snapshot(output: &str) -> String {
    output
        .lines()
        .map(|line| {
            // Handle porcelain format lines
            if line.starts_with("author-mail") || line.starts_with("committer-mail") {
                // Keep these lines as-is for porcelain format
                line.to_string()
            } else if line.starts_with("author ") || line.starts_with("committer ") {
                // Keep author/committer lines as-is for porcelain format
                line.to_string()
            } else if line.starts_with("author-time")
                || line.starts_with("author-tz")
                || line.starts_with("committer-time")
                || line.starts_with("committer-tz")
            {
                // Keep time/tz lines as-is for porcelain format
                line.to_string()
            } else if line.starts_with("summary")
                || line.starts_with("boundary")
                || line.starts_with("filename")
            {
                // Keep metadata lines as-is for porcelain format
                line.to_string()
            } else if line.starts_with('\t') {
                // Keep content lines (starting with tab) as-is for porcelain format
                line.to_string()
            } else if let Some(start) = line.find('(') {
                if let Some(end) = line[start..].find(')') {
                    // Replace the entire author/date/line section with a consistent placeholder
                    let before = &line[..start + 1];
                    let after = &line[start + end..];
                    format!("{}<AUTHOR_INFO>{}", before, after)
                } else {
                    line.to_string()
                }
            } else {
                line.to_string()
            }
        })
        .map(|line| {
            // Remove the ^ prefix that git adds for boundary commits
            if let Some(stripped) = line.strip_prefix('^') {
                stripped.to_string()
            } else {
                line
            }
        })
        .map(|line| {
            // Only normalize hash length for lines that look like blame output (start with hash)
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let first_part = parts[0];
                // Only apply hash normalization if the first part looks like a hash (hex chars)
                if first_part.chars().all(|c| c.is_ascii_hexdigit()) && first_part.len() >= 7 {
                    let rest = &parts[1..];
                    // Truncate hash to 7 characters for consistent comparison (git blame default)
                    let normalized_hash = if first_part.len() > 7 {
                        &first_part[..7]
                    } else {
                        first_part
                    };
                    format!("{} {}", normalized_hash, rest.join(" "))
                } else {
                    line
                }
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn multiple_range_blame_repo() -> TestRepo {
    let repo = TestRepo::new();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines![
        "Line 1",
        "Line 2",
        "Line 3",
        "Line 4",
        "Line 5".ai(),
        "Line 6".ai(),
        "Line 7",
        "Line 8"
    ]);

    repo.stage_all_and_commit("Initial commit").unwrap();

    repo
}

fn assert_multiple_range_blame_matches_git(args: &[&str], message: &str) {
    let repo = multiple_range_blame_repo();
    let git_output = repo.git(args).unwrap();
    let spawn_log = repo.test_home_path().join("blame-render-spawns.log");
    let spawn_log = spawn_log.to_str().expect("spawn log path should be UTF-8");
    let git_ai_output = repo
        .git_ai_with_env(args, &[("GIT_AI_SPAWN_LOG", spawn_log)])
        .unwrap();

    assert_eq!(
        normalize_for_snapshot(&git_output),
        normalize_for_snapshot(&git_ai_output),
        "{message}"
    );
    let spawns = std::fs::read_to_string(spawn_log).expect("read blame spawn log");
    assert_eq!(
        spawns.lines().filter(|line| *line == "blame").count(),
        2,
        "each invocation should use one analysis blame and one renderer-preparation blame; spawns:\n{spawns}"
    );
}

mod author_display;
mod formatting;
mod ignore_revisions;
mod ranges_and_machine_output;
mod unknown_attribution;
