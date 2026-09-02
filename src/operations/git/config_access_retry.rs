//! Opt-in retry for the known transient Git-for-Windows config denial.
//! Generic Git execution stays non-retrying because callers may mutate state.

use std::time::Duration;

use crate::error::GitAiError;

// Git for Windows can briefly deny access to a repository's config while a
// concurrent Git process is finishing an update. Keep this retry bounded: a
// real config or repository error must still reach the caller promptly.
const CONFIG_ACCESS_RETRY_DELAYS: &[Duration] = &[
    Duration::from_millis(25),
    Duration::from_millis(50),
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
    Duration::from_millis(800),
];

pub(crate) fn with_config_access_retry<T>(
    mut operation: impl FnMut() -> Result<T, GitAiError>,
    mut sleeper: impl FnMut(Duration),
) -> Result<T, GitAiError> {
    for (attempt, delay) in CONFIG_ACCESS_RETRY_DELAYS.iter().enumerate() {
        match operation() {
            Ok(output) => return Ok(output),
            Err(error) if attempt + 1 < CONFIG_ACCESS_RETRY_DELAYS.len() => {
                if is_transient_config_access_error(&error) {
                    sleeper(*delay);
                    continue;
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        }
    }

    // The retry table is non-empty by construction, but keep this branch
    // explicit so a future change cannot turn an exhausted retry into success.
    unreachable!("config access retry table must contain at least one delay")
}

fn is_transient_config_access_error(error: &GitAiError) -> bool {
    matches!(
        error,
        GitAiError::GitCliError {
            code: Some(128),
            stderr,
            ..
        } if stderr.contains("unable to access '.git/config': Permission denied")
            && stderr.contains("unknown error occurred while reading the configuration files")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG_DENIAL: &str = concat!(
        "error: unable to access '.git/config': Permission denied\n",
        "fatal: unknown error occurred while reading the configuration files"
    );

    fn git_error(code: Option<i32>, stderr: impl Into<String>, args: &[&str]) -> GitAiError {
        GitAiError::GitCliError {
            code,
            stderr: stderr.into(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
        }
    }

    #[test]
    fn exact_config_denial_is_transient() {
        assert!(is_transient_config_access_error(&git_error(
            Some(128),
            CONFIG_DENIAL,
            &["git", "rev-parse"]
        )));
    }

    #[test]
    fn classifier_rejects_wrong_code_or_missing_marker() {
        for error in [
            git_error(Some(1), CONFIG_DENIAL, &["git", "rev-parse"]),
            git_error(
                Some(128),
                "fatal: unknown error occurred while reading the configuration files",
                &["git", "rev-parse"],
            ),
            git_error(
                Some(128),
                "warning: unable to access '.git/config': Permission denied",
                &["git", "rev-parse"],
            ),
        ] {
            assert!(!is_transient_config_access_error(&error));
        }

        assert!(!is_transient_config_access_error(&GitAiError::IoError(
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, CONFIG_DENIAL),
        )));
    }

    #[test]
    fn retries_with_expected_delays_before_success() {
        let mut attempts = 0;
        let mut delays = Vec::new();

        let result = with_config_access_retry(
            || {
                attempts += 1;
                if attempts < 3 {
                    Err(git_error(Some(128), CONFIG_DENIAL, &["git", "rev-parse"]))
                } else {
                    Ok("success")
                }
            },
            |delay| delays.push(delay),
        );

        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempts, 3);
        assert_eq!(
            delays,
            [Duration::from_millis(25), Duration::from_millis(50)]
        );
    }

    #[test]
    fn unrelated_error_returns_without_sleeping() {
        let mut attempts = 0;
        let mut delays = Vec::new();

        let error = with_config_access_retry(
            || {
                attempts += 1;
                Err::<(), _>(git_error(
                    Some(128),
                    "fatal: not a git repository",
                    &["git", "rev-parse"],
                ))
            },
            |delay| delays.push(delay),
        )
        .unwrap_err();

        assert_eq!(attempts, 1);
        assert!(delays.is_empty());
        assert_eq!(
            error.to_string(),
            "Git CLI (git rev-parse) failed with exit code 128: fatal: not a git repository"
        );
    }

    #[test]
    fn exhaustion_returns_final_git_cli_error_unchanged() {
        let mut attempts = 0;
        let mut delays = Vec::new();

        let error = with_config_access_retry(
            || {
                attempts += 1;
                Err::<(), _>(git_error(
                    Some(128),
                    format!("{CONFIG_DENIAL}\nattempt {attempts}"),
                    &["git", "rev-parse", &format!("attempt-{attempts}")],
                ))
            },
            |delay| delays.push(delay),
        )
        .unwrap_err();

        assert_eq!(attempts, 6);
        assert_eq!(
            delays,
            [
                Duration::from_millis(25),
                Duration::from_millis(50),
                Duration::from_millis(100),
                Duration::from_millis(200),
                Duration::from_millis(400),
            ]
        );
        let expected_args = vec![
            "git".to_string(),
            "rev-parse".to_string(),
            "attempt-6".to_string(),
        ];
        match &error {
            GitAiError::GitCliError { code, stderr, args } => {
                assert_eq!(*code, Some(128));
                assert_eq!(stderr, &format!("{CONFIG_DENIAL}\nattempt 6"));
                assert_eq!(args, &expected_args);
            }
            other => panic!("expected GitCliError, got {other:?}"),
        }
        assert_eq!(
            error.to_string(),
            format!(
                "Git CLI (git rev-parse attempt-6) failed with exit code 128: {CONFIG_DENIAL}\nattempt 6"
            )
        );
    }
}
