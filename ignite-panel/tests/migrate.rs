//! Migrations are a deploy step: `ignite migrate` applies them, `ignite user create` refuses
//! to run on an old schema, and the app never migrates on start. Each test takes the
//! database lock through `reset`, which rolls the panel's migrations back and applies them
//! again, so every test starts with the same batch.

mod common;

use std::process::{Command, Stdio};

use common::{database_url, reset};
use diesel::connection::SimpleConnection;
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::BigInt;
use rocket::local::blocking::Client;

/// The latest migration, which adds the index on `login_throttles(window_start)`.
const VERSION: &str = "20261112000000";
const LATEST: &str = "20261112000000_login_throttles_window_index";
const INDEX: &str = "login_throttles_window_start_index";
/// The migrations that Diesel had applied before the roles migration.
const BEFORE_ROLES: [&str; 6] = [
    "20261108000000_create_users",
    "20261108010000_create_sessions",
    "20261108020000_create_login_throttles",
    "20261110000000_add_user_theme",
    "20261111000000_profile",
    "20261112000000_login_throttles_window_index",
];

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

/// Whether Orbit's table records the latest migration as applied.
fn version_recorded(conn: &mut MysqlConnection) -> bool {
    count(
        conn,
        &format!("SELECT COUNT(*) AS count FROM migrations WHERE migration = '{LATEST}'"),
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
        "DELETE FROM migrations WHERE migration = '{LATEST}'"
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
fn rollback_reverts_the_last_batch_and_migrate_restores_it() {
    let _guard = reset();
    let mut conn = connection();
    assert!(
        index_exists(&mut conn),
        "the latest migration is applied at the start"
    );

    // `reset` applies every migration in one batch, so one rollback reverts all of them,
    // last first.
    let rolled = ignite(&["migrate:rollback"], "");
    assert_eq!(
        rolled.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&rolled.stderr)
    );
    let out = String::from_utf8_lossy(&rolled.stdout);
    assert!(
        out.contains("Rolled back the last batch: 9 migrations."),
        "stdout: {out}"
    );
    assert!(
        out.contains(&format!("reverted {LATEST}.")),
        "stdout: {out}"
    );
    assert!(
        out.contains("reverted 20261115000000_password_reset_tokens."),
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
        status_out.contains(&format!("Pending  {LATEST}")),
        "stdout: {status_out}"
    );

    let nothing = ignite(&["migrate:rollback"], "");
    assert!(
        String::from_utf8_lossy(&nothing.stdout).contains("Nothing to roll back."),
        "stdout: {}",
        String::from_utf8_lossy(&nothing.stdout)
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

/// Turns the database into one that Diesel had migrated up to the roles migration: Orbit's
/// table is dropped, Diesel's history lists the six migrations before roles, and their tables
/// exist. The roles, account lifecycle and password reset migrations are still to run.
fn make_diesel_database(conn: &mut MysqlConnection) {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/migrations/");
    // Undo the whole panel first, so the schema is empty before the old one is built.
    rollback_everything();
    sql_query("DROP TABLE IF EXISTS migrations")
        .execute(conn)
        .unwrap();
    sql_query(
        "CREATE TABLE IF NOT EXISTS __diesel_schema_migrations (\
         version VARCHAR(50) PRIMARY KEY NOT NULL, \
         run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(conn)
    .unwrap();
    sql_query("DELETE FROM __diesel_schema_migrations")
        .execute(conn)
        .unwrap();
    for name in BEFORE_ROLES {
        let up = std::fs::read_to_string(format!("{root}{name}/up.sql")).unwrap();
        conn.batch_execute(&up).unwrap();
        sql_query(format!(
            "INSERT INTO __diesel_schema_migrations (version) VALUES ('{}')",
            &name[..14]
        ))
        .execute(conn)
        .unwrap();
    }
}

/// Reverts every batch until none is left, through the `ignite` binary.
fn rollback_everything() {
    loop {
        let out = ignite(&["migrate:rollback"], "");
        assert_eq!(out.status.code(), Some(0), "rollback failed");
        if String::from_utf8_lossy(&out.stdout).contains("Nothing to roll back.") {
            return;
        }
    }
}

#[test]
fn the_roles_migration_makes_existing_users_super_admins() {
    let _guard = reset();
    let mut conn = connection();
    make_diesel_database(&mut conn);
    sql_query("INSERT INTO users (name, email, password_hash, theme) VALUES ('Old', 'upgraded@example.test', 'x', 'light')")
        .execute(&mut conn)
        .unwrap();

    let status = ignite(&["migrate:status"], "");
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(
        status_out.contains(&format!("Applied  {VERSION}_login_throttles_window_index")),
        "the Diesel history counts as applied: {status_out}"
    );
    assert!(
        status_out.contains("Pending  20261113000000_roles_permissions"),
        "stdout: {status_out}"
    );

    let migrated = ignite(&["migrate"], "");
    assert_eq!(
        migrated.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    let out = String::from_utf8_lossy(&migrated.stdout);
    assert!(
        !out.contains("Applied 20261108000000_create_users"),
        "the imported migrations are not run again: {out}"
    );
    assert!(
        out.contains("Applied 20261113000000_roles_permissions"),
        "stdout: {out}"
    );

    let roles = ignite(&["user", "roles", "upgraded@example.test"], "");
    assert_eq!(
        String::from_utf8_lossy(&roles.stdout).trim(),
        "super-admin",
        "an existing user keeps full access after the upgrade"
    );
}

#[test]
fn a_rollback_of_the_imported_batch_is_not_undone_by_the_next_migrate() {
    let _guard = reset();
    let mut conn = connection();
    make_diesel_database(&mut conn);

    // The imported batch reverts with the rest. Orbit's table exists now, so the next migrate
    // must run the migrations again rather than record them as applied.
    let migrated = ignite(&["migrate"], "");
    assert_eq!(migrated.status.code(), Some(0));
    rollback_everything();
    let again = ignite(&["migrate"], "");
    assert_eq!(
        again.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&again.stderr)
    );
    let out = String::from_utf8_lossy(&again.stdout);
    assert!(
        out.contains("Applied 20261108000000_create_users"),
        "stdout: {out}"
    );
    assert!(index_exists(&mut conn), "the index is back after a re-run");
}
