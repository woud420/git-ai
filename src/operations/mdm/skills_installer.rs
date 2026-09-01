use crate::config::skills_dir_path;
use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use crate::operations::mdm::file_ops::write_atomic;
use crate::operations::mdm::paths::claude_config_dir;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

/// One file bundled with an embedded skill.
struct EmbeddedSkillFile {
    relative_path: &'static str,
    contents: &'static str,
}

/// Embedded skill bundle.
struct EmbeddedSkill {
    name: &'static str,
    files: &'static [EmbeddedSkillFile],
}

/// All embedded skills - add new skills here
const EMBEDDED_SKILLS: &[EmbeddedSkill] = &[
    EmbeddedSkill {
        name: "prompt-analysis",
        files: &[
            EmbeddedSkillFile {
                relative_path: "SKILL.md",
                contents: include_str!("../../../.agents/skills/prompt-analysis/SKILL.md"),
            },
            EmbeddedSkillFile {
                relative_path: "agents/openai.yaml",
                contents: include_str!(
                    "../../../.agents/skills/prompt-analysis/agents/openai.yaml"
                ),
            },
        ],
    },
    EmbeddedSkill {
        name: "git-ai-search",
        files: &[
            EmbeddedSkillFile {
                relative_path: "SKILL.md",
                contents: include_str!("../../../.agents/skills/git-ai-search/SKILL.md"),
            },
            EmbeddedSkillFile {
                relative_path: "agents/openai.yaml",
                contents: include_str!("../../../.agents/skills/git-ai-search/agents/openai.yaml"),
            },
        ],
    },
    EmbeddedSkill {
        name: "ask",
        files: &[
            EmbeddedSkillFile {
                relative_path: "SKILL.md",
                contents: include_str!("../../../.agents/skills/ask/SKILL.md"),
            },
            EmbeddedSkillFile {
                relative_path: "agents/openai.yaml",
                contents: include_str!("../../../.agents/skills/ask/agents/openai.yaml"),
            },
        ],
    },
];

/// Result of installing skills
pub struct SkillsInstallResult {
    /// Whether any changes were made
    pub changed: bool,
    /// Number of skills installed
    #[allow(dead_code)]
    pub installed_count: usize,
}

/// Get the ~/.agents/skills directory path
fn agents_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".agents").join("skills"))
}

fn claude_skills_dir() -> Option<PathBuf> {
    Some(claude_config_dir().join("skills"))
}

/// Get the ~/.cursor/skills directory path
fn cursor_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cursor").join("skills"))
}

struct SkillsPaths {
    source: PathBuf,
    agents: Option<PathBuf>,
    claude: Option<PathBuf>,
    cursor: Option<PathBuf>,
}

impl SkillsPaths {
    fn current() -> Result<Self, GitAiError> {
        Ok(Self {
            source: skills_dir_path().ok_or_else(missing_skills_dir)?,
            agents: agents_skills_dir(),
            claude: claude_skills_dir(),
            cursor: cursor_skills_dir(),
        })
    }
}

fn missing_skills_dir() -> PersistenceError {
    PersistenceError::Io {
        operation: "Generic error",
        path: String::new(),
        kind: std::io::ErrorKind::NotFound,
        message: "Could not determine skills directory path".to_string(),
    }
}

/// Link a skill directory to the target location.
/// On Unix, creates a symlink. On Windows, copies the directory to avoid requiring
/// Administrator privileges (which symlink creation requires on Windows).
fn link_skill_dir(target: &PathBuf, link_path: &PathBuf) -> Result<(), GitAiError> {
    // Create parent directory if needed
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Remove existing file/symlink/directory if present
    if link_path.exists() || link_path.symlink_metadata().is_ok() {
        if link_path.is_dir()
            && !link_path
                .symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
        {
            fs::remove_dir_all(link_path)?;
        } else {
            fs::remove_file(link_path)?;
        }
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link_path)?;

    #[cfg(windows)]
    copy_dir_recursive(target, link_path)?;

    Ok(())
}

