use super::{ExpectedLineExt, TestRepo, fs};

#[test]
fn test_realistic_config_file_with_comments() {
    // Test AI and human editing a config file with comments
    let repo = TestRepo::new();
    let file_path = repo.path().join("config.toml");

    // Human creates initial config
    fs::write(
        &file_path,
        "[server]
host = \"localhost\"
port = 8080
",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial config").unwrap();

    // AI adds database config
    fs::write(
        &file_path,
        "[server]
host = \"localhost\"
port = 8080

[database]
url = \"postgresql://localhost/mydb\"
max_connections = 10
",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "config.toml"])
        .unwrap();
    repo.stage_all_and_commit("AI adds database config")
        .unwrap();

    // Human adds comments and changes port
    fs::write(
        &file_path,
        "# Server configuration
[server]
host = \"localhost\"
# Changed to use port 3000
port = 3000

[database]
url = \"postgresql://localhost/mydb\"
max_connections = 10
",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds comments and changes port")
        .unwrap();

    // AI adds logging config
    fs::write(
        &file_path,
        "# Server configuration
[server]
host = \"localhost\"
# Changed to use port 3000
port = 3000

[database]
url = \"postgresql://localhost/mydb\"
max_connections = 10

[logging]
level = \"info\"
format = \"json\"
",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "config.toml"])
        .unwrap();
    repo.stage_all_and_commit("AI adds logging config").unwrap();

    // Verify alignment with git
    let mut file = repo.filename("config.toml");
    file.assert_lines_and_blame(crate::lines![
        "# Server configuration".human(),
        "[server]".human(),
        "host = \"localhost\"".human(),
        "# Changed to use port 3000".human(),
        "port = 3000".human(),
        "".ai(), // Line 6: git attributes empty line to AI (inserted between sections)
        "[database]".ai(),
        "url = \"postgresql://localhost/mydb\"".ai(),
        "max_connections = 10".ai(),
        "".ai(),
        "[logging]".ai(),
        "level = \"info\"".ai(),
        "format = \"json\"".ai(),
    ]);
}

#[test]
fn test_realistic_jsx_component_development() {
    // Test AI and human building a React component together
    let repo = TestRepo::new();
    let file_path = repo.path().join("Button.jsx");

    // Human creates basic component
    fs::write(
        &file_path,
        "export function Button({ children }) {
  return <button>{children}</button>;
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial Button component")
        .unwrap();

    // AI adds onClick and styling props
    fs::write(
        &file_path,
        "export function Button({ children, onClick, className }) {
  return (
    <button onClick={onClick} className={className}>
      {children}
    </button>
  );
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "Button.jsx"])
        .unwrap();
    repo.stage_all_and_commit("AI adds onClick and className props")
        .unwrap();

    // Human adds variant prop with styles
    fs::write(
        &file_path,
        "export function Button({ children, onClick, className, variant = 'primary' }) {
  const baseStyles = 'px-4 py-2 rounded';
  const variantStyles = variant === 'primary' ? 'bg-blue-500 text-white' : 'bg-gray-200';

  return (
    <button onClick={onClick} className={`${baseStyles} ${variantStyles} ${className}`}>
      {children}
    </button>
  );
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds variant styling")
        .unwrap();

    // AI adds disabled state
    fs::write(
        &file_path,
        "export function Button({ children, onClick, className, variant = 'primary', disabled = false }) {
  const baseStyles = 'px-4 py-2 rounded';
  const variantStyles = variant === 'primary' ? 'bg-blue-500 text-white' : 'bg-gray-200';
  const disabledStyles = disabled ? 'opacity-50 cursor-not-allowed' : '';

  return (
    <button
      onClick={disabled ? undefined : onClick}
      className={`${baseStyles} ${variantStyles} ${disabledStyles} ${className}`}
      disabled={disabled}
    >
      {children}
    </button>
  );
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "Button.jsx"])
        .unwrap();
    repo.stage_all_and_commit("AI adds disabled state").unwrap();

    // Verify git blame alignment
    let mut file = repo.filename("Button.jsx");
    file.assert_lines_and_blame(crate::lines![
        "export function Button({ children, onClick, className, variant = 'primary', disabled = false }) {".ai(),
        "  const baseStyles = 'px-4 py-2 rounded';".human(),
        "  const variantStyles = variant === 'primary' ? 'bg-blue-500 text-white' : 'bg-gray-200';".human(),
        "  const disabledStyles = disabled ? 'opacity-50 cursor-not-allowed' : '';".ai(),
        "  ".human(),  // Line 5: git attributes whitespace-only line to human
        "  return (".ai(),
        "    <button".ai(),
        "      onClick={disabled ? undefined : onClick}".ai(),
        "      className={`${baseStyles} ${variantStyles} ${disabledStyles} ${className}`}".ai(),
        "      disabled={disabled}".ai(),
        "    >".ai(),
        "      {children}".ai(),
        "    </button>".ai(),
        "  );".ai(),
        "}".human(),  // Line 15: final closing brace stays human
    ]);
}

#[test]
fn test_realistic_class_with_multiple_methods() {
    // Test complex class evolution with multiple method additions and modifications
    let repo = TestRepo::new();
    let file_path = repo.path().join("UserManager.ts");

    // Human creates initial class with one method
    fs::write(
        &file_path,
        "export class UserManager {
  private users: Map<string, User> = new Map();

  constructor() {}

  addUser(user: User): void {
    this.users.set(user.id, user);
  }
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial UserManager class")
        .unwrap();

    // AI adds getUser and removeUser
    fs::write(
        &file_path,
        "export class UserManager {
  private users: Map<string, User> = new Map();

  constructor() {}

  addUser(user: User): void {
    this.users.set(user.id, user);
  }

  getUser(id: string): User | undefined {
    return this.users.get(id);
  }

  removeUser(id: string): boolean {
    return this.users.delete(id);
  }
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "UserManager.ts"])
        .unwrap();
    repo.stage_all_and_commit("AI adds getUser and removeUser")
        .unwrap();

    // Human refactors addUser to validate
    fs::write(
        &file_path,
        "export class UserManager {
  private users: Map<string, User> = new Map();

  constructor() {}

  addUser(user: User): void {
    if (!user.id || !user.email) {
      throw new Error('Invalid user');
    }
    this.users.set(user.id, user);
  }

  getUser(id: string): User | undefined {
    return this.users.get(id);
  }

  removeUser(id: string): boolean {
    return this.users.delete(id);
  }
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds validation to addUser")
        .unwrap();

    // AI adds updateUser method
    fs::write(
        &file_path,
        "export class UserManager {
  private users: Map<string, User> = new Map();

  constructor() {}

  addUser(user: User): void {
    if (!user.id || !user.email) {
      throw new Error('Invalid user');
    }
    this.users.set(user.id, user);
  }

  getUser(id: string): User | undefined {
    return this.users.get(id);
  }

  updateUser(id: string, updates: Partial<User>): User | undefined {
    const user = this.users.get(id);
    if (!user) return undefined;
    const updatedUser = { ...user, ...updates };
    this.users.set(id, updatedUser);
    return updatedUser;
  }

  removeUser(id: string): boolean {
    return this.users.delete(id);
  }
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "UserManager.ts"])
        .unwrap();
    repo.stage_all_and_commit("AI adds updateUser method")
        .unwrap();

    // Human adds getAllUsers and count
    fs::write(
        &file_path,
        "export class UserManager {
  private users: Map<string, User> = new Map();

  constructor() {}

  addUser(user: User): void {
    if (!user.id || !user.email) {
      throw new Error('Invalid user');
    }
    this.users.set(user.id, user);
  }

  getUser(id: string): User | undefined {
    return this.users.get(id);
  }

  getAllUsers(): User[] {
    return Array.from(this.users.values());
  }

  getUserCount(): number {
    return this.users.size;
  }

  updateUser(id: string, updates: Partial<User>): User | undefined {
    const user = this.users.get(id);
    if (!user) return undefined;
    const updatedUser = { ...user, ...updates };
    this.users.set(id, updatedUser);
    return updatedUser;
  }

  removeUser(id: string): boolean {
    return this.users.delete(id);
  }
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds getAllUsers and getUserCount")
        .unwrap();

    // Verify git blame alignment
    let mut file = repo.filename("UserManager.ts");
    file.assert_lines_and_blame(crate::lines![
        "export class UserManager {".human(),
        "  private users: Map<string, User> = new Map();".human(),
        "".human(),
        "  constructor() {}".human(),
        "".human(),
        "  addUser(user: User): void {".human(),
        "    if (!user.id || !user.email) {".human(),
        "      throw new Error('Invalid user');".human(),
        "    }".human(),
        "    this.users.set(user.id, user);".human(),
        "  }".human(),
        "".ai(), // Line 12: git attributes empty line to AI
        "  getUser(id: string): User | undefined {".ai(),
        "    return this.users.get(id);".ai(),
        "  }".ai(),
        "".ai(),
        "  getAllUsers(): User[] {".human(),
        "    return Array.from(this.users.values());".human(),
        "  }".human(),
        "".human(),
        "  getUserCount(): number {".human(),
        "    return this.users.size;".human(),
        "  }".human(),
        "".human(), // Line 24: human empty line
        "  updateUser(id: string, updates: Partial<User>): User | undefined {".ai(),
        "    const user = this.users.get(id);".ai(),
        "    if (!user) return undefined;".ai(),
        "    const updatedUser = { ...user, ...updates };".ai(),
        "    this.users.set(id, updatedUser);".ai(),
        "    return updatedUser;".ai(),
        "  }".ai(),
        "".ai(),
        "  removeUser(id: string): boolean {".ai(),
        "    return this.users.delete(id);".ai(),
        "  }".ai(),
        "}".human(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_realistic_config_file_with_comments,
    test_realistic_jsx_component_development,
    test_realistic_class_with_multiple_methods,
);
