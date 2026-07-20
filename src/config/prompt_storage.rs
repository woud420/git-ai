/// Prompt storage mode enum for type-safe handling
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptStorageMode {
    /// Default mode: prompts uploaded via CAS API, stripped from git notes
    Default,
    /// Notes mode: prompts stored in git notes (after secret redaction)
    Notes,
    /// Local mode: prompts only stored in local SQLite, never shared
    Local,
}

impl PromptStorageMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PromptStorageMode::Default => "default",
            PromptStorageMode::Notes => "notes",
            PromptStorageMode::Local => "local",
        }
    }
}

impl std::str::FromStr for PromptStorageMode {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.trim().to_lowercase().as_str() {
            "default" => Ok(PromptStorageMode::Default),
            "notes" => Ok(PromptStorageMode::Notes),
            "local" => Ok(PromptStorageMode::Local),
            other => Err(format!("invalid prompt storage mode: '{}'", other)),
        }
    }
}

impl std::fmt::Display for PromptStorageMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