/// Recursively copy a directory and its contents from src to dst.
#[cfg(windows)]
fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> Result<(), GitAiError> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let entry_path = entry.path();
        let dest_path = dst.join(entry.file_name());
        if entry_path.is_dir() {
            copy_dir_recursive(&entry_path, &dest_path)?;
        } else {
            fs::copy(&entry_path, &dest_path)?;
        }
    }
    Ok(())
}

/// Remove a skill link (symlink on Unix, copied directory on Windows) if it exists.
fn remove_skill_link(link_path: &PathBuf) -> Result<(), GitAiError> {
    if link_path.symlink_metadata().is_ok() {
        let is_symlink = link_path
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if is_symlink {
            fs::remove_file(link_path)?;
        } else if link_path.is_dir() {
            fs::remove_dir_all(link_path)?;
        }
    }
    Ok(())
}

/// Install all embedded skills to ~/.git-ai/skills/
/// This nukes the entire skills directory and recreates it fresh each time.
///
/// Creates the standard skills structure:
/// ~/.git-ai/skills/
/// └── prompt-analysis/
///     ├── SKILL.md
///     └── agents/openai.yaml
///
/// Then links each skill to:
/// - ~/.agents/skills/{skill-name} (symlink on Unix, copy on Windows)
/// - ~/.claude/skills/{skill-name} (symlink on Unix, copy on Windows)
pub fn install_skills(
    dry_run: bool,
    _verbose: bool,
    installed_tools: &HashSet<String>,
) -> Result<SkillsInstallResult, GitAiError> {
    install(&SkillsPaths::current()?, dry_run, installed_tools)
}

fn install(
    paths: &SkillsPaths,
    dry_run: bool,
    installed_tools: &HashSet<String>,
) -> Result<SkillsInstallResult, GitAiError> {
    let skills_base = &paths.source;

    if dry_run {
        return Ok(SkillsInstallResult {
            changed: true,
            installed_count: EMBEDDED_SKILLS.len(),
        });
    }

    // Nuke the skills directory if it exists
    if skills_base.exists() {
        fs::remove_dir_all(skills_base)?;
    }

    // Create fresh skills directory
    fs::create_dir_all(skills_base)?;

    let link_roots = [
        paths.agents.as_ref(),
        installed_tools
            .contains("claude-code")
            .then_some(paths.claude.as_ref())
            .flatten(),
        installed_tools
            .contains("cursor")
            .then_some(paths.cursor.as_ref())
            .flatten(),
    ];

    // Install each skill
    for skill in EMBEDDED_SKILLS {
        // Create skill directory: ~/.git-ai/skills/{skill-name}/
        let skill_dir = skills_base.join(skill.name);
        fs::create_dir_all(&skill_dir)?;

        // Write the complete skill bundle, including per-harness metadata.
        for file in skill.files {
            let file_path = skill_dir.join(file.relative_path);
            write_atomic(&file_path, file.contents.as_bytes())?;
        }

        for link_root in link_roots.iter().copied().flatten() {
            let link = link_root.join(skill.name);
            if let Err(e) = link_skill_dir(&skill_dir, &link) {
                eprintln!("Warning: Failed to link skill at {:?}: {}", link, e);
            }
        }
    }

    Ok(SkillsInstallResult {
        changed: true,
        installed_count: EMBEDDED_SKILLS.len(),
    })
}

/// Uninstall all skills by removing ~/.git-ai/skills/ and linked skill directories
pub fn uninstall_skills(dry_run: bool, _verbose: bool) -> Result<SkillsInstallResult, GitAiError> {
    uninstall(&SkillsPaths::current()?, dry_run)
}

