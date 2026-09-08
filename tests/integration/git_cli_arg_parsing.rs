use git_ai::operations::git::cli_parser::parse_git_cli_args;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

mod command_selection;
mod global_options;
mod help_and_version;
mod inverse_arguments;
