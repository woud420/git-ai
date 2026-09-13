use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use std::fs;

mod refactoring;
mod test_and_schema_evolution;

#[test]
fn test_realistic_api_endpoint_expansion() {
    // Test AI expanding an API with multiple endpoints, with human edits in between
    let repo = TestRepo::new();
    let file_path = repo.path().join("handlers.rs");

    // Human writes initial GET endpoint
    fs::write(
        &file_path,
        "use axum::{Json, extract::Path};

pub async fn get_user(Path(id): Path<u32>) -> Json<User> {
    let user = fetch_user_from_db(id).await;
    Json(user)
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Add get_user endpoint").unwrap();

    // AI adds POST endpoint
    fs::write(
        &file_path,
        "use axum::{Json, extract::Path};

pub async fn get_user(Path(id): Path<u32>) -> Json<User> {
    let user = fetch_user_from_db(id).await;
    Json(user)
}

pub async fn create_user(Json(payload): Json<CreateUser>) -> Json<User> {
    let user = insert_user_to_db(payload).await;
    Json(user)
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "handlers.rs"])
        .unwrap();
    repo.stage_all_and_commit("AI adds create_user endpoint")
        .unwrap();

    // Human adds validation to create_user
    fs::write(
        &file_path,
        "use axum::{Json, extract::Path};

pub async fn get_user(Path(id): Path<u32>) -> Json<User> {
    let user = fetch_user_from_db(id).await;
    Json(user)
}

pub async fn create_user(Json(payload): Json<CreateUser>) -> Result<Json<User>, String> {
    if payload.username.is_empty() {
        return Err(\"Username cannot be empty\".to_string());
    }
    let user = insert_user_to_db(payload).await;
    Ok(Json(user))
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds validation").unwrap();

    // AI adds UPDATE and DELETE endpoints
    fs::write(
        &file_path,
        "use axum::{Json, extract::Path};

pub async fn get_user(Path(id): Path<u32>) -> Json<User> {
    let user = fetch_user_from_db(id).await;
    Json(user)
}

pub async fn create_user(Json(payload): Json<CreateUser>) -> Result<Json<User>, String> {
    if payload.username.is_empty() {
        return Err(\"Username cannot be empty\".to_string());
    }
    let user = insert_user_to_db(payload).await;
    Ok(Json(user))
}

pub async fn update_user(Path(id): Path<u32>, Json(payload): Json<UpdateUser>) -> Json<User> {
    let user = update_user_in_db(id, payload).await;
    Json(user)
}

pub async fn delete_user(Path(id): Path<u32>) -> Json<()> {
    delete_user_from_db(id).await;
    Json(())
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "handlers.rs"])
        .unwrap();
    repo.stage_all_and_commit("AI adds update and delete endpoints")
        .unwrap();

    // Human refactors error handling across all endpoints
    fs::write(
        &file_path,
        "use axum::{Json, extract::Path, http::StatusCode};

pub async fn get_user(Path(id): Path<u32>) -> Result<Json<User>, StatusCode> {
    let user = fetch_user_from_db(id).await?;
    Ok(Json(user))
}

pub async fn create_user(Json(payload): Json<CreateUser>) -> Result<Json<User>, StatusCode> {
    if payload.username.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let user = insert_user_to_db(payload).await?;
    Ok(Json(user))
}

pub async fn update_user(Path(id): Path<u32>, Json(payload): Json<UpdateUser>) -> Json<User> {
    let user = update_user_in_db(id, payload).await;
    Json(user)
}

pub async fn delete_user(Path(id): Path<u32>) -> Json<()> {
    delete_user_from_db(id).await;
    Json(())
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human refactors error handling")
        .unwrap();

    // Verify attribution aligns with git blame
    let mut file = repo.filename("handlers.rs");
    file.assert_lines_and_blame(crate::lines![
        "use axum::{Json, extract::Path, http::StatusCode};".human(),
        "".human(),
        "pub async fn get_user(Path(id): Path<u32>) -> Result<Json<User>, StatusCode> {".human(),
        "    let user = fetch_user_from_db(id).await?;".human(),
        "    Ok(Json(user))".human(),
        "}".ai(),  // Line 6: git attributes closing brace to AI due to AI adding next function
        "".ai(),
        "pub async fn create_user(Json(payload): Json<CreateUser>) -> Result<Json<User>, StatusCode> {".human(),
        "    if payload.username.is_empty() {".human(),
        "        return Err(StatusCode::BAD_REQUEST);".human(),
        "    }".human(),
        "    let user = insert_user_to_db(payload).await?;".human(),
        "    Ok(Json(user))".human(),
        "}".ai(),  // Line 14: git attributes closing brace to AI
        "".ai(),
        "pub async fn update_user(Path(id): Path<u32>, Json(payload): Json<UpdateUser>) -> Json<User> {".ai(),
        "    let user = update_user_in_db(id, payload).await;".ai(),
        "    Json(user)".ai(),
        "}".ai(),
        "".ai(),
        "pub async fn delete_user(Path(id): Path<u32>) -> Json<()> {".ai(),
        "    delete_user_from_db(id).await;".ai(),
        "    Json(())".ai(),
        "}".human(),  // Line 24: final closing brace stays human from original file
    ]);
}

#[test]
fn test_realistic_middleware_chain_development() {
    // Test building middleware with AI and human working together
    let repo = TestRepo::new();
    let file_path = repo.path().join("middleware.ts");

    // Human creates basic logging middleware
    fs::write(
        &file_path,
        "export function loggerMiddleware(req, res, next) {
  console.log(`${req.method} ${req.path}`);
  next();
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial logger middleware")
        .unwrap();

    // AI adds auth middleware
    fs::write(
        &file_path,
        "export function loggerMiddleware(req, res, next) {
  console.log(`${req.method} ${req.path}`);
  next();
}

export function authMiddleware(req, res, next) {
  const token = req.headers['authorization'];
  if (!token) {
    return res.status(401).json({ error: 'Unauthorized' });
  }
  next();
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "middleware.ts"])
        .unwrap();
    repo.stage_all_and_commit("AI adds auth middleware")
        .unwrap();

    // Human improves logging with timestamps
    fs::write(
        &file_path,
        "export function loggerMiddleware(req, res, next) {
  const timestamp = new Date().toISOString();
  console.log(`[${timestamp}] ${req.method} ${req.path}`);
  next();
}

export function authMiddleware(req, res, next) {
  const token = req.headers['authorization'];
  if (!token) {
    return res.status(401).json({ error: 'Unauthorized' });
  }
  next();
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds timestamps to logger")
        .unwrap();

    // AI adds rate limiting middleware
    fs::write(
        &file_path,
        "export function loggerMiddleware(req, res, next) {
  const timestamp = new Date().toISOString();
  console.log(`[${timestamp}] ${req.method} ${req.path}`);
  next();
}

export function authMiddleware(req, res, next) {
  const token = req.headers['authorization'];
  if (!token) {
    return res.status(401).json({ error: 'Unauthorized' });
  }
  next();
}

const rateLimitStore = new Map();

export function rateLimitMiddleware(limit = 100) {
  return (req, res, next) => {
    const ip = req.ip;
    const count = rateLimitStore.get(ip) || 0;
    if (count >= limit) {
      return res.status(429).json({ error: 'Rate limit exceeded' });
    }
    rateLimitStore.set(ip, count + 1);
    next();
  };
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint", "mock_ai", "middleware.ts"])
        .unwrap();
    repo.stage_all_and_commit("AI adds rate limiting").unwrap();

    // Human adds error handling middleware
    fs::write(
        &file_path,
        "export function loggerMiddleware(req, res, next) {
  const timestamp = new Date().toISOString();
  console.log(`[${timestamp}] ${req.method} ${req.path}`);
  next();
}

export function authMiddleware(req, res, next) {
  const token = req.headers['authorization'];
  if (!token) {
    return res.status(401).json({ error: 'Unauthorized' });
  }
  next();
}

const rateLimitStore = new Map();

export function rateLimitMiddleware(limit = 100) {
  return (req, res, next) => {
    const ip = req.ip;
    const count = rateLimitStore.get(ip) || 0;
    if (count >= limit) {
      return res.status(429).json({ error: 'Rate limit exceeded' });
    }
    rateLimitStore.set(ip, count + 1);
    next();
  };
}

export function errorHandlerMiddleware(err, req, res, next) {
  console.error('Error:', err);
  res.status(500).json({ error: 'Internal server error' });
}",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Human adds error handler")
        .unwrap();

    // Verify git alignment
    let mut file = repo.filename("middleware.ts");
    file.assert_lines_and_blame(crate::lines![
        "export function loggerMiddleware(req, res, next) {".human(),
        "  const timestamp = new Date().toISOString();".human(),
        "  console.log(`[${timestamp}] ${req.method} ${req.path}`);".human(),
        "  next();".human(),
        "}".ai(), // Line 5: git attributes closing brace to AI
        "".ai(),
        "export function authMiddleware(req, res, next) {".ai(),
        "  const token = req.headers['authorization'];".ai(),
        "  if (!token) {".ai(),
        "    return res.status(401).json({ error: 'Unauthorized' });".ai(),
        "  }".ai(),
        "  next();".ai(),
        "}".ai(), // Line 13: git attributes closing brace to AI
        "".ai(),
        "const rateLimitStore = new Map();".ai(),
        "".ai(),
        "export function rateLimitMiddleware(limit = 100) {".ai(),
        "  return (req, res, next) => {".ai(),
        "    const ip = req.ip;".ai(),
        "    const count = rateLimitStore.get(ip) || 0;".ai(),
        "    if (count >= limit) {".ai(),
        "      return res.status(429).json({ error: 'Rate limit exceeded' });".ai(),
        "    }".ai(),
        "    rateLimitStore.set(ip, count + 1);".ai(),
        "    next();".ai(),
        "  };".ai(),
        "}".human(), // Line 27: git attributes to Test User (human adds error handler after this)
        "".human(),
        "export function errorHandlerMiddleware(err, req, res, next) {".human(),
        "  console.error('Error:', err);".human(),
        "  res.status(500).json({ error: 'Internal server error' });".human(),
        "}".human(), // Line 32: final closing brace stays human
    ]);
}

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
    test_realistic_api_endpoint_expansion,
    test_realistic_middleware_chain_development,
    test_realistic_config_file_with_comments,
    test_realistic_jsx_component_development,
    test_realistic_class_with_multiple_methods,
);
