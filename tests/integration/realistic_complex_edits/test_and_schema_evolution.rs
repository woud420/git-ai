use super::{ExpectedLineExt, TestRepo, fs};

#[test]
fn test_realistic_test_file_evolution() {
    // Test evolution of a test file with AI adding tests and human refactoring
    let repo = TestRepo::new();
    let file_path = repo.path().join("tests.rs");

    // Human writes initial test
    fs::write(
        &file_path,
        "#[test]
fn test_addition() {
    assert_eq!(2 + 2, 4);
}
",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial test").unwrap();

    // AI adds more test cases
    fs::write(
        &file_path,
        "#[test]
fn test_addition() {
    assert_eq!(2 + 2, 4);
}

#[test]
fn test_subtraction() {
    assert_eq!(5 - 3, 2);
}

#[test]
fn test_multiplication() {
    assert_eq!(3 * 4, 12);
}
",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "tests.rs"]).unwrap();
    repo.stage_all_and_commit("AI adds more tests").unwrap();

    // Human refactors to use test module
    fs::write(
        &file_path,
        "mod arithmetic_tests {
    #[test]
    fn test_addition() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn test_subtraction() {
        assert_eq!(5 - 3, 2);
    }

    #[test]
    fn test_multiplication() {
        assert_eq!(3 * 4, 12);
    }
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds module wrapper")
        .unwrap();

    // AI adds division test
    fs::write(
        &file_path,
        "mod arithmetic_tests {
    #[test]
    fn test_addition() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn test_subtraction() {
        assert_eq!(5 - 3, 2);
    }

    #[test]
    fn test_multiplication() {
        assert_eq!(3 * 4, 12);
    }

    #[test]
    fn test_division() {
        assert_eq!(12 / 3, 4);
    }
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "tests.rs"]).unwrap();
    repo.stage_all_and_commit("AI adds division test").unwrap();

    // Human adds edge case test
    fs::write(
        &file_path,
        "mod arithmetic_tests {
    #[test]
    fn test_addition() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn test_subtraction() {
        assert_eq!(5 - 3, 2);
    }

    #[test]
    fn test_multiplication() {
        assert_eq!(3 * 4, 12);
    }

    #[test]
    fn test_division() {
        assert_eq!(12 / 3, 4);
    }

    #[test]
    #[should_panic]
    fn test_division_by_zero() {
        let _ = 1 / 0;
    }
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds edge case test")
        .unwrap();

    // Verify git alignment
    // Without move detection, when human refactored to add module wrapper,
    // git attributes all indented lines to human, but blank lines stay with AI
    let mut file = repo.filename("tests.rs");
    file.assert_lines_and_blame(crate::lines![
        "mod arithmetic_tests {".human(),
        "    #[test]".human(),
        "    fn test_addition() {".human(),
        "        assert_eq!(2 + 2, 4);".human(),
        "    }".human(), // Line 5: attributed to human who added indentation
        "".ai(),         // Blank lines stay attributed to AI who originally added them
        "    #[test]".human(),
        "    fn test_subtraction() {".human(),
        "        assert_eq!(5 - 3, 2);".human(),
        "    }".human(),
        "".ai(), // Blank line stays with AI
        "    #[test]".human(),
        "    fn test_multiplication() {".human(),
        "        assert_eq!(3 * 4, 12);".human(),
        "    }".human(),
        "".ai(), // Blank line added by AI with division test
        "    #[test]".ai(),
        "    fn test_division() {".ai(),
        "        assert_eq!(12 / 3, 4);".ai(),
        "    }".ai(),
        "".human(), // Blank line added by human with edge case test
        "    #[test]".human(),
        "    #[should_panic]".human(),
        "    fn test_division_by_zero() {".human(),
        "        let _ = 1 / 0;".human(),
        "    }".human(),
        "}".human(), // Line 27: closing brace from human's module wrapper
    ]);
}

#[test]
fn test_realistic_sql_migration_sequence() {
    // Test AI and human collaborating on database migrations
    let repo = TestRepo::new();
    let file_path = repo.path().join("001_initial.sql");

    // Human creates initial users table
    fs::write(
        &file_path,
        "-- Initial migration
CREATE TABLE users (
  id SERIAL PRIMARY KEY,
  email VARCHAR(255) NOT NULL
);
",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial migration").unwrap();

    // AI adds indexes and constraints
    fs::write(
        &file_path,
        "-- Initial migration
CREATE TABLE users (
  id SERIAL PRIMARY KEY,
  email VARCHAR(255) NOT NULL,
  UNIQUE(email)
);

CREATE INDEX idx_users_email ON users(email);
",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "001_initial.sql"])
        .unwrap();
    repo.stage_all_and_commit("AI adds indexes and constraints")
        .unwrap();

    // Human adds created_at column
    fs::write(
        &file_path,
        "-- Initial migration
CREATE TABLE users (
  id SERIAL PRIMARY KEY,
  email VARCHAR(255) NOT NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(email)
);

CREATE INDEX idx_users_email ON users(email);
",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds created_at").unwrap();

    // AI adds posts table with foreign key
    fs::write(
        &file_path,
        "-- Initial migration
CREATE TABLE users (
  id SERIAL PRIMARY KEY,
  email VARCHAR(255) NOT NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(email)
);

CREATE INDEX idx_users_email ON users(email);

CREATE TABLE posts (
  id SERIAL PRIMARY KEY,
  user_id INTEGER NOT NULL,
  title VARCHAR(255) NOT NULL,
  content TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_posts_user_id ON posts(user_id);
",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "001_initial.sql"])
        .unwrap();
    repo.stage_all_and_commit("AI adds posts table").unwrap();

    // Verify alignment
    let mut file = repo.filename("001_initial.sql");
    file.assert_lines_and_blame(crate::lines![
        "-- Initial migration".human(),
        "CREATE TABLE users (".human(),
        "  id SERIAL PRIMARY KEY,".human(),
        "  email VARCHAR(255) NOT NULL,".ai(), // Line 4: git attributes to AI due to adding UNIQUE constraint
        "  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,".human(),
        "  UNIQUE(email)".ai(),
        // @todo this is caused by last line diff bug.
        // started showing up when we toggled off move feature flag
        ");".human(),
        "".ai(),
        "CREATE INDEX idx_users_email ON users(email);".ai(),
        "".ai(),
        "CREATE TABLE posts (".ai(),
        "  id SERIAL PRIMARY KEY,".ai(),
        "  user_id INTEGER NOT NULL,".ai(),
        "  title VARCHAR(255) NOT NULL,".ai(),
        "  content TEXT,".ai(),
        "  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,".ai(),
        "  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE".ai(),
        ");".ai(),
        "".ai(),
        "CREATE INDEX idx_posts_user_id ON posts(user_id);".ai(),
    ]);
}

#[test]
fn test_realistic_multi_file_commit() {
    // Test editing multiple related files in a single commit
    let repo = TestRepo::new();
    let model_path = repo.path().join("models.rs");
    let handler_path = repo.path().join("handlers.rs");
    let schema_path = repo.path().join("schema.sql");

    // Human creates initial model
    fs::write(
        &model_path,
        "pub struct User {
    pub id: i32,
    pub name: String,
}",
    )
    .unwrap();

    fs::write(
        &handler_path,
        "use crate::models::User;

pub fn get_user(id: i32) -> Option<User> {
    None
}",
    )
    .unwrap();

    fs::write(
        &schema_path,
        "CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial user model and schema")
        .unwrap();

    // AI adds email field to all three files
    fs::write(
        &model_path,
        "pub struct User {
    pub id: i32,
    pub name: String,
    pub email: String,
}",
    )
    .unwrap();

    fs::write(
        &handler_path,
        "use crate::models::User;

pub fn get_user(id: i32) -> Option<User> {
    None
}

pub fn get_user_by_email(email: &str) -> Option<User> {
    None
}",
    )
    .unwrap();

    fs::write(
        &schema_path,
        "CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT UNIQUE NOT NULL
);

CREATE INDEX idx_users_email ON users(email);",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();
    repo.stage_all_and_commit("AI adds email field across all files")
        .unwrap();

    // Human adds validation
    fs::write(
        &model_path,
        "pub struct User {
    pub id: i32,
    pub name: String,
    pub email: String,
}

impl User {
    pub fn validate_email(&self) -> bool {
        self.email.contains('@')
    }
}",
    )
    .unwrap();

    fs::write(
        &handler_path,
        "use crate::models::User;

pub fn get_user(id: i32) -> Option<User> {
    None
}

pub fn get_user_by_email(email: &str) -> Option<User> {
    if !email.contains('@') {
        return None;
    }
    None
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds email validation")
        .unwrap();

    // Verify models.rs
    let mut models_file = repo.filename("models.rs");
    models_file.assert_lines_and_blame(crate::lines![
        "pub struct User {".human(),
        "    pub id: i32,".human(),
        "    pub name: String,".human(),
        "    pub email: String,".ai(),
        "}".human(), // Line 5: git attributes closing brace to human (impl added after)
        "".human(),
        "impl User {".human(),
        "    pub fn validate_email(&self) -> bool {".human(),
        "        self.email.contains('@')".human(),
        "    }".human(),
        "}".human(), // Line 11: stays human
    ]);

    // Verify handlers.rs
    let mut handlers_file = repo.filename("handlers.rs");
    handlers_file.assert_lines_and_blame(crate::lines![
        "use crate::models::User;".human(),
        "".human(),
        "pub fn get_user(id: i32) -> Option<User> {".human(),
        "    None".human(),
        "}".ai(), // Line 5: git attributes closing brace to AI (next function added by AI)
        "".ai(),
        "pub fn get_user_by_email(email: &str) -> Option<User> {".ai(),
        "    if !email.contains('@') {".human(),
        "        return None;".human(),
        "    }".human(),
        "    None".ai(),
        "}".human(), // Line 12: final closing brace stays human
    ]);

    // Verify schema.sql
    let mut schema_file = repo.filename("schema.sql");
    schema_file.assert_lines_and_blame(crate::lines![
        "CREATE TABLE users (".human(),
        "    id INTEGER PRIMARY KEY,".human(),
        "    name TEXT NOT NULL,".ai(), // Line 3: git attributes to AI (comma added)
        "    email TEXT UNIQUE NOT NULL".ai(),
        ");".ai(),
        "".ai(),
        "CREATE INDEX idx_users_email ON users(email);".ai(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_realistic_test_file_evolution,
    test_realistic_sql_migration_sequence,
    test_realistic_multi_file_commit,
);
