//! Common attributes shared across all metric events.

use super::pos_encoded::{PosEncoded, PosField, sparse_get_string, sparse_set, string_to_json};
use super::types::SparseArray;

/// Attribute positions (shared across all events).
pub mod attr_pos {
    pub const GIT_AI_VERSION: usize = 0;
    pub const REPO_URL: usize = 1;
    pub const AUTHOR: usize = 2;
    pub const COMMIT_SHA: usize = 3;
    pub const BASE_COMMIT_SHA: usize = 4;
    pub const BRANCH: usize = 5;
    pub const TOOL: usize = 20;
    pub const MODEL: usize = 21;
    // Position 22 (PROMPT_ID): TOMBSTONED - never reuse this index
    pub const PROMPT_ID: usize = 22;
    pub const EXTERNAL_SESSION_ID: usize = 23;
    pub const SESSION_ID: usize = 24;
    pub const TRACE_ID: usize = 25;
    pub const PARENT_SESSION_ID: usize = 26;
    pub const EXTERNAL_PARENT_SESSION_ID: usize = 27;
    pub const CUSTOM_ATTRIBUTES: usize = 30;
}

/// Common attributes for all events.
///
/// | Position | Name | Type | Required |
/// |----------|------|------|----------|
/// | 0 | git_ai_version | String | Yes |
/// | 1 | repo_url | String | No (nullable) |
/// | 2 | author | String | No (nullable) |
/// | 3 | commit_sha | String | No (nullable) |
/// | 4 | base_commit_sha | String | No (nullable) |
/// | 5 | branch | String | No (nullable) |
/// | 20 | tool | String | No (nullable) |
/// | 21 | model | String | No (nullable) |
/// | 22 | prompt_id (TOMBSTONED) | String | No (nullable) |
/// | 23 | external_session_id | String | No (nullable) |
/// | 24 | session_id | String | Yes |
/// | 25 | trace_id | String | No (nullable) |
/// | 26 | parent_session_id | String | No (nullable) |
/// | 27 | external_parent_session_id | String | No (nullable) |
/// | 30 | custom_attributes | String (JSON) | No (nullable) |
#[derive(Debug, Clone, Default)]
pub struct EventAttributes {
    pub git_ai_version: PosField<String>,
    pub repo_url: PosField<String>,
    pub author: PosField<String>,
    pub commit_sha: PosField<String>,
    pub base_commit_sha: PosField<String>,
    pub branch: PosField<String>,
    pub tool: PosField<String>,
    pub model: PosField<String>,
    pub prompt_id: PosField<String>,
    pub session_id: PosField<String>,
    pub trace_id: PosField<String>,
    pub parent_session_id: PosField<String>,
    pub external_session_id: PosField<String>,
    pub external_parent_session_id: PosField<String>,
    pub custom_attributes: PosField<String>,
}

impl EventAttributes {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with required git_ai_version field set.
    pub fn with_version(version: impl Into<String>) -> Self {
        Self {
            git_ai_version: Some(Some(version.into())),
            ..Default::default()
        }
    }

