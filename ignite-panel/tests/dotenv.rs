//! `Ignite::new` reads a `.env` file from the working directory, and never overrides a
//! variable that is already set in the environment, as Laravel does. A missing `.env` is
//! fine; a `.env` that cannot be parsed stops the app. The tests run the real `Ignite::new`
//! in a child process whose working directory is a temporary directory, because the child's
//! environment and files are the only things that can be controlled.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MODE: &str = "IGNITE_DOTENV_CHILD_MODE";
const TEST: &str = "ignite_new_follows_the_dot_env_rules";

/// A fresh temporary directory, named for this test process and the case.
fn scratch(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ignite-dotenv-{case}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs this test again as a child in `dir`, in `mode`, with the test database in the
/// environment (unless the case removes it).
fn run_child(mode: &str, dir: &Path, extra_env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env(MODE, mode)
        .env("DATABASE_URL", common::database_url())
        .env_remove("IGNITE_SECRET_KEY")
        .env_remove("IGNITE_DOTENV_MARKER")
        .current_dir(dir);
    for (name, value) in extra_env {
        command.env(name, value);
    }
    command.output().expect("run the test again as a child")
}

#[test]
fn ignite_new_follows_the_dot_env_rules() {
    match std::env::var(MODE).ok().as_deref() {
        Some("valid") => child_valid(),
        Some("missing") => child_builds(),
        Some("malformed") => child_builds(),
        _ => parent(),
    }
}

fn parent() {
    // A valid file: loaded, and never overriding the environment.
    let dir = scratch("valid");
    std::fs::write(
        dir.join(".env"),
        format!(
            "DATABASE_URL={}\nIGNITE_SECRET_KEY={}\nIGNITE_TRUSTED_PROXIES=192.0.2.1\nIGNITE_DOTENV_MARKER=from-file\n",
            common::database_url(),
            common::random_key()
        ),
    )
    .unwrap();
    let output = run_child(
        "valid",
        &dir,
        // Set in the environment, so it must win over the file.
        &[("IGNITE_TRUSTED_PROXIES", "10.0.0.0/8")],
    );
    assert_success(&output, "a valid .env");
    let _ = std::fs::remove_dir_all(&dir);

    // No .env at all: no error, and the environment is used.
    let dir = scratch("missing");
    let output = run_child("missing", &dir, &[]);
    assert_success(&output, "a missing .env");
    let _ = std::fs::remove_dir_all(&dir);

    // A .env that cannot be parsed: the app stops, and says why.
    let dir = scratch("malformed");
    std::fs::write(dir.join(".env"), "DATABASE_URL=\"never closed\n").unwrap();
    let output = run_child("malformed", &dir, &[]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        !output.status.success(),
        "a malformed .env must stop the app\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Ignite could not read .env"),
        "the panic names the .env file: {stderr}"
    );
}

fn assert_success(output: &Output, case: &str) {
    assert!(
        output.status.success(),
        "{case}: child failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The child with a valid `.env`: builds the panel, then checks the process environment.
fn child_valid() {
    let _ignite = ignite_panel::Ignite::new();

    assert_eq!(
        std::env::var("IGNITE_DOTENV_MARKER").as_deref(),
        Ok("from-file"),
        "a variable only in .env is loaded"
    );
    assert_eq!(
        std::env::var("DATABASE_URL").ok(),
        Some(common::database_url()),
        "DATABASE_URL comes from the environment or .env"
    );
    assert_eq!(
        std::env::var("IGNITE_TRUSTED_PROXIES").as_deref(),
        Ok("10.0.0.0/8"),
        "a variable already in the environment is not overridden by .env"
    );
}

/// The child for the missing and malformed cases. A missing file builds the panel; a
/// malformed file panics in `Ignite::new` before it gets this far.
fn child_builds() {
    let _ignite = ignite_panel::Ignite::new();
}
