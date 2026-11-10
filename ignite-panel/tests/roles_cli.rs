//! Tests for the `ignite role` and `ignite user role` commands, against `ignite_test`.

mod common;

use std::process::{Command, Stdio};

use common::{client, database_url, reset, EMAIL};
use diesel::mysql::MysqlConnection;
use diesel::Connection;
use ignite_panel::authz::SUPER_ADMIN;
use ignite_panel::users;

/// Runs `ignite` with the test database and the given arguments, from the temp dir so no
/// `.env` file is read.
fn ignite(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ignite"))
        .args(args)
        .env("DATABASE_URL", database_url())
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run the ignite binary")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn connection() -> MysqlConnection {
    MysqlConnection::establish(&database_url()).expect("connect to ignite_test")
}

#[test]
fn role_create_makes_a_role_and_refuses_a_second_one_with_the_same_name() {
    let _guard = reset();
    let first = ignite(&["role", "create", "editors"]);
    assert_eq!(
        first.status.code(),
        Some(0),
        "stderr: {}",
        text(&first.stderr)
    );
    assert!(text(&first.stdout).contains("Created role editors."));

    let second = ignite(&["role", "create", "editors"]);
    assert_eq!(second.status.code(), Some(1));
    assert!(text(&second.stderr).contains("A role named editors already exists."));

    let missing = ignite(&["role", "create"]);
    assert_eq!(
        missing.status.code(),
        Some(2),
        "a missing name is a usage error"
    );
}

#[test]
fn role_grant_gives_a_permission_that_the_app_registered_and_refuses_an_unknown_one() {
    let _guard = reset();
    let _ = client(None);
    assert_eq!(
        ignite(&["role", "create", "editors"]).status.code(),
        Some(0)
    );

    let granted = ignite(&["role", "grant", "editors", "widgets.viewAny"]);
    assert_eq!(
        granted.status.code(),
        Some(0),
        "stderr: {}",
        text(&granted.stderr)
    );
    assert!(text(&granted.stdout).contains("Granted widgets.viewAny to editors."));

    let unknown = ignite(&["role", "grant", "editors", "widgets.fly"]);
    assert_eq!(unknown.status.code(), Some(1));
    let stderr = text(&unknown.stderr);
    assert!(stderr.contains("widgets.fly"), "stderr: {stderr}");
    assert!(stderr.contains("Start the app once"), "stderr: {stderr}");

    let no_role = ignite(&["role", "grant", "nobody", "widgets.viewAny"]);
    assert_eq!(no_role.status.code(), Some(1));
    assert!(text(&no_role.stderr).contains("The role nobody does not exist."));
}

#[test]
fn user_role_add_lists_and_removes_a_role() {
    let _guard = reset();
    let _ = client(None);
    let mut conn = connection();
    users::create_user(&mut conn, "Ada", "ada@example.test", "a-long-password").unwrap();
    assert_eq!(
        ignite(&["role", "create", "editors"]).status.code(),
        Some(0)
    );

    let added = ignite(&["user", "role", "add", "ada@example.test", "editors"]);
    assert_eq!(
        added.status.code(),
        Some(0),
        "stderr: {}",
        text(&added.stderr)
    );
    assert!(text(&added.stdout).contains("Added role editors to ada@example.test."));

    let listed = ignite(&["user", "roles", "ada@example.test"]);
    assert_eq!(listed.status.code(), Some(0));
    assert_eq!(text(&listed.stdout).trim(), "editors");

    let removed = ignite(&["user", "role", "remove", "ada@example.test", "editors"]);
    assert_eq!(
        removed.status.code(),
        Some(0),
        "stderr: {}",
        text(&removed.stderr)
    );
    assert!(text(&removed.stdout).contains("Removed role editors from ada@example.test."));

    let again = ignite(&["user", "role", "remove", "ada@example.test", "editors"]);
    assert_eq!(again.status.code(), Some(0));
    assert!(text(&again.stdout).contains("did not have the role editors"));

    let none = ignite(&["user", "roles", "ada@example.test"]);
    assert!(text(&none.stdout).contains("ada@example.test has no roles."));

    let unknown = ignite(&["user", "role", "add", "nobody@example.test", "editors"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(text(&unknown.stderr).contains("The user nobody@example.test does not exist."));
}

#[test]
fn the_cli_refuses_to_remove_the_last_super_admin() {
    let _guard = reset();
    let refused = ignite(&["user", "role", "remove", EMAIL, SUPER_ADMIN]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        text(&refused.stderr).contains("last super-admin"),
        "stderr: {}",
        text(&refused.stderr)
    );
    let listed = ignite(&["user", "roles", EMAIL]);
    assert_eq!(text(&listed.stdout).trim(), SUPER_ADMIN);

    let mut conn = connection();
    users::create_user(
        &mut conn,
        "Second",
        "second@example.test",
        "a-long-password",
    )
    .unwrap();
    assert_eq!(
        ignite(&["user", "role", "add", "second@example.test", SUPER_ADMIN])
            .status
            .code(),
        Some(0)
    );
    let allowed = ignite(&["user", "role", "remove", EMAIL, SUPER_ADMIN]);
    assert_eq!(
        allowed.status.code(),
        Some(0),
        "stderr: {}",
        text(&allowed.stderr)
    );
}

#[test]
fn role_commands_refuse_when_the_roles_tables_are_missing() {
    let _guard = reset();
    // The roles migration is the latest one, so one rollback drops its tables.
    let rolled = ignite(&["migrate:rollback"]);
    assert_eq!(
        rolled.status.code(),
        Some(0),
        "stderr: {}",
        text(&rolled.stderr)
    );

    let refused = ignite(&["role", "create", "editors"]);
    assert_eq!(refused.status.code(), Some(1));
    let stderr = text(&refused.stderr);
    assert!(stderr.contains("ignite migrate"), "stderr: {stderr}");

    // Put the tables back, so the other tests start from a migrated database.
    assert_eq!(ignite(&["migrate"]).status.code(), Some(0));
}
