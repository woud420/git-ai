use insta::assert_debug_snapshot;

use super::*;

#[test]
fn test_p_random_random_strings() {
    // These should be detected as random
    assert!(p_random(b"pk_test_TYooMQauvdEDq54NiTphI7jx") > 1.0 / 1e4);
    assert!(p_random(b"sk_test_4eC39HqLyjWDarjtT1zdp7dc") > 1.0 / 1e4);
}

#[test]
fn test_p_random_non_random_strings() {
    // These should NOT be detected as random
    assert!(p_random(b"hello_world") < 1.0 / 1e6);
    assert!(p_random(b"PROJECT_NAME_ALIAS") < 1.0 / 1e4);
}

#[test]
fn test_is_random() {
    // Secrets
    assert!(is_random(b"pk_test_TYooMQauvdEDq54NiTphI7jx"));
    assert!(is_random(b"sk_test_4eC39HqLyjWDarjtT1zdp7dc"));
    assert!(is_random(b"AKIAIOSFODNN7EXAMPLE"));

    // Not secrets
    assert!(!is_random(b"hello_world"));
    assert!(!is_random(b"my_variable_name"));
}

#[test]
fn test_extract_tokens() {
    let text = "API_KEY=sk_test_4eC39HqLyjWDarjtT1zdp7dc";
    let tokens = extract_tokens(text);
    assert!(!tokens.is_empty());
    // The token should be extracted (API_KEY is 7 chars, too short; the secret is 32 chars)
    assert!(
        tokens
            .iter()
            .any(|&(start, end)| &text[start..end] == "sk_test_4eC39HqLyjWDarjtT1zdp7dc")
    );
}

#[test]
fn test_redact_secret() {
    assert_eq!(
        redact_secret("sk_test_4eC39HqLyjWDarjtT1zdp7dc"),
        "sk_t********p7dc"
    );
    assert_eq!(redact_secret("AKIAIOSFODNN7EXAMPLE"), "AKIA********MPLE");
    assert_eq!(redact_secret("short"), "*****"); // Too short
}

#[test]
fn test_redact_secrets_in_text() {
    let text = "Set API_KEY=sk_test_4eC39HqLyjWDarjtT1zdp7dc in your config";
    let (redacted, count) = redact_secrets_in_text(text);
    assert!(!redacted.contains("sk_test_4eC39HqLyjWDarjtT1zdp7dc"));
    assert!(redacted.contains("sk_t********p7dc"));
    assert_eq!(count, 1);
}

#[test]
fn test_no_redaction_for_normal_text() {
    let text = "This is normal text without any secrets";
    let (redacted, count) = redact_secrets_in_text(text);
    assert_eq!(text, redacted);
    assert_eq!(count, 0);
}

#[test]
fn test_distinct_values() {
    assert_eq!(analyze_token(b"abca").distinct_count, 3);
    assert_eq!(analyze_token(b"aaaaaa").distinct_count, 1);
    assert_eq!(analyze_token(b"abcdef").distinct_count, 6);
}

#[test]
fn test_redact_secret_in_lorem_ipsum() {
    let text = concat!(
        "\n",
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor \n",
        "incididunt ut labore et dolore magna aliqua. Here is my API key: \n",
        "sk_live_51HG8vDKj2xPmVnRqT9wYzABC and you should use it carefully.\n",
        "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut \n",
        "aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \n",
        "voluptate velit esse cillum dolore eu fugiat nulla pariatur.\n",
    );
    let (redacted, count) = redact_secrets_in_text(text);

    // Secret should be redacted
    assert!(!redacted.contains("sk_live_51HG8vDKj2xPmVnRqT9wYzABC"));
    assert!(redacted.contains("sk_l********zABC"));
    assert_eq!(count, 1);

    // Rest of text should be intact
    assert!(redacted.contains("Lorem ipsum dolor sit amet"));
    assert!(redacted.contains("consectetur adipiscing elit"));
    assert!(redacted.contains("Here is my API key:"));
}

