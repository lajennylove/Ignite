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

/// Runs `ignite` with `stdin` piped in and the test database configured. The
/// working directory is the temp dir, so no `.env` file is read.
fn run_piped(args: &[&str], stdin: &str) -> std::process::Output {
    use std::io::Write;
    let url = std::env::var("DATABASE_URL").expect("set DATABASE_URL to the test database");
    let mut child = Command::new(env!("CARGO_BIN_EXE_ignite"))
        .args(args)
        .env("DATABASE_URL", url)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run the ignite binary");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .expect("write the password");
    child.wait_with_output().expect("wait for ignite")
}

/// An email that no other test or run uses. Tests run in parallel, and a clock can
/// give two calls the same reading, so a counter and the process id make it unique.
fn fresh_email() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    format!("stdin-{}-{n}@example.test", std::process::id())
}

#[test]
fn a_piped_password_creates_the_user_with_an_argon2_hash() {
    let email = fresh_email();
    let output = run_piped(
        &["user", "create", "--email", &email, "--password-stdin"],
        "piped-password-1\n",
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Created user"));

    let url = std::env::var("DATABASE_URL").unwrap();
    use diesel::Connection;
    let mut conn = diesel::mysql::MysqlConnection::establish(&url).unwrap();
    let user = ignite_panel::users::find_by_email(&mut conn, &email)
        .unwrap()
        .expect("created");
    assert!(user.password_hash.starts_with("$argon2id$"));
    assert!(ignite_panel::users::verify_password(
        &user.password_hash,
        "piped-password-1"
    ));
}

#[test]
fn a_non_terminal_standard_input_is_read_as_the_password_without_the_flag() {
    let email = fresh_email();
    let output = run_piped(&["user", "create", "--email", &email], "no-flag-password\n");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_short_piped_password_is_refused() {
    let output = run_piped(
        &[
            "user",
            "create",
            "--email",
            &fresh_email(),
            "--password-stdin",
        ],
        "short\n",
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("at least 8 characters"));
}

#[test]
fn an_empty_standard_input_is_refused() {
    let output = run_piped(
        &[
            "user",
            "create",
            "--email",
            &fresh_email(),
            "--password-stdin",
        ],
        "",
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("No password on standard input"));
}

#[test]
fn a_duplicate_email_is_refused_before_the_password_is_read() {
    let email = fresh_email();
    let first = run_piped(
        &["user", "create", "--email", &email, "--password-stdin"],
        "first-password\n",
    );
    assert_eq!(first.status.code(), Some(0));

    let second = run_piped(
        &["user", "create", "--email", &email, "--password-stdin"],
        "second-password\n",
    );
    assert_eq!(second.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&second.stderr).contains("already exists"));
}

#[test]
fn migrate_needs_database_url() {
    let output = Command::new(env!("CARGO_BIN_EXE_ignite"))
        .arg("migrate")
        .env_remove("DATABASE_URL")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .output()
        .expect("run the ignite binary");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("DATABASE_URL must be set."));
}

#[test]
fn an_unknown_command_prints_the_usage_and_exits_2() {
    let output = Command::new(env!("CARGO_BIN_EXE_ignite"))
        .arg("launch")
        .stdin(Stdio::null())
        .output()
        .expect("run the ignite binary");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ignite migrate"), "stderr: {stderr}");
}
