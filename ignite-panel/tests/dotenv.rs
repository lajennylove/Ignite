//! `Ignite::new` reads a `.env` file from the working directory, and never overrides a
//! variable that is already set in the environment, as Laravel does. The test runs the
//! real `Ignite::new` in a child process, with a temporary directory as its working
//! directory, because the child's environment is the only one that can be controlled.

mod common;

use std::process::Command;

const CHILD: &str = "IGNITE_DOTENV_CHILD_RUN";
const TEST: &str = "ignite_new_reads_dot_env_without_overriding_the_environment";

#[test]
fn ignite_new_reads_dot_env_without_overriding_the_environment() {
    if std::env::var_os(CHILD).is_some() {
        child();
        return;
    }

    let dir = std::env::temp_dir().join(format!("ignite-dotenv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(".env"),
        format!(
            "DATABASE_URL={}\nIGNITE_SECRET_KEY={}\nIGNITE_TRUSTED_PROXIES=192.0.2.1\nIGNITE_DOTENV_MARKER=from-file\n",
            common::database_url(),
            common::random_key()
        ),
    )
    .unwrap();

    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env_remove("DATABASE_URL")
        .env_remove("IGNITE_SECRET_KEY")
        .env_remove("IGNITE_DOTENV_MARKER")
        // Set in the environment, so it must win over the file.
        .env("IGNITE_TRUSTED_PROXIES", "10.0.0.0/8")
        .current_dir(&dir)
        .output()
        .expect("run the test again as a child");
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        output.status.success(),
        "child failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The child: builds the panel, then checks what the process environment holds.
fn child() {
    let _ignite = ignite_panel::Ignite::new();

    assert_eq!(
        std::env::var("IGNITE_DOTENV_MARKER").as_deref(),
        Ok("from-file"),
        "a variable only in .env is loaded"
    );
    assert_eq!(
        std::env::var("DATABASE_URL").ok(),
        Some(common::database_url()),
        "DATABASE_URL comes from .env"
    );
    assert_eq!(
        std::env::var("IGNITE_TRUSTED_PROXIES").as_deref(),
        Ok("10.0.0.0/8"),
        "a variable already in the environment is not overridden by .env"
    );
}
