use super::CodexInstaller;
use super::config_edit::update_config;
use super::tests::{test_binary_path, with_temp_home};
use crate::operations::mdm::hook_installer::{HookInstaller, HookInstallerParams};
use serial_test::serial;
use std::fs;
use toml::Value;

#[test]
#[serial]
fn install_preserves_config_text_and_custom_hook_comments() {
    with_temp_home(|home| {
        let config_path = home.join(".codex/config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        let original = "# My settings\nmodel = 'gpt-5' # pinned\n\n[features]\n# Search stays enabled\nweb_search_request = true\n\n# My hook\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'notify-send done' # keep this note\n";
        fs::write(&config_path, original).unwrap();
        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };

        installer.install_hooks(&params, true).unwrap();
        assert_eq!(fs::read_to_string(&config_path).unwrap(), original);
        installer.install_hooks(&params, false).unwrap();
        let installed = fs::read_to_string(&config_path).unwrap();
        assert!(installed.contains("model = 'gpt-5' # pinned\n"));
        assert!(installed.contains("# Search stays enabled\nweb_search_request = true\n"));
        assert!(installed.contains("# My hook\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'notify-send done' # keep this note\n"));
        assert!(CodexInstaller::config_has_inline_hooks(
            &CodexInstaller::parse_config_toml(&installed).unwrap()
        ));
        assert!(installer.install_hooks(&params, false).unwrap().is_none());
        assert_eq!(fs::read_to_string(&config_path).unwrap(), installed);
        installer.uninstall_hooks(&params, true).unwrap();
        assert_eq!(fs::read_to_string(&config_path).unwrap(), installed);
        installer.uninstall_hooks(&params, false).unwrap();
        let uninstalled = fs::read_to_string(&config_path).unwrap();
        assert!(uninstalled.contains("model = 'gpt-5' # pinned\n"));
        assert!(uninstalled.contains("# Search stays enabled\nweb_search_request = true\n"));
        assert!(uninstalled.contains("# My hook\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'notify-send done' # keep this note\n"));
        assert!(!uninstalled.contains("checkpoint codex"));
    });
}

fn install_and_remove(source: &str) -> (String, String) {
    let before = CodexInstaller::parse_config_toml(source).unwrap();
    let desired =
        CodexInstaller::config_with_installed_hooks(&before, &test_binary_path()).unwrap();
    let installed = update_config(source, &desired).unwrap();
    assert_eq!(
        CodexInstaller::parse_config_toml(&installed).unwrap(),
        desired
    );
    assert_eq!(update_config(&installed, &desired).unwrap(), installed);
    let (without_hooks, _) = CodexInstaller::remove_inline_hooks_from_config(&desired).unwrap();
    let removed = CodexInstaller::remove_feature_flags(&without_hooks).unwrap();
    let uninstalled = update_config(&installed, &removed).unwrap();
    assert_eq!(
        CodexInstaller::parse_config_toml(&uninstalled).unwrap(),
        removed
    );
    (installed, uninstalled)
}

#[test]
fn preserves_unrelated_text_across_toml_representations() {
    let fixtures = [
        "# settings\nmodel = 'gpt-5' # pinned\n",
        "# settings\r\nmodel = 'gpt-5' # pinned\r\n",
        "model = 'gpt-5' # no final newline",
        "model = '''\n[features]\nhooks = false\n''' # a string, not settings\n",
        "model = 'gpt-5'\nfeatures.codex_hooks = true # legacy\nfeatures.other = true # keep dotted\n",
        "model = 'gpt-5'\n'features'.'codex_hooks' = true\n'features'.'other' = true # keep quoted\n",
        "model = 'gpt-5'\nfeatures = { codex_hooks = true, other = true } # inline settings\n",
        "model = 'gpt-5'\n[features]\nhooks = false # keep this comment\nother = true # keep value\n",
        "model = 'gpt-5'\nnotify = [\n  '/old/git-ai', # binary\n  'checkpoint', 'codex', '--hook-input'\n]\n[features]\nother = true # keep value\n",
        "",
        "# only a comment",
        "\u{feff}features.other = true # keep BOM\n",
        "[features]\n[unrelated]\nname = 'keep' # keep empty table neighbor\n",
    ];
    for source in fixtures {
        let (installed, uninstalled) = install_and_remove(source);
        if source.starts_with('\u{feff}') {
            assert!(installed.starts_with('\u{feff}'));
            assert!(uninstalled.starts_with('\u{feff}'));
        }
        for line in source
            .lines()
            .map(|line| line.trim_start_matches('\u{feff}'))
            .filter(|line| {
                line.starts_with("model =")
                    || line.contains("# keep")
                    || line.contains("# pinned")
                    || line.contains("# inline settings")
            })
        {
            if line.contains("codex_hooks") || line.starts_with("hooks =") {
                assert!(
                    installed.contains(line.split('#').nth(1).unwrap()),
                    "{installed}"
                );
                assert!(
                    uninstalled.contains(line.split('#').nth(1).unwrap()),
                    "{uninstalled}"
                );
            } else {
                assert!(installed.contains(line), "missing {line:?} in {installed}");
                assert!(
                    uninstalled.contains(line),
                    "missing {line:?} in {uninstalled}"
                );
            }
        }
        if source.contains("\r\n") {
            assert!(!installed.replace("\r\n", "").contains('\n'));
            assert!(!uninstalled.replace("\r\n", "").contains('\n'));
        }
    }
}

