//! Migrations are a deploy step: `ignite migrate` applies them, `ignite user create` refuses
//! to run on an old schema, and the app never migrates on start. Each test takes the
//! database lock through `reset`, and sets the migration back to pending when it is done.

mod common;

use std::process::{Command, Stdio};

use common::{database_url, reset};
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::BigInt;
use rocket::local::blocking::Client;

/// The latest migration, which adds the index on `login_throttles(window_start)`.
const VERSION: &str = "20261112000000";
const INDEX: &str = "login_throttles_window_start_index";

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn connection() -> MysqlConnection {
    MysqlConnection::establish(&database_url()).expect("connect to ignite_test")
}

fn count(conn: &mut MysqlConnection, sql: &str) -> i64 {
    sql_query(sql)
        .get_result::<CountRow>(conn)
        .expect("count query")
        .count
}

fn version_recorded(conn: &mut MysqlConnection) -> bool {
    count(
        conn,
        &format!(
            "SELECT COUNT(*) AS count FROM __diesel_schema_migrations WHERE version = '{VERSION}'"
        ),
    ) > 0
}

fn index_exists(conn: &mut MysqlConnection) -> bool {
    count(
        conn,
        &format!(
            "SELECT COUNT(*) AS count FROM information_schema.statistics \
             WHERE table_schema = DATABASE() AND table_name = 'login_throttles' AND index_name = '{INDEX}'"
        ),
    ) > 0
}

/// Makes the latest migration pending again: no index, and no record that it ran.
fn make_latest_pending(conn: &mut MysqlConnection) {
    sql_query(format!("DROP INDEX IF EXISTS {INDEX} ON login_throttles"))
        .execute(conn)
        .unwrap();
    sql_query(format!(
        "DELETE FROM __diesel_schema_migrations WHERE version = '{VERSION}'"
    ))
    .execute(conn)
    .unwrap();
}

/// Runs the `ignite` binary with the test database and the given arguments.
fn ignite(args: &[&str], stdin: &str) -> std::process::Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_ignite"))
        .args(args)
        .env("DATABASE_URL", database_url())
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
        .unwrap();
    child.wait_with_output().expect("wait for ignite")
}

#[test]
fn ignite_migrate_applies_what_is_pending_and_then_nothing() {
    let _guard = reset();
    let mut conn = connection();
    make_latest_pending(&mut conn);

    let first = ignite(&["migrate"], "");
    assert_eq!(
        first.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let out = String::from_utf8_lossy(&first.stdout);
    assert!(
        out.contains(&format!("Applied {VERSION}_login_throttles_window_index")),
        "stdout: {out}"
    );
    assert!(version_recorded(&mut conn));
    assert!(index_exists(&mut conn));

    let second = ignite(&["migrate"], "");
    assert_eq!(second.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&second.stdout).contains("Nothing to migrate."));
}

#[test]
fn user_create_refuses_on_a_pending_schema_and_does_not_migrate() {
    let _guard = reset();
    let mut conn = connection();
    make_latest_pending(&mut conn);

    let output = ignite(
        &[
            "user",
            "create",
            "--email",
            "pending@example.test",
            "--password-stdin",
        ],
        "a-long-password\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ignite migrate"), "stderr: {stderr}");
    assert!(!version_recorded(&mut conn), "user create must not migrate");
    assert!(!index_exists(&mut conn), "user create must not migrate");
}

#[test]
fn the_app_does_not_migrate_when_it_starts() {
    let _guard = reset();
    let mut conn = connection();
    make_latest_pending(&mut conn);

    // Launching the client runs the panel's fairing, which is what an app does at start.
    let _client = Client::tracked(
        rocket::build().attach(
            ignite_panel::Ignite::new()
                .resource::<common::Widgets>()
                .fairing(),
        ),
    )
    .expect("valid rocket");

    assert!(!version_recorded(&mut conn), "the fairing must not migrate");
    assert!(!index_exists(&mut conn), "the fairing must not migrate");
}

#[test]
fn migrate_status_lists_every_migration_as_applied_after_a_migrate() {
    let _guard = reset();
    let output = ignite(&["migrate:status"], "");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(
        out.contains(&format!("Applied  {VERSION}_login_throttles_window_index")),
        "stdout: {out}"
    );
    assert!(!out.contains("Pending"), "stdout: {out}");
}

#[test]
fn rollback_reverts_one_step_and_migrate_restores_it() {
    let _guard = reset();
    let mut conn = connection();
    assert!(
        index_exists(&mut conn),
        "the latest migration is applied at the start"
    );

    // The account lifecycle and roles migrations are newer than the index one, so the first
    // two steps revert them, and the third step reverts the index migration this test is about.
    let lifecycle = ignite(&["migrate:rollback"], "");
    assert_eq!(lifecycle.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&lifecycle.stdout)
            .contains("reverted 20261114000000_account_lifecycle."),
        "stdout: {}",
        String::from_utf8_lossy(&lifecycle.stdout)
    );
    let roles = ignite(&["migrate:rollback"], "");
    assert_eq!(roles.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&roles.stdout)
            .contains("reverted 20261113000000_roles_permissions."),
        "stdout: {}",
        String::from_utf8_lossy(&roles.stdout)
    );
    assert!(
        index_exists(&mut conn),
        "the roles step leaves the index alone"
    );

    let rolled = ignite(&["migrate:rollback"], "");
    assert_eq!(
        rolled.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&rolled.stderr)
    );
    let out = String::from_utf8_lossy(&rolled.stdout);
    assert!(
        out.contains(&format!(
            "Rolled back one migration: reverted {VERSION}_login_throttles_window_index."
        )),
        "stdout: {out}"
    );
    assert!(!index_exists(&mut conn), "down.sql dropped the index");
    assert!(
        !version_recorded(&mut conn),
        "the record of the migration is removed"
    );

    let status = ignite(&["migrate:status"], "");
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(
        status_out.contains(&format!("Pending  {VERSION}_login_throttles_window_index")),
        "stdout: {status_out}"
    );

    let migrated = ignite(&["migrate"], "");
    assert_eq!(
        migrated.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    assert!(index_exists(&mut conn), "migrate restores the index");
    assert!(
        version_recorded(&mut conn),
        "migrate records the migration again"
    );
}

#[test]
fn the_roles_migration_makes_existing_users_super_admins() {
    let _guard = reset();
    let mut conn = connection();
    // Revert the account lifecycle migration, then the roles migration.
    for _ in 0..2 {
        let rolled = ignite(&["migrate:rollback"], "");
        assert_eq!(rolled.status.code(), Some(0));
    }
    sql_query("DELETE FROM users").execute(&mut conn).unwrap();
    let email = "upgraded@example.test";
    sql_query(format!(
        "INSERT INTO users (name, email, password_hash, theme) VALUES ('Old', '{email}', 'x', 'light')"
    ))
    .execute(&mut conn)
    .unwrap();

    let migrated = ignite(&["migrate"], "");
    assert_eq!(
        migrated.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    let roles = ignite(&["user", "roles", email], "");
    assert_eq!(
        String::from_utf8_lossy(&roles.stdout).trim(),
        "super-admin",
        "an existing user keeps full access after the upgrade"
    );
}
