use super::{ExpectedLineExt, TestRepo, fs};

#[test]
fn test_realistic_refactoring_sequence() {
    // Test a realistic code refactoring scenario with multiple human and AI edits
    let repo = TestRepo::new();
    let file_path = repo.path().join("calculator.rs");

    // Initial human-written code
    fs::write(
        &file_path,
        "pub struct Calculator {
    value: i32,
}

impl Calculator {
    pub fn new() -> Self {
        Self { value: 0 }
    }

    pub fn add(&mut self, n: i32) {
        self.value += n;
    }
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial calculator implementation")
        .unwrap();

    // AI adds subtract and multiply methods
    fs::write(
        &file_path,
        "pub struct Calculator {
    value: i32,
}

impl Calculator {
    pub fn new() -> Self {
        Self { value: 0 }
    }

    pub fn add(&mut self, n: i32) {
        self.value += n;
    }

    pub fn subtract(&mut self, n: i32) {
        self.value -= n;
    }

    pub fn multiply(&mut self, n: i32) {
        self.value *= n;
    }
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "calculator.rs"])
        .unwrap();
    repo.stage_all_and_commit("AI adds subtract and multiply")
        .unwrap();

    // Human refactors to add error handling
    fs::write(
        &file_path,
        "pub struct Calculator {
    value: i32,
}

impl Calculator {
    pub fn new() -> Self {
        Self { value: 0 }
    }

    pub fn add(&mut self, n: i32) -> Result<(), String> {
        self.value = self.value.checked_add(n).ok_or(\"Overflow\")?;
        Ok(())
    }

    pub fn subtract(&mut self, n: i32) {
        self.value -= n;
    }

    pub fn multiply(&mut self, n: i32) {
        self.value *= n;
    }
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds overflow check to add")
        .unwrap();

    // AI completes the refactoring for other methods
    fs::write(
        &file_path,
        "pub struct Calculator {
    value: i32,
}

impl Calculator {
    pub fn new() -> Self {
        Self { value: 0 }
    }

    pub fn add(&mut self, n: i32) -> Result<(), String> {
        self.value = self.value.checked_add(n).ok_or(\"Overflow\")?;
        Ok(())
    }

    pub fn subtract(&mut self, n: i32) -> Result<(), String> {
        self.value = self.value.checked_sub(n).ok_or(\"Underflow\")?;
        Ok(())
    }

    pub fn multiply(&mut self, n: i32) -> Result<(), String> {
        self.value = self.value.checked_mul(n).ok_or(\"Overflow\")?;
        Ok(())
    }
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "calculator.rs"])
        .unwrap();
    repo.stage_all_and_commit("AI adds error handling to other methods")
        .unwrap();

    // Verify final attribution aligns with git blame
    let mut file = repo.filename("calculator.rs");
    file.assert_lines_and_blame(crate::lines![
        "pub struct Calculator {".human(),
        "    value: i32,".human(),
        "}".human(),
        "".human(), // Line 4: empty line from original
        "impl Calculator {".human(),
        "    pub fn new() -> Self {".human(),
        "        Self { value: 0 }".human(),
        "    }".human(),
        "".human(), // Line 9: empty line from original
        "    pub fn add(&mut self, n: i32) -> Result<(), String> {".human(),
        "        self.value = self.value.checked_add(n).ok_or(\"Overflow\")?;".human(),
        "        Ok(())".human(),
        "    }".human(),
        "".ai(), // Line 14: empty line added when AI added subtract (git attributes to AI)
        "    pub fn subtract(&mut self, n: i32) -> Result<(), String> {".ai(),
        "        self.value = self.value.checked_sub(n).ok_or(\"Underflow\")?;".ai(),
        "        Ok(())".ai(),
        "    }".ai(),
        "".ai(), // Line 19: empty line from AI's additions
        "    pub fn multiply(&mut self, n: i32) -> Result<(), String> {".ai(),
        "        self.value = self.value.checked_mul(n).ok_or(\"Overflow\")?;".ai(),
        "        Ok(())".ai(),
        "    }".ai(),
        "}".human(),
    ]);
}

#[test]
fn test_realistic_refactoring_with_deletions() {
    // Test removing deprecated code - AI removes old API, human cleans up more
    let repo = TestRepo::new();
    let file_path = repo.path().join("api.rs");

    // Human creates initial API with old and new versions
    fs::write(
        &file_path,
        "// Legacy API - deprecated
pub fn process_data_v1(data: &str) -> String {
    data.to_uppercase()
}

pub fn process_data_v1_with_trim(data: &str) -> String {
    data.trim().to_uppercase()
}

// New API
pub fn process_data(data: &str) -> Result<String, String> {
    if data.is_empty() {
        return Err(\"Empty data\".to_string());
    }
    Ok(data.trim().to_uppercase())
}

// Helper function
pub fn validate_input(data: &str) -> bool {
    !data.is_empty()
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial API with legacy functions")
        .unwrap();

    // AI removes deprecated v1 functions
    fs::write(
        &file_path,
        "// New API
pub fn process_data(data: &str) -> Result<String, String> {
    if data.is_empty() {
        return Err(\"Empty data\".to_string());
    }
    Ok(data.trim().to_uppercase())
}

// Helper function
pub fn validate_input(data: &str) -> bool {
    !data.is_empty()
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "api.rs"]).unwrap();
    repo.stage_all_and_commit("AI removes deprecated v1 functions")
        .unwrap();

    // Human adds new function and improves validation
    fs::write(
        &file_path,
        "// New API
pub fn process_data(data: &str) -> Result<String, String> {
    if data.is_empty() {
        return Err(\"Empty data\".to_string());
    }
    Ok(data.trim().to_uppercase())
}

pub fn process_batch(items: &[&str]) -> Vec<Result<String, String>> {
    items.iter().map(|item| process_data(item)).collect()
}

// Helper function
pub fn validate_input(data: &str) -> bool {
    !data.trim().is_empty()
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds batch processing and improves validation")
        .unwrap();

    // AI removes comment and adds error type
    fs::write(
        &file_path,
        "pub type ProcessError = String;

pub fn process_data(data: &str) -> Result<String, ProcessError> {
    if data.is_empty() {
        return Err(\"Empty data\".to_string());
    }
    Ok(data.trim().to_uppercase())
}

pub fn process_batch(items: &[&str]) -> Vec<Result<String, ProcessError>> {
    items.iter().map(|item| process_data(item)).collect()
}

pub fn validate_input(data: &str) -> bool {
    !data.trim().is_empty()
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "api.rs"]).unwrap();
    repo.stage_all_and_commit("AI adds error type alias")
        .unwrap();

    // Verify attribution after deletions
    let mut file = repo.filename("api.rs");
    file.assert_lines_and_blame(crate::lines![
        "pub type ProcessError = String;".ai(),
        "".ai(),
        "pub fn process_data(data: &str) -> Result<String, ProcessError> {".ai(),
        "    if data.is_empty() {".human(),
        "        return Err(\"Empty data\".to_string());".human(),
        "    }".human(),
        "    Ok(data.trim().to_uppercase())".human(),
        "}".human(),
        "".human(),
        "pub fn process_batch(items: &[&str]) -> Vec<Result<String, ProcessError>> {".ai(),
        "    items.iter().map(|item| process_data(item)).collect()".human(),
        "}".human(),
        "".human(),
        "pub fn validate_input(data: &str) -> bool {".human(),
        "    !data.trim().is_empty()".human(),
        "}".human(),
    ]);
}

#[test]
fn test_realistic_formatting_and_whitespace_changes() {
    // Test code formatting changes - human writes compact, AI reformats, human adds features
    let repo = TestRepo::new();
    let file_path = repo.path().join("config.py");

    // Human writes compact Python config
    fs::write(
        &file_path,
        "class Config:
    def __init__(self):
        self.debug = False
        self.port = 8000
        self.host = \"localhost\"

    def get_url(self):
        return f\"http://{self.host}:{self.port}\"",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial compact config").unwrap();

    // AI reformats with better spacing and adds docstrings
    fs::write(
        &file_path,
        "class Config:
    \"\"\"Application configuration.\"\"\"

    def __init__(self):
        \"\"\"Initialize with default settings.\"\"\"
        self.debug = False
        self.port = 8000
        self.host = \"localhost\"

    def get_url(self):
        \"\"\"Get the full application URL.\"\"\"
        return f\"http://{self.host}:{self.port}\"",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "config.py"])
        .unwrap();
    repo.stage_all_and_commit("AI adds docstrings and formatting")
        .unwrap();

    // Human adds database config
    fs::write(
        &file_path,
        "class Config:
    \"\"\"Application configuration.\"\"\"

    def __init__(self):
        \"\"\"Initialize with default settings.\"\"\"
        self.debug = False
        self.port = 8000
        self.host = \"localhost\"
        self.db_url = \"sqlite:///app.db\"

    def get_url(self):
        \"\"\"Get the full application URL.\"\"\"
        return f\"http://{self.host}:{self.port}\"

    def get_database_url(self):
        return self.db_url",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds database config")
        .unwrap();

    // AI reformats new method with docstring
    fs::write(
        &file_path,
        "class Config:
    \"\"\"Application configuration.\"\"\"

    def __init__(self):
        \"\"\"Initialize with default settings.\"\"\"
        self.debug = False
        self.port = 8000
        self.host = \"localhost\"
        self.db_url = \"sqlite:///app.db\"

    def get_url(self):
        \"\"\"Get the full application URL.\"\"\"
        return f\"http://{self.host}:{self.port}\"

    def get_database_url(self):
        \"\"\"Get the database connection URL.\"\"\"
        return self.db_url",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "config.py"])
        .unwrap();
    repo.stage_all_and_commit("AI adds docstring to new method")
        .unwrap();

    // Verify attribution with whitespace changes
    let mut file = repo.filename("config.py");
    file.assert_lines_and_blame(crate::lines![
        "class Config:".human(),
        "    \"\"\"Application configuration.\"\"\"".ai(),
        "    ".ai(),
        "    def __init__(self):".human(),
        "        \"\"\"Initialize with default settings.\"\"\"".ai(),
        "        self.debug = False".human(),
        "        self.port = 8000".human(),
        "        self.host = \"localhost\"".human(),
        "        self.db_url = \"sqlite:///app.db\"".human(),
        "    ".human(), // Line 10: git attributes whitespace to human
        "    def get_url(self):".human(),
        "        \"\"\"Get the full application URL.\"\"\"".ai(),
        "        return f\"http://{self.host}:{self.port}\"".human(),
        "    ".human(), // Line 14: git attributes to human
        "    def get_database_url(self):".human(),
        "        \"\"\"Get the database connection URL.\"\"\"".ai(),
        "        return self.db_url".human(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_realistic_refactoring_sequence,
    test_realistic_refactoring_with_deletions,
    test_realistic_formatting_and_whitespace_changes,
);