#[test]
fn preserves_non_finite_floats_and_unicode() {
    let source = "# café\ncustom = nan # user setting\nmodel = '模型'\n# Custom hook\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ncommand = 'notify-send done'\ncustom_weight = nan # keep\n";
    let before = CodexInstaller::parse_config_toml(source).unwrap();
    let desired =
        CodexInstaller::config_with_installed_hooks(&before, &test_binary_path()).unwrap();
    let updated = update_config(source, &desired).unwrap();
    assert!(updated.contains(source.split("# Custom hook").next().unwrap()));
    assert!(updated.contains("# Custom hook\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ncommand = 'notify-send done'\ncustom_weight = nan # keep\n"));
    assert_eq!(update_config(&updated, &desired).unwrap(), updated);
}

#[test]
fn preserves_custom_hooks_before_and_after_managed_hooks_and_groups() {
    let managed = "[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = '/old/git-ai checkpoint codex --hook-input stdin'\n";
    let custom = "# Custom hook\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'notify-send done' # keep custom\n";
    for source in [
        format!("[[hooks.Stop]]\n{managed}{custom}"),
        format!("[[hooks.Stop]]\n{custom}{managed}"),
        format!("[[hooks.Stop]]\n{managed}# Custom group\n[[hooks.Stop]]\n{custom}"),
        format!("# Custom group\n[[hooks.Stop]]\n{custom}[[hooks.Stop]]\n{managed}"),
        format!(
            "[[hooks.Stop]]\nmatcher = 'Bash'\n{managed}# Custom group\n[[hooks.Stop]]\nmatcher = 'Read'\n{custom}"
        ),
    ] {
        let (installed, uninstalled) = install_and_remove(&source);
        for output in [&installed, &uninstalled] {
            assert!(output.contains(custom), "{output}");
            assert_eq!(output.matches("# Custom hook").count(), 1);
            if source.contains("# Custom group") {
                assert!(
                    output.contains("# Custom group\n[[hooks.Stop]]"),
                    "{output}"
                );
            }
        }
        assert!(!uninstalled.contains("checkpoint codex"));
    }
}

#[test]
fn retains_each_commented_duplicate_custom_group() {
    let group = "[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ncommand = 'notify-send done'\n";
    let command = Value::String(CodexInstaller::desired_command(&test_binary_path()));
    let source = format!(
        "# First group\n{group}# Second group\n{group}[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = {command}\n"
    );
    let (installed, uninstalled) = install_and_remove(&source);
    for output in [&installed, &uninstalled] {
        assert!(
            output.contains(&format!("# First group\n{group}")),
            "{output}"
        );
        assert!(
            output.contains(&format!("# Second group\n{group}")),
            "{output}"
        );
    }
}

