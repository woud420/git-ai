use serde::{Deserialize, Serialize};

/// Optional git-ai author override for authorship metadata.
///
/// Any unset field falls back to the effective Git committer identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AuthorConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl AuthorConfig {
    pub fn normalized(mut self) -> Self {
        self.name = normalize_optional_string(self.name);
        self.email = normalize_optional_string(self.email);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.email.is_none()
    }
}

pub(crate) fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
