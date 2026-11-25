use super::*;

mod commit_sequence;
mod complex_changes;

#[derive(Clone, Copy)]
enum HumanContextAttribution {
    Known,
    Unattributed,
}

impl HumanContextAttribution {
    fn expected_line(self, contents: &str) -> ExpectedLine {
        match self {
            Self::Known => contents.human(),
            Self::Unattributed => contents.unattributed_human(),
        }
    }

    fn assert_metadata_humans(self, repo: &TestRepo, commit: &str, context: &str) {
        if matches!(self, Self::Unattributed) {
            return;
        }

        let conflict_note = parse_note(repo, commit);
        assert!(
            conflict_note
                .metadata
                .humans
                .contains_key("h_e858f2c2faea28"),
            "{context} should have h_e858f2c2faea28 in metadata.humans (human context lines in resolved file)"
        );
        assert_eq!(
            conflict_note.metadata.humans["h_e858f2c2faea28"].author,
            "Test User <test@example.com>"
        );
    }
}