    // Builder methods for git_ai_version
    #[allow(dead_code)]
    pub fn git_ai_version(mut self, value: impl Into<String>) -> Self {
        self.git_ai_version = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn git_ai_version_null(mut self) -> Self {
        self.git_ai_version = Some(None);
        self
    }

    // Builder methods for repo_url
    pub fn repo_url(mut self, value: impl Into<String>) -> Self {
        self.repo_url = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn repo_url_null(mut self) -> Self {
        self.repo_url = Some(None);
        self
    }

    // Builder methods for author
    pub fn author(mut self, value: impl Into<String>) -> Self {
        self.author = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn author_null(mut self) -> Self {
        self.author = Some(None);
        self
    }

    // Builder methods for commit_sha
    pub fn commit_sha(mut self, value: impl Into<String>) -> Self {
        self.commit_sha = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn commit_sha_null(mut self) -> Self {
        self.commit_sha = Some(None);
        self
    }

    // Builder methods for base_commit_sha
    pub fn base_commit_sha(mut self, value: impl Into<String>) -> Self {
        self.base_commit_sha = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn base_commit_sha_null(mut self) -> Self {
        self.base_commit_sha = Some(None);
        self
    }

    // Builder methods for branch
    pub fn branch(mut self, value: impl Into<String>) -> Self {
        self.branch = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn branch_null(mut self) -> Self {
        self.branch = Some(None);
        self
    }

    // Builder methods for tool
    pub fn tool(mut self, value: impl Into<String>) -> Self {
        self.tool = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn tool_null(mut self) -> Self {
        self.tool = Some(None);
        self
    }

    // Builder methods for model
    pub fn model(mut self, value: impl Into<String>) -> Self {
        self.model = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn model_null(mut self) -> Self {
        self.model = Some(None);
        self
    }

    // Position 22 (prompt_id) is TOMBSTONED - setters removed, field kept for reading legacy data.

    // Builder methods for session_id
    pub fn session_id(mut self, value: impl Into<String>) -> Self {
        self.session_id = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn session_id_null(mut self) -> Self {
        self.session_id = Some(None);
        self
    }

    // Builder methods for trace_id
    pub fn trace_id(mut self, value: impl Into<String>) -> Self {
        self.trace_id = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn trace_id_null(mut self) -> Self {
        self.trace_id = Some(None);
        self
    }

    // Builder methods for parent_session_id
    pub fn parent_session_id(mut self, value: impl Into<String>) -> Self {
        self.parent_session_id = Some(Some(value.into()));
        self
    }

    pub fn parent_session_id_opt(self, value: Option<String>) -> Self {
        match value {
            Some(v) => self.parent_session_id(v),
            None => self,
        }
    }

    // Builder methods for external_session_id
    pub fn external_session_id(mut self, value: impl Into<String>) -> Self {
        self.external_session_id = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn external_session_id_null(mut self) -> Self {
        self.external_session_id = Some(None);
        self
    }

    pub fn external_session_id_opt(self, value: Option<String>) -> Self {
        match value {
            Some(v) => self.external_session_id(v),
            None => self,
        }
    }

    // Builder methods for external_parent_session_id
    pub fn external_parent_session_id(mut self, value: impl Into<String>) -> Self {
        self.external_parent_session_id = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn external_parent_session_id_null(mut self) -> Self {
        self.external_parent_session_id = Some(None);
        self
    }

    pub fn external_parent_session_id_opt(self, value: Option<String>) -> Self {
        match value {
            Some(v) => self.external_parent_session_id(v),
            None => self,
        }
    }

    // Builder methods for custom_attributes
    pub fn custom_attributes(mut self, value: impl Into<String>) -> Self {
        self.custom_attributes = Some(Some(value.into()));
        self
    }

    #[allow(dead_code)]
    pub fn custom_attributes_null(mut self) -> Self {
        self.custom_attributes = Some(None);
        self
    }

    pub fn custom_attributes_map(self, attrs: &std::collections::HashMap<String, String>) -> Self {
        if attrs.is_empty() {
            self
        } else {
            match serde_json::to_string(attrs) {
                Ok(json) => self.custom_attributes(json),
                Err(_) => self,
            }
        }
    }
}

impl PosEncoded for EventAttributes {
    fn to_sparse(&self) -> SparseArray {
        let mut map = SparseArray::new();
        sparse_set(
            &mut map,
            attr_pos::GIT_AI_VERSION,
            string_to_json(&self.git_ai_version),
        );
        sparse_set(&mut map, attr_pos::REPO_URL, string_to_json(&self.repo_url));
        sparse_set(&mut map, attr_pos::AUTHOR, string_to_json(&self.author));
        sparse_set(
            &mut map,
            attr_pos::COMMIT_SHA,
            string_to_json(&self.commit_sha),
        );
        sparse_set(
            &mut map,
            attr_pos::BASE_COMMIT_SHA,
            string_to_json(&self.base_commit_sha),
        );
        sparse_set(&mut map, attr_pos::BRANCH, string_to_json(&self.branch));
        sparse_set(&mut map, attr_pos::TOOL, string_to_json(&self.tool));
        sparse_set(&mut map, attr_pos::MODEL, string_to_json(&self.model));
        // Position 22 (PROMPT_ID) is TOMBSTONED - no longer written, only read for legacy data
        sparse_set(
            &mut map,
            attr_pos::EXTERNAL_SESSION_ID,
            string_to_json(&self.external_session_id),
        );
        sparse_set(
            &mut map,
            attr_pos::SESSION_ID,
            string_to_json(&self.session_id),
        );
        sparse_set(&mut map, attr_pos::TRACE_ID, string_to_json(&self.trace_id));
        sparse_set(
            &mut map,
            attr_pos::PARENT_SESSION_ID,
            string_to_json(&self.parent_session_id),
        );
        sparse_set(
            &mut map,
            attr_pos::EXTERNAL_PARENT_SESSION_ID,
            string_to_json(&self.external_parent_session_id),
        );
        sparse_set(
            &mut map,
            attr_pos::CUSTOM_ATTRIBUTES,
            string_to_json(&self.custom_attributes),
        );
        map
    }

    fn from_sparse(arr: &SparseArray) -> Self {
        Self {
            git_ai_version: sparse_get_string(arr, attr_pos::GIT_AI_VERSION),
            repo_url: sparse_get_string(arr, attr_pos::REPO_URL),
            author: sparse_get_string(arr, attr_pos::AUTHOR),
            commit_sha: sparse_get_string(arr, attr_pos::COMMIT_SHA),
            base_commit_sha: sparse_get_string(arr, attr_pos::BASE_COMMIT_SHA),
            branch: sparse_get_string(arr, attr_pos::BRANCH),
            tool: sparse_get_string(arr, attr_pos::TOOL),
            model: sparse_get_string(arr, attr_pos::MODEL),
            prompt_id: sparse_get_string(arr, attr_pos::PROMPT_ID),
            session_id: sparse_get_string(arr, attr_pos::SESSION_ID),
            trace_id: sparse_get_string(arr, attr_pos::TRACE_ID),
            parent_session_id: sparse_get_string(arr, attr_pos::PARENT_SESSION_ID),
            external_session_id: sparse_get_string(arr, attr_pos::EXTERNAL_SESSION_ID),
            external_parent_session_id: sparse_get_string(
                arr,
                attr_pos::EXTERNAL_PARENT_SESSION_ID,
            ),
            custom_attributes: sparse_get_string(arr, attr_pos::CUSTOM_ATTRIBUTES),
        }
    }
}

#[path = "attrs_tests.rs"]
#[cfg(test)]
mod tests;
