#[derive(Debug, Clone, PartialEq)]
pub enum AuthorType {
    Human,
    UnattributedHuman,
    Ai,
}

#[derive(Debug, Clone)]
pub struct ExpectedLine {
    pub contents: String,
    pub author_type: AuthorType,
}

impl ExpectedLine {
    pub(super) fn new(contents: String, author_type: AuthorType) -> Self {
        if contents.contains('\n') {
            panic!(
                "fluent test file API does not support strings with new lines (must be a single line): {:?}",
                contents
            );
        }
        Self {
            contents,
            author_type,
        }
    }
}

/// Trait to add .ai(), .human(), and .unattributed_human() methods to string types
pub trait ExpectedLineExt {
    fn ai(self) -> ExpectedLine;
    fn human(self) -> ExpectedLine;
    fn unattributed_human(self) -> ExpectedLine;
}

impl ExpectedLineExt for &str {
    fn ai(self) -> ExpectedLine {
        ExpectedLine::new(self.to_string(), AuthorType::Ai)
    }

    fn human(self) -> ExpectedLine {
        ExpectedLine::new(self.to_string(), AuthorType::Human)
    }

    fn unattributed_human(self) -> ExpectedLine {
        ExpectedLine::new(self.to_string(), AuthorType::UnattributedHuman)
    }
}

impl ExpectedLineExt for String {
    fn ai(self) -> ExpectedLine {
        ExpectedLine::new(self, AuthorType::Ai)
    }

    fn human(self) -> ExpectedLine {
        ExpectedLine::new(self, AuthorType::Human)
    }

    fn unattributed_human(self) -> ExpectedLine {
        ExpectedLine::new(self, AuthorType::UnattributedHuman)
    }
}

impl ExpectedLineExt for ExpectedLine {
    fn ai(self) -> ExpectedLine {
        ExpectedLine::new(self.contents, AuthorType::Ai)
    }

    fn human(self) -> ExpectedLine {
        ExpectedLine::new(self.contents, AuthorType::Human)
    }

    fn unattributed_human(self) -> ExpectedLine {
        ExpectedLine::new(self.contents, AuthorType::UnattributedHuman)
    }
}

/// Default conversion from &str to ExpectedLine (defaults to Human authorship)
impl From<&str> for ExpectedLine {
    fn from(s: &str) -> Self {
        ExpectedLine::new(s.to_string(), AuthorType::Human)
    }
}

/// Default conversion from String to ExpectedLine (defaults to Human authorship)
impl From<String> for ExpectedLine {
    fn from(s: String) -> Self {
        ExpectedLine::new(s, AuthorType::Human)
    }
}
