use super::{ExpectedLineExt, TestRepo, fs};

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

crate::reuse_tests_in_worktree!(
    test_realistic_api_endpoint_expansion,
    test_realistic_middleware_chain_development,
);
