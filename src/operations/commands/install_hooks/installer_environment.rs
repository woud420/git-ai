use crate::error::GitAiError;
use std::collections::BTreeMap;
use std::path::Path;

const ALLOWED_PATH_VARIABLES: [&str; 4] = ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct InstallerEnvironment {
    values: BTreeMap<&'static str, String>,
}

impl InstallerEnvironment {
    pub(super) fn insert(&mut self, payload: &str) -> Result<(), GitAiError> {
        let Some((name, value)) = payload.split_once('=') else {
            return Err(invalid_payload());
        };
        let Some(name) = ALLOWED_PATH_VARIABLES
            .iter()
            .copied()
            .find(|allowed| *allowed == name)
        else {
            return Err(GitAiError::Generic(format!(
                "{name} is not allowed in --installer-env"
            )));
        };

        if value.is_empty() || value.contains('\0') {
            return Err(GitAiError::Generic(format!(
                "{name} must be a non-empty absolute path"
            )));
        }
        if !is_absolute_user_path(value) {
            return Err(GitAiError::Generic(format!(
                "{name} must be an absolute path"
            )));
        }
        if self.values.contains_key(name) {
            return Err(GitAiError::Generic(format!(
                "duplicate {name} in --installer-env"
            )));
        }

        self.values.insert(name, value.to_string());
        Ok(())
    }

    pub(super) fn apply(&self) {
        for (name, value) in &self.values {
            // SAFETY: command dispatch invokes this before install-hooks starts
            // its async runtime, telemetry worker, daemon work, or subprocesses.
            unsafe { std::env::set_var(name, value) };
        }
    }
}

fn invalid_payload() -> GitAiError {
    GitAiError::Generic("invalid --installer-env value; expected NAME=ABSOLUTE_PATH".to_string())
}

fn is_absolute_user_path(value: &str) -> bool {
    Path::new(value).is_absolute() || is_windows_absolute_path(value)
}

fn is_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    let has_drive_root = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let has_unc_root = (value.starts_with(r"\\") || value.starts_with("//"))
        && value[2..]
            .split(['/', '\\'])
            .filter(|part| !part.is_empty())
            .take(2)
            .count()
            == 2;
    has_drive_root || has_unc_root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_documented_absolute_user_paths() {
        let mut environment = InstallerEnvironment::default();
        environment.insert("HOME=/Users/alice").unwrap();
        environment
            .insert(r"USERPROFILE=C:\Users\Alice Example")
            .unwrap();
        environment
            .insert(r"APPDATA=\\server\profiles\alice\AppData\Roaming")
            .unwrap();
        environment
            .insert("LOCALAPPDATA=C:/Users/Alice/AppData/Local")
            .unwrap();

        assert_eq!(environment.values.len(), ALLOWED_PATH_VARIABLES.len());
    }

    #[test]
    fn rejects_malformed_empty_relative_and_duplicate_payloads() {
        let mut environment = InstallerEnvironment::default();
        assert!(environment.insert("HOME").is_err());
        assert!(environment.insert("HOME=").is_err());
        assert!(environment.insert("HOME=relative/path").is_err());
        environment.insert("HOME=/Users/alice").unwrap();
        assert!(environment.insert("HOME=/Users/bob").is_err());
    }

    #[test]
    fn rejects_unknown_variables_without_including_their_values() {
        let secret = "do-not-echo-this-secret";
        let error = InstallerEnvironment::default()
            .insert(&format!("API_KEY={secret}"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("API_KEY is not allowed"));
        assert!(!error.contains(secret));
    }
}
