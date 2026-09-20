use crate::operations::git::{find_repository, notes_bundle};
use std::path::Path;

pub fn handle_notes_bundle(args: &[String]) {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        println!("git ai notes bundle <output.bundle> <full-commit-id>...");
        println!(
            "Export notes for 1 to {} selected commits into a companion Git bundle.",
            notes_bundle::MAX_COMMITS
        );
        println!(
            "Supports git_notes and sqlite; reports missing notes. Existing files are never overwritten."
        );
        return;
    }
    let result = (|| {
        let [output, commits @ ..] = args else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "usage: git ai notes bundle <output.bundle> <full-commit-id>...",
            )
            .into());
        };
        let repo = find_repository(&[])?;
        notes_bundle::export_bundle(&repo, Path::new(output), commits)
    })();
    match result {
        Ok((exported, missing)) => {
            println!("Exported {exported} note(s); {missing} selected commit(s) missing notes.")
        }
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