#[test]
fn preserves_inline_hook_entries_and_comments_in_multiline_arrays() {
    let managed =
        "{ type = 'command', command = '/old/git-ai checkpoint codex --hook-input stdin' }";
    let custom = "{ type = 'command', command = 'notify-send done' }";
    for source in [
        format!(
            "[[hooks.Stop]]\nhooks = [\n{managed}, # managed\n# Custom hook, including a comma\n{custom}, # keep\n]\n"
        ),
        format!("[[hooks.Stop]]\nhooks = [\n# Custom hook\n{custom}, # keep\n{managed},\n]\n"),
        format!(
            "[[hooks.Stop]]\nhooks = [\n{custom}, # first\n{managed},\n{{ command = 'echo second' }}, # second\n]\n"
        ),
        format!("hooks = {{ Stop = [ {{ hooks = [{managed}, {custom}] }} ] }} # keep inline\n"),
        format!(
            "hooks.Stop = [\n{{ hooks = [{managed}] }},\n# Custom group\n{{ hooks = [{custom}] }},\n]\n"
        ),
    ] {
        let (installed, uninstalled) = install_and_remove(&source);
        for output in [&installed, &uninstalled] {
            assert!(output.contains(custom), "{output}");
            if source.contains(&format!("{custom}, # keep")) {
                assert!(output.contains(&format!("{custom}, # keep")), "{output}");
            }
            if source.contains(&format!("{custom}, # first")) {
                assert!(output.contains(&format!("{custom}, # first")), "{output}");
                assert!(
                    output.contains("{ command = 'echo second' }, # second"),
                    "{output}"
                );
            }
            for line in source.lines().filter(|line| line.starts_with("# Custom")) {
                assert!(output.contains(line), "{output}");
            }
        }
        assert!(!uninstalled.contains("checkpoint codex"));
    }
}

#[test]
#[serial]
fn preserves_text_during_both_hook_format_migrations() {
    with_temp_home(|home| {
        let config_path = home.join(".codex/config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        let original = "# My settings\nmodel = 'gpt-5' # pinned\n[features]\ncodex_hooks = true # old flag\nother = true # keep\n# Custom hook\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ncommand = 'notify-send done' # keep\n";
        fs::write(&config_path, original).unwrap();
        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };
        installer.install_hooks(&params, false).unwrap();
        fs::create_dir_all(home.join(".git-ai")).unwrap();
        fs::write(
            home.join(".git-ai/config.json"),
            r#"{"codex_hooks_format":"hooks_json"}"#,
        )
        .unwrap();
        installer.install_hooks(&params, false).unwrap();
        let json_mode = fs::read_to_string(&config_path).unwrap();
        assert!(!json_mode.contains("checkpoint codex"));
        assert!(home.join(".codex/hooks.json").exists());
        fs::write(
            home.join(".git-ai/config.json"),
            r#"{"codex_hooks_format":"inline"}"#,
        )
        .unwrap();
        installer.install_hooks(&params, false).unwrap();
        let inline_mode = fs::read_to_string(&config_path).unwrap();
        assert!(CodexInstaller::config_has_inline_hooks(
            &CodexInstaller::parse_config_toml(&inline_mode).unwrap()
        ));
        assert!(!home.join(".codex/hooks.json").exists());
        for output in [&json_mode, &inline_mode] {
            assert!(output.contains("# My settings\nmodel = 'gpt-5' # pinned\n"));
            assert!(output.contains("other = true # keep\n"));
            assert!(output.contains("# Custom hook\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ncommand = 'notify-send done' # keep\n"));
        }
    });
}

#[test]
#[serial]
fn invalid_configs_do_not_write_config() {
    with_temp_home(|home| {
        let config_path = home.join(".codex/config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        let installer = CodexInstaller;
        let params = HookInstallerParams {
            binary_path: test_binary_path(),
        };
        for source in ["model = [", "[features]\nhooks = true\nhooks = false\n"] {
            fs::write(&config_path, source).unwrap();
            assert!(installer.install_hooks(&params, false).is_err());
            assert_eq!(fs::read_to_string(&config_path).unwrap(), source);
        }
        assert!(update_config("# comment\n", &Value::Boolean(true)).is_err());
    });
}

#[test]
fn repairs_non_array_hook_fields_using_existing_transformations() {
    let source = "model = 'gpt-5' # keep\nhooks.Stop.unexpected = 'not an array'\n";
    let (installed, uninstalled) = install_and_remove(source);
    assert!(installed.contains("model = 'gpt-5' # keep"));
    assert!(uninstalled.contains("model = 'gpt-5' # keep"));
}
