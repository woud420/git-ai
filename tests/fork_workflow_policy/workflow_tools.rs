use super::{
    BUNDLED_SKILL_FILES, DEVIN_REVIEW_BOT_NAME, GNU_MAKE_SETUP_ACTION, GNU_MAKE_SETUP_USE,
    GNU_MAKE_WORKFLOWS, GRAPHITE_ACTIVE_ROOTS, GRAPHITE_NAME, GRAPHITE_RETIRED_PATHS,
    GRAPHITE_RETIRED_WIRING, GRAPHITE_SCAN_EXCEPTIONS, Path, REQUIRED_MAKE_INTERFACE,
    REQUIRED_MAKE_TARGETS, RETIRED_SKILL_COMMANDS, REVIEW_PROCESS_FILES, TASK_MAINTAINED_SURFACES,
    TASK_RETIRED_FRAGMENTS, TASK_RETIRED_PATHS, collect_files, fs, is_repository_text_file,
};

// Regression coverage for ENG-351.
#[test]
fn eng_351_review_workflow_has_no_unused_bot_assumptions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = REVIEW_PROCESS_FILES
        .iter()
        .map(|path| root.join(path))
        .collect::<Vec<_>>();
    collect_files(&root.join(".github"), &mut files);
    files.sort();

    let mut violations = Vec::new();
    for path in files {
        if !is_repository_text_file(&path) {
            continue;
        }
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for (line_index, line) in contents.lines().enumerate() {
            if line.to_ascii_lowercase().contains(DEVIN_REVIEW_BOT_NAME) {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                violations.push(format!("{}:{}", relative.display(), line_index + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "fork review workflow still assumes an unused review bot:\n{}",
        violations.join("\n")
    );
}

// Regression coverage for ENG-352.
#[test]
fn eng_352_active_fork_surfaces_do_not_maintain_graphite_compatibility() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for relative in GRAPHITE_RETIRED_PATHS {
        if root.join(relative).exists() {
            violations.push(format!("{relative} (retired path still exists)"));
        }
    }
    for (relative, fragment) in GRAPHITE_RETIRED_WIRING {
        let path = root.join(relative);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        if contents.contains(fragment) {
            violations.push(format!("{relative} (retired wiring remains)"));
        }
    }

    let mut files = Vec::new();
    for relative in GRAPHITE_ACTIVE_ROOTS {
        let path = root.join(relative);
        if path.is_dir() {
            collect_files(&path, &mut files);
        } else {
            files.push(path);
        }
    }
    files.sort();
    files.dedup();

    for path in files {
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let relative_text = relative.to_string_lossy();
        if GRAPHITE_SCAN_EXCEPTIONS
            .iter()
            .any(|exception| relative == Path::new(exception))
            || !is_repository_text_file(&path)
        {
            continue;
        }

        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        if relative_text.to_ascii_lowercase().contains(GRAPHITE_NAME)
            || contents.to_ascii_lowercase().contains(GRAPHITE_NAME)
        {
            violations.push(relative_text.into_owned());
        }
    }

    assert!(
        violations.is_empty(),
        "active fork surfaces still maintain Graphite compatibility:\n{}",
        violations.join("\n")
    );
}

#[test]
fn eng_286_make_is_the_only_maintained_command_surface() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let makefile_path = root.join("Makefile");
    let makefile = fs::read_to_string(&makefile_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", makefile_path.display()));

    for relative in TASK_RETIRED_PATHS {
        assert!(
            !root.join(relative).exists(),
            "retired Task path still exists: {relative}"
        );
    }

    let declared_targets = makefile
        .lines()
        .filter(|line| !line.starts_with('\t') && !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once(':').map(|(targets, _)| targets))
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>();
    for target in REQUIRED_MAKE_TARGETS {
        assert!(
            declared_targets.contains(target),
            "Makefile is missing required target `{target}`"
        );
    }
    for fragment in REQUIRED_MAKE_INTERFACE {
        assert!(
            makefile.contains(fragment),
            "Makefile is missing required interface fragment `{fragment}`"
        );
    }

    let check_steps = ["$(MAKE) lint", "$(MAKE) format-check", "$(MAKE) test"];
    let mut previous = 0;
    for step in check_steps {
        let index = makefile
            .find(step)
            .unwrap_or_else(|| panic!("make check is missing sequential step `{step}`"));
        assert!(index >= previous, "make check steps are out of order");
        previous = index;
    }

    let mut violations = Vec::new();
    for relative in TASK_MAINTAINED_SURFACES {
        let path = root.join(relative);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for fragment in TASK_RETIRED_FRAGMENTS {
            if contents.contains(fragment) {
                violations.push(format!("{relative}: `{fragment}`"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "maintained surfaces still depend on Task:\n{}",
        violations.join("\n")
    );
}

#[test]
fn eng_286_requires_and_provisions_gnu_make_4_4_1() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let setup_path = root.join(GNU_MAKE_SETUP_ACTION);
    let setup = fs::read_to_string(&setup_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", setup_path.display()));

    for fragment in [
        "GNU_MAKE_VERSION=4.4.1",
        "dd16fb1d67bfab79a72f5e8390735c49e3e8e70b4945a15ab1f81ddb78658fb3",
        "brew install make",
        "choco install make --version=4.4.1 --no-progress --yes",
        "GNU Make 4.4.1",
    ] {
        assert!(
            setup.contains(fragment),
            "GNU Make setup action is missing `{fragment}`"
        );
    }

    let source_directory_index = setup
        .find("cd \"$source_dir\"")
        .expect("Linux setup must enter the extracted source directory");
    let configure_index = setup
        .find("./configure --prefix=\"$prefix\"")
        .expect("Linux setup must configure from the extracted source directory");
    assert!(
        source_directory_index < configure_index,
        "Linux setup must enter the extracted source directory before configuring"
    );

    for (relative, expected_uses) in GNU_MAKE_WORKFLOWS {
        let path = root.join(relative);
        let workflow = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        assert_eq!(
            workflow.matches(GNU_MAKE_SETUP_USE).count(),
            *expected_uses,
            "{relative} must set up GNU Make once per Make-running job"
        );
    }

    let contributing =
        fs::read_to_string(root.join("CONTRIBUTING.md")).expect("CONTRIBUTING.md must be readable");
    assert!(
        contributing.contains("GNU Make 4.4.1 or newer"),
        "contributor prerequisites must require GNU Make 4.4.1 or newer"
    );
    assert!(
        contributing.contains("$(brew --prefix make)/libexec/gnubin"),
        "macOS setup must document Homebrew's gnubin path"
    );
}

#[test]
fn eng_372_bundled_skills_reference_supported_commands() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut combined = String::new();
    let mut violations = Vec::new();

    for relative in BUNDLED_SKILL_FILES {
        let contents = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        for command in RETIRED_SKILL_COMMANDS {
            if contents.contains(command) {
                violations.push(format!("{relative}: `{command}`"));
            }
        }
        combined.push_str(&contents);
    }

    assert!(
        violations.is_empty(),
        "bundled skills still invoke retired commands:\n{}",
        violations.join("\n")
    );
    for command in [
        "git-ai blame",
        "git-ai show ",
        "git-ai show-prompt",
        "git-ai analyze",
    ] {
        assert!(
            combined.contains(command),
            "bundled skills do not document supported command `{command}`"
        );
    }
}
