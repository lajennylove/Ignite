//! The `ignite` command checks its configuration before it asks for a password.

use std::process::{Command, Stdio};

#[test]
fn the_cli_checks_database_url_before_it_prompts() {
    // With no DATABASE_URL the command must fail at once. A prompt would wait on
    // the terminal, which a test cannot answer.
    let output = Command::new(env!("CARGO_BIN_EXE_ignite"))
        .args(["user", "create", "--email", "cli-test@example.test"])
        .env_remove("DATABASE_URL")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .output()
        .expect("run the ignite binary");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("DATABASE_URL must be set."),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains("Password"),
        "the password prompt must not run first"
    );
}

#[test]
fn the_cli_requires_an_email_before_anything_else() {
    let output = Command::new(env!("CARGO_BIN_EXE_ignite"))
        .args(["user", "create"])
        .env_remove("DATABASE_URL")
        .stdin(Stdio::null())
        .output()
        .expect("run the ignite binary");

    assert_eq!(output.status.code(), Some(2));
}
