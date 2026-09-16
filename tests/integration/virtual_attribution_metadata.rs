use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::model::attribution_tracker::LineAttribution;
use git_ai::model::authorship_log::{LineRange, PromptRecord, SessionRecord};
use git_ai::model::authorship_log_serialization::{AuthorshipLog, generate_short_hash};
use git_ai::model::working_log::{AgentId, Checkpoint, CheckpointKind};
use git_ai::operations::authorship::virtual_attribution::VirtualAttributions;
use git_ai::operations::git::repository::{Repository, find_repository_in_path};
use std::collections::{BTreeMap, HashMap};

struct MetadataFixture {
    _repo: TestRepo,
    repository: Repository,
    parent: String,
    commit: String,
    attributions: VirtualAttributions,
    prompt_ids: [String; 4],
}

impl MetadataFixture {
    fn new(initial_prompts: bool, committed_references: bool) -> Self {
        let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
        repo.commit_untracked_file("base.txt", "base", "base");
        repo.filename("base.txt")
            .assert_committed_lines(crate::lines!["base".unattributed_human()]);
        let parent = repo
            .git_og(&["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();

        repo.write_file("first.txt", "initial reference\ncheckpoint reference\n");
        repo.write_file("second.txt", "repeated reference\nsession reference\n");
        repo.git_og(&["add", "first.txt", "second.txt"]).unwrap();
        repo.git_og(&["commit", "-m", "metadata conversion inputs"])
            .unwrap();
        repo.filename("base.txt")
            .assert_committed_lines(crate::lines!["base".unattributed_human()]);
        repo.filename("first.txt")
            .assert_committed_lines(crate::lines![
                "initial reference".unattributed_human(),
                "checkpoint reference".unattributed_human(),
            ]);
        repo.filename("second.txt")
            .assert_committed_lines(crate::lines![
                "repeated reference".unattributed_human(),
                "session reference".unattributed_human(),
            ]);
        let commit = repo
            .git_og(&["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();
        let repository = find_repository_in_path(repo.path().to_str().unwrap()).unwrap();
        let working_log = repository
            .storage
            .working_log_for_base_commit(&parent)
            .unwrap();

        let agents = [
            "initial-referenced",
            "initial-unreferenced",
            "checkpoint-referenced",
            "checkpoint-unreferenced",
        ]
        .map(|id| AgentId {
            tool: "metadata-fixture".to_string(),
            id: id.to_string(),
            model: "fixture-model".to_string(),
        });
        let prompt_ids = agents
            .each_ref()
            .map(|agent| generate_short_hash(&agent.id, &agent.tool));
        let prompts = if initial_prompts {
            agents
                .iter()
                .zip(&prompt_ids)
                .map(|(agent, id)| {
                    (
                        id.clone(),
                        PromptRecord {
                            agent_id: agent.clone(),
                            human_author: Some("Fixture Human <fixture@example.com>".to_string()),
                            messages_url: Some(format!("https://example.test/{id}")),
                            total_additions: 9,
                            total_deletions: 2,
                            accepted_lines: 3,
                            overriden_lines: 4,
                            custom_attributes: Some(HashMap::from([(
                                "source".to_string(),
                                "initial".to_string(),
                            )])),
                        },
                    )
                })
                .collect()
        } else {
            HashMap::new()
        };
        let sessions = ["s_referenced", "s_unreferenced"]
            .into_iter()
            .map(|id| {
                (
                    id.to_string(),
                    SessionRecord {
                        agent_id: AgentId {
                            tool: "metadata-fixture".to_string(),
                            id: id.to_string(),
                            model: "session-model".to_string(),
                        },
                        human_author: Some("Session Human <session@example.com>".to_string()),
                        custom_attributes: None,
                    },
                )
            })
            .collect();
        let files = if committed_references {
            HashMap::from([
                (
                    "first.txt".to_string(),
                    vec![
                        LineAttribution::new(1, 1, prompt_ids[0].clone(), None),
                        LineAttribution::new(2, 2, prompt_ids[2].clone(), None),
                    ],
                ),
                (
                    "second.txt".to_string(),
                    vec![
                        LineAttribution::new(1, 1, prompt_ids[0].clone(), None),
                        LineAttribution::new(2, 2, "s_referenced::trace".to_string(), None),
                    ],
                ),
            ])
        } else {
            // INITIAL must have a file to survive persistence. This unchanged
            // base line has no hunk in the target commit, so it is not attested.
            HashMap::from([(
                "base.txt".to_string(),
                vec![LineAttribution::new(1, 1, prompt_ids[0].clone(), None)],
            )])
        };
        working_log
            .write_initial_attributions_with_contents(
                files,
                prompts,
                BTreeMap::new(),
                HashMap::from([
                    ("base.txt".to_string(), "base\n".to_string()),
                    (
                        "first.txt".to_string(),
                        "initial reference\ncheckpoint reference\n".to_string(),
                    ),
                    (
                        "second.txt".to_string(),
                        "repeated reference\nsession reference\n".to_string(),
                    ),
                ]),
                sessions,
            )
            .unwrap();

        // A legacy checkpoint removes its prompt from INITIAL-only tracking even
        // when it has no file entries; this differs from a session checkpoint.
        for agent in agents.iter().skip(if initial_prompts { 2 } else { 0 }) {
            let mut checkpoint = Checkpoint::new(
                CheckpointKind::AiAgent,
                String::new(),
                "Fixture Human <fixture@example.com>".to_string(),
                Vec::new(),
            );
            checkpoint.agent_id = Some(agent.clone());
            working_log.append_checkpoint(&checkpoint).unwrap();
        }
        let attributions =
            VirtualAttributions::from_just_working_log(repository.clone(), parent.clone(), None)
                .unwrap();
        assert_eq!(attributions.prompts.len(), 4);
        assert_eq!(attributions.sessions.len(), 2);
        Self {
            _repo: repo,
            repository,
            parent,
            commit,
            attributions,
            prompt_ids,
        }
    }

    fn outputs(&self) -> [(&'static str, AuthorshipLog); 2] {
        let index_only = self
            .attributions
            .to_authorship_log_index_only(&self.repository, &self.parent, &self.commit, None)
            .unwrap();
        let (full, initial, contents) = self
            .attributions
            .to_authorship_log_and_initial_working_log(
                &self.repository,
                &self.parent,
                &self.commit,
                None,
                None,
            )
            .unwrap();
        assert!(initial.files.is_empty());
        assert!(contents.is_empty());
        [("index-only", index_only), ("full", full)]
    }

    fn expected_prompts(&self, indices: &[usize]) -> BTreeMap<String, PromptRecord> {
        indices
            .iter()
            .map(|&index| {
                let id = &self.prompt_ids[index];
                (
                    id.clone(),
                    self.attributions.prompts[id]
                        .values()
                        .next()
                        .unwrap()
                        .clone(),
                )
            })
            .collect()
    }
}

#[test]
fn metadata_conversion_preserves_initial_and_checkpoint_prompt_retention() {
    let fixture = MetadataFixture::new(true, true);
    let expected_prompts = fixture.expected_prompts(&[0, 2, 3]);
    let expected_lines = BTreeMap::from([
        (
            ("first.txt".to_string(), fixture.prompt_ids[0].clone()),
            vec![LineRange::Single(1)],
        ),
        (
            ("first.txt".to_string(), fixture.prompt_ids[2].clone()),
            vec![LineRange::Single(2)],
        ),
        (
            ("second.txt".to_string(), fixture.prompt_ids[0].clone()),
            vec![LineRange::Single(1)],
        ),
        (
            ("second.txt".to_string(), "s_referenced::trace".to_string()),
            vec![LineRange::Single(2)],
        ),
    ]);
    for (mode, output) in fixture.outputs() {
        assert_eq!(output.metadata.prompts, expected_prompts, "{mode}");
        assert_eq!(output.metadata.base_commit_sha, fixture.parent, "{mode}");
        let lines: BTreeMap<_, _> = output
            .attestations
            .iter()
            .flat_map(|file| {
                file.entries.iter().map(|entry| {
                    (
                        (file.file_path.clone(), entry.hash.clone()),
                        entry.line_ranges.clone(),
                    )
                })
            })
            .collect();
        assert_eq!(lines, expected_lines, "{mode}");
        assert_eq!(output.attestations.len(), 2, "{mode}");
        let expected_sessions = if mode == "index-only" {
            fixture.attributions.sessions.clone()
        } else {
            BTreeMap::from([(
                "s_referenced".to_string(),
                fixture.attributions.sessions["s_referenced"].clone(),
            )])
        };
        assert_eq!(output.metadata.sessions, expected_sessions, "{mode}");
    }
}

#[test]
fn metadata_conversion_preserves_empty_initial_and_attestation_cases() {
    for (initial_prompts, expected_indices) in [(true, vec![2, 3]), (false, vec![0, 1, 2, 3])] {
        let fixture = MetadataFixture::new(initial_prompts, false);
        let expected_prompts = fixture.expected_prompts(&expected_indices);
        for (mode, output) in fixture.outputs() {
            assert!(
                output.attestations.is_empty(),
                "{mode}, initial={initial_prompts}"
            );
            assert_eq!(
                output.metadata.prompts, expected_prompts,
                "{mode}, initial={initial_prompts}"
            );
            let expected_sessions = if mode == "index-only" {
                fixture.attributions.sessions.clone()
            } else {
                BTreeMap::new()
            };
            assert_eq!(
                output.metadata.sessions, expected_sessions,
                "{mode}, initial={initial_prompts}"
            );
        }
    }
}
