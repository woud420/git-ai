use super::parse_git_cli_args;

#[test]
fn inverse_with_end_of_opts_roundtrips() {
    let args = vec!["-C", ".", "--", "--weird"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let parsed = parse_git_cli_args(&args);
    assert_eq!(parsed.global_args, vec!["-C".to_string(), ".".to_string()]);
    assert_eq!(parsed.command.as_deref(), Some("--weird"));
    assert!(parsed.saw_end_of_opts);
    assert_eq!(parsed.to_invocation_vec(), args);
}

#[test]
fn inverse_with_end_of_opts_no_command() {
    let args = vec!["-C", ".", "--"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let parsed = parse_git_cli_args(&args);
    assert_eq!(parsed.command, None);
    assert!(parsed.saw_end_of_opts);
    assert_eq!(parsed.to_invocation_vec(), args);
}

#[test]
fn inverse_simple_commit() {
    let args = vec!["-C", "..", "commit", "-m", "foo"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let parsed = parse_git_cli_args(&args);
    assert_eq!(parsed.to_invocation_vec(), args);
}

#[test]
fn inverse_meta_no_command() {
    let args = vec!["--version"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let parsed = parse_git_cli_args(&args);
    // global=[], command="versino", command_args=[]
    assert_eq!(parsed.global_args, Vec::<String>::new());
    assert_eq!(parsed.command, Some("version".into()));
    assert_eq!(parsed.command_args, Vec::<String>::new());
    assert_eq!(parsed.to_invocation_vec(), ["version".to_string()]);
}

#[test]
fn inverse_unknown_option_passthrough() {
    let args = vec!["--mystery", "status", "-s"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let parsed = parse_git_cli_args(&args);
    assert_eq!(parsed.command, None);
    assert_eq!(parsed.to_invocation_vec(), args);
}

#[test]
fn inverse_end_of_opts_note() {
    let args = vec!["-C", ".", "--", "--weird"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let parsed = parse_git_cli_args(&args);
    assert_eq!(parsed.global_args, vec!["-C".to_string(), ".".to_string()]);
    assert_eq!(parsed.command.as_deref(), Some("--weird"));
    assert_eq!(parsed.command_args, Vec::<String>::new());
    assert_eq!(
        parsed.to_invocation_vec(),
        vec![
            "-C".to_string(),
            ".".to_string(),
            "--".to_string(),
            "--weird".to_string()
        ]
    );
}
