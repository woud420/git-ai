use crate::model::authorship_log::{HumanRecord, PromptRecord, SessionRecord};
use crate::operations::authorship::attribution_tracker::{Attribution, LineAttribution};
use crate::operations::git::repository::Repository;
use std::collections::{BTreeMap, HashMap, HashSet};

pub struct VirtualAttributions {
    pub(super) repo: Repository,
    pub(super) base_commit: String,
    // Maps file path -> (char attributions, line attributions)
    pub attributions: HashMap<String, (Vec<Attribution>, Vec<LineAttribution>)>,
    // Maps file path -> file content
    pub(super) file_contents: HashMap<String, String>,
    // Prompt records mapping prompt_id -> (commit_sha -> PromptRecord)
    // Same prompt can appear in multiple commits, allowing us to track and sort them
    pub prompts: BTreeMap<String, BTreeMap<String, PromptRecord>>,
    // Timestamp to use for attributions
    pub(super) ts: u128,
    pub blame_start_commit: Option<String>,
    pub humans: BTreeMap<String, HumanRecord>,
    // Prompt IDs that came from INITIAL attributions only (no matching checkpoint).
    // These are stale prompts from prior commits and should only appear in the
    // authorship note if they have committed lines in the current commit.
    pub(super) initial_only_prompt_ids: HashSet<String>,
    pub sessions: BTreeMap<String, SessionRecord>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct AuthorshipLogDiffContext<'a> {
    pub precomputed_parent_diff: Option<&'a crate::operations::authorship::rewrite::DiffTreeResult>,
    pub fallback_committed_diff_base: Option<&'a str>,
}

impl VirtualAttributions {
    /// Create VirtualAttributions from raw components (used for transformations)
    pub fn new(
        repo: Repository,
        base_commit: String,
        attributions: HashMap<String, (Vec<Attribution>, Vec<LineAttribution>)>,
        file_contents: HashMap<String, String>,
        ts: u128,
    ) -> Self {
        VirtualAttributions {
            repo,
            base_commit,
            attributions,
            file_contents,
            prompts: BTreeMap::new(),
            ts,
            blame_start_commit: None,
            humans: BTreeMap::new(),
            initial_only_prompt_ids: HashSet::new(),
            sessions: BTreeMap::new(),
        }
    }

    pub fn new_with_prompts(
        repo: Repository,
        base_commit: String,
        attributions: HashMap<String, (Vec<Attribution>, Vec<LineAttribution>)>,
        file_contents: HashMap<String, String>,
        prompts: BTreeMap<String, BTreeMap<String, PromptRecord>>,
        ts: u128,
    ) -> Self {
        VirtualAttributions {
            repo,
            base_commit,
            attributions,
            file_contents,
            prompts,
            ts,
            blame_start_commit: None,
            humans: BTreeMap::new(), // TODO(known-human): propagate humans from caller when rebase path is wired (Task 12)
            initial_only_prompt_ids: HashSet::new(),
            sessions: BTreeMap::new(),
        }
    }

    /// Get both character and line attributions for a file
    #[allow(dead_code)]
    pub fn get_attributions(
        &self,
        file_path: &str,
    ) -> Option<&(Vec<Attribution>, Vec<LineAttribution>)> {
        self.attributions.get(file_path)
    }

    /// Get just character-level attributions for a file
    pub fn get_char_attributions(&self, file_path: &str) -> Option<&Vec<Attribution>> {
        self.attributions
            .get(file_path)
            .map(|(char_attrs, _)| char_attrs)
    }

    /// Get just line-level attributions for a file
    pub fn get_line_attributions(&self, file_path: &str) -> Option<&Vec<LineAttribution>> {
        self.attributions
            .get(file_path)
            .map(|(_, line_attrs)| line_attrs)
    }

    /// List all tracked files
    pub fn files(&self) -> Vec<String> {
        self.attributions.keys().cloned().collect()
    }

    /// Get the base commit SHA
    pub fn base_commit(&self) -> &str {
        &self.base_commit
    }

    /// Get the timestamp used for attributions
    pub fn timestamp(&self) -> u128 {
        self.ts
    }

    /// Get the prompts metadata (prompt_id -> commit_sha -> PromptRecord)
    pub fn prompts(&self) -> &BTreeMap<String, BTreeMap<String, PromptRecord>> {
        &self.prompts
    }

    /// Get the file content for a tracked file
    pub fn get_file_content(&self, file_path: &str) -> Option<&String> {
        self.file_contents.get(file_path)
    }

    /// Get a reference to the repository
    pub fn repo(&self) -> &Repository {
        &self.repo
    }

    /// Get sessions map
    pub fn sessions(&self) -> &BTreeMap<String, SessionRecord> {
        &self.sessions
    }

    /// Union-merge two human records maps.
    /// Because records are keyed by content-hash of the author identity, any value
    /// for a given key is semantically equivalent. Simple `b`-wins extension is safe.
    pub(super) fn merge_humans(
        a: &BTreeMap<String, HumanRecord>,
        b: &BTreeMap<String, HumanRecord>,
    ) -> BTreeMap<String, HumanRecord> {
        let mut result = a.clone();
        result.extend(b.iter().map(|(k, v)| (k.clone(), v.clone())));
        result
    }
}