fn uninstall(paths: &SkillsPaths, dry_run: bool) -> Result<SkillsInstallResult, GitAiError> {
    let skills_base = &paths.source;

    if !skills_base.exists() {
        return Ok(SkillsInstallResult {
            changed: false,
            installed_count: 0,
        });
    }

    if dry_run {
        return Ok(SkillsInstallResult {
            changed: true,
            installed_count: EMBEDDED_SKILLS.len(),
        });
    }

    // Remove linked skill directories first
    for skill in EMBEDDED_SKILLS {
        for link_root in [&paths.agents, &paths.claude, &paths.cursor]
            .into_iter()
            .flatten()
        {
            let link = link_root.join(skill.name);
            if let Err(e) = remove_skill_link(&link) {
                eprintln!("Warning: Failed to remove skill link at {:?}: {}", link, e);
            }
        }
    }

    // Nuke the entire skills directory
    fs::remove_dir_all(skills_base)?;

    Ok(SkillsInstallResult {
        changed: true,
        installed_count: EMBEDDED_SKILLS.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_skills_are_loaded() {
        for skill in EMBEDDED_SKILLS {
            assert!(!skill.name.is_empty(), "Skill name should not be empty");
            assert!(!skill.files.is_empty(), "Skill {} has no files", skill.name);
            for file in skill.files {
                assert!(
                    !file.relative_path.is_empty(),
                    "Skill {} has an unnamed file",
                    skill.name
                );
                assert!(
                    !file.relative_path.starts_with('/'),
                    "Skill {} has an absolute file path: {}",
                    skill.name,
                    file.relative_path
                );
                assert!(
                    !file.contents.is_empty(),
                    "Skill file is empty: {}/{}",
                    skill.name,
                    file.relative_path
                );
                if file.relative_path == "SKILL.md" {
                    assert!(
                        file.contents.contains("---"),
                        "Skill {} should have frontmatter",
                        skill.name
                    );
                }
            }
        }
    }

    #[test]
    fn test_skills_dir_path_is_under_git_ai() {
        if let Some(path) = skills_dir_path() {
            assert!(path.ends_with("skills"));
            let parent = path.parent().unwrap();
            assert!(parent.ends_with(".git-ai"));
        }
    }

    #[test]
    fn test_missing_skills_dir_preserves_error_contract() {
        let error: GitAiError = missing_skills_dir().into();
        assert!(matches!(&error, GitAiError::Persistence(_)));
        assert_eq!(
            error.to_string(),
            "Generic error: Could not determine skills directory path"
        );
    }

    #[test]
    fn test_link_skill_dir_creates_link_and_content_is_accessible() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source-skill");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "test content").unwrap();

        let link = tmp.path().join("linked-skill");
        link_skill_dir(&source, &link).unwrap();

        assert!(link.exists());
        assert!(link.join("SKILL.md").exists());
        assert_eq!(
            fs::read_to_string(link.join("SKILL.md")).unwrap(),
            "test content"
        );
    }

    #[test]
    fn test_link_skill_dir_replaces_existing_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source-skill");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "new content").unwrap();

        let link = tmp.path().join("linked-skill");
        fs::create_dir_all(&link).unwrap();
        fs::write(link.join("SKILL.md"), "old content").unwrap();

        link_skill_dir(&source, &link).unwrap();

        assert_eq!(
            fs::read_to_string(link.join("SKILL.md")).unwrap(),
            "new content"
        );
    }

    #[test]
    fn test_link_skill_dir_replaces_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source-skill");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "content").unwrap();

        let link = tmp.path().join("linked-skill");
        fs::write(&link, "i am a file").unwrap();

        link_skill_dir(&source, &link).unwrap();

        assert!(link.is_dir() || link.is_symlink());
        assert!(link.join("SKILL.md").exists());
    }

    #[test]
    fn test_link_skill_dir_creates_parent_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source-skill");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "content").unwrap();

        let link = tmp.path().join("deep").join("nested").join("linked-skill");
        link_skill_dir(&source, &link).unwrap();

        assert!(link.exists());
        assert!(link.join("SKILL.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_link_skill_dir_creates_symlink_on_unix() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source-skill");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "content").unwrap();

        let link = tmp.path().join("linked-skill");
        link_skill_dir(&source, &link).unwrap();

        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), source);
    }

    #[test]
    fn test_remove_skill_link_removes_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skill-dir");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "content").unwrap();

        remove_skill_link(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_skill_link_removes_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();

        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        remove_skill_link(&link).unwrap();
        assert!(link.symlink_metadata().is_err());
        assert!(target.exists(), "original target should not be removed");
    }

    #[test]
    fn test_remove_skill_link_noop_on_nonexistent_path() {
        let tmp = tempfile::tempdir().unwrap();
        let nonexistent = tmp.path().join("does-not-exist");
        remove_skill_link(&nonexistent).unwrap();
    }

    #[test]
    fn test_install_and_uninstall_skills_lifecycle() {
        // Regression coverage for ENG-339.
        let temp = tempfile::tempdir().unwrap();
        let paths = SkillsPaths {
            source: temp.path().join(".git-ai/skills"),
            agents: Some(temp.path().join(".agents/skills")),
            claude: Some(temp.path().join(".claude/skills")),
            cursor: Some(temp.path().join(".cursor/skills")),
        };
        let all_tools = HashSet::from(["claude-code".to_string(), "cursor".to_string()]);

        let assert_installed = || {
            let roots = [
                &paths.source,
                paths.agents.as_ref().unwrap(),
                paths.claude.as_ref().unwrap(),
                paths.cursor.as_ref().unwrap(),
            ];
            for skill in EMBEDDED_SKILLS {
                for file in skill.files {
                    for root in roots {
                        let installed_file = root.join(skill.name).join(file.relative_path);
                        assert!(
                            installed_file.exists(),
                            "{} missing for {} under {}",
                            file.relative_path,
                            skill.name,
                            root.display()
                        );
                        assert_eq!(fs::read_to_string(installed_file).unwrap(), file.contents);
                    }
                }
            }
        };

        let dry_result = install(&paths, true, &all_tools).unwrap();
        assert!(dry_result.changed);
        assert_eq!(dry_result.installed_count, EMBEDDED_SKILLS.len());
        for root in [
            &paths.source,
            paths.agents.as_ref().unwrap(),
            paths.claude.as_ref().unwrap(),
            paths.cursor.as_ref().unwrap(),
        ] {
            assert!(
                root.symlink_metadata().is_err(),
                "dry run wrote {}",
                root.display()
            );
        }

        let result = install(&paths, false, &all_tools).unwrap();
        assert!(result.changed);
        assert_eq!(result.installed_count, EMBEDDED_SKILLS.len());
        assert_installed();

        let first_skill = &EMBEDDED_SKILLS[0];
        let first_file = &first_skill.files[0];
        let stale_link_file = paths
            .agents
            .as_ref()
            .unwrap()
            .join(first_skill.name)
            .join(first_file.relative_path);
        fs::write(stale_link_file, "stale content").unwrap();

        let repeated_result = install(&paths, false, &all_tools).unwrap();
        assert!(repeated_result.changed);
        assert_eq!(repeated_result.installed_count, EMBEDDED_SKILLS.len());
        assert_installed();

        let uninstall_result = uninstall(&paths, false).unwrap();
        assert!(uninstall_result.changed);
        assert!(paths.source.symlink_metadata().is_err());
        for root in [
            paths.agents.as_ref().unwrap(),
            paths.claude.as_ref().unwrap(),
            paths.cursor.as_ref().unwrap(),
        ] {
            for skill in EMBEDDED_SKILLS {
                let link = root.join(skill.name);
                assert!(
                    link.symlink_metadata().is_err(),
                    "link remains after uninstall: {}",
                    link.display()
                );
            }
        }

        let noop_result = uninstall(&paths, false).unwrap();
        assert!(!noop_result.changed);
        assert_eq!(noop_result.installed_count, 0);
    }
}