#[test]
fn test_redact_multiple_secrets_in_code() {
    let code = concat!(
        "\n",
        "use std::env;\n",
        "\n",
        "fn main() {\n",
        "    // Database credentials\n",
        "    let db_password = \"xK9mP2nQ7rS4tU6vW8yZ1aB3cD5eF7gH\";\n",
        "    \n",
        "    // API configuration\n",
        "    let stripe_key = \"sk_test_4eC39HqLyjWDarjtT1zdp7dc\";\n",
        "    let aws_key = \"AKIAIOSFODNN7EXAMPLE\";\n",
        "    \n",
        "    // Normal config values - should NOT be redacted\n",
        "    let app_name = \"my_application_name\";\n",
        "    let log_level = \"debug\";\n",
        "    let max_connections = 100;\n",
        "    \n",
        "    println!(\"Starting application...\");\n",
        "}\n",
    );
    let (redacted, count) = redact_secrets_in_text(code);

    // Secrets should be redacted
    assert!(!redacted.contains("xK9mP2nQ7rS4tU6vW8yZ1aB3cD5eF7gH"));
    assert!(!redacted.contains("sk_test_4eC39HqLyjWDarjtT1zdp7dc"));
    assert!(!redacted.contains("AKIAIOSFODNN7EXAMPLE"));
    assert_eq!(count, 3);

    // Normal identifiers should remain
    assert!(redacted.contains("my_application_name"));
    assert!(redacted.contains("debug"));
    assert!(redacted.contains("max_connections"));
    assert!(redacted.contains("println!"));
}

#[test]
fn test_redact_secret_in_json_config() {
    let json = r#"{
    "database": {
        "host": "localhost",
        "port": 5432,
        "password": "Rj7kL9mN2pQ4sT6vX8zA1bC3dE5fG7hI"
    },
    "api": {
        "endpoint": "https://api.example.com",
        "key": "pk_live_TYooMQauvdEDq54NiTphI7jx"
    },
    "logging": {
        "level": "info",
        "format": "json"
    }
}"#;
    let (redacted, count) = redact_secrets_in_text(json);

    // Secrets should be redacted
    assert!(!redacted.contains("Rj7kL9mN2pQ4sT6vX8zA1bC3dE5fG7hI"));
    assert!(!redacted.contains("pk_live_TYooMQauvdEDq54NiTphI7jx"));
    assert_eq!(count, 2);

    // Normal config should remain
    assert!(redacted.contains("localhost"));
    assert!(redacted.contains("5432"));
    assert!(redacted.contains("https://api.example.com"));
    assert!(redacted.contains("info"));
}

#[test]
fn test_redact_secret_in_env_file() {
    let env_content = r#"
# Application configuration
APP_NAME=my-cool-app
DEBUG=true
LOG_LEVEL=debug

# Secrets - these should be redacted
DATABASE_URL=postgres://user:pA5sW0rD9xK2mN7qR4tU6vY8zA1bC3dE@localhost:5432/mydb
STRIPE_SECRET_KEY=sk_live_51HG8vDKj2xPmVnRqT9wYzABC
AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE
JWT_SECRET=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9

# More normal config
PORT=3000
HOST=0.0.0.0
"#;
    let (redacted, count) = redact_secrets_in_text(env_content);

    println!("redacted: {}", redacted);
    assert_debug_snapshot!(redacted);
    // Secrets should be redacted
    assert!(!redacted.contains("pA5sW0rD9xK2mN7qR4tU6vY8zA1bC3dE"));
    assert!(!redacted.contains("sk_live_51HG8vDKj2xPmVnRqT9wYzABC"));
    assert!(!redacted.contains("AKIAIOSFODNN7EXAMPLE"));
    assert!(count >= 3); // At least 3 secrets

    // Normal values should remain
    assert!(redacted.contains("my-cool-app"));
    assert!(redacted.contains("DEBUG=true"));
    assert!(redacted.contains("PORT=3000"));
}

#[test]
fn test_no_false_positives_in_normal_code() {
    let code = r#"
pub fn calculate_total(items: &[Item]) -> f64 {
    items.iter().map(|item| item.price * item.quantity as f64).sum()
}

struct Configuration {
    database_host: String,
    database_port: u16,
    application_name: String,
    max_retry_attempts: u32,
}

impl Configuration {
    pub fn from_environment() -> Self {
        Self {
            database_host: std::env::var("DB_HOST").unwrap_or_default(),
            database_port: 5432,
            application_name: "my_service".to_string(),
            max_retry_attempts: 3,
        }
    }
}
"#;
    let (redacted, count) = redact_secrets_in_text(code);

    // Code should be completely unchanged - no false positives
    assert_eq!(code, redacted);
    assert_eq!(count, 0);
}
