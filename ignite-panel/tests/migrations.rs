//! The panel's migrations, written with Orbit's schema builder, on SQLite, MySQL and MariaDB,
//! and PostgreSQL. Each database runs every migration through `ignite_panel::migrate`, checks
//! the tables and keys they leave, and rolls everything back through `ignite_panel::rollback`.
//! The `ignite migrate` command runs them too, on each database the test can reach.
//!
//! MySQL and MariaDB also stand in for an existing install: the old SQL files, which installs
//! ran before the move to the schema builder, build the database, and the new migrations must
//! find nothing to run, report everything applied, and roll that schema back by the names of
//! its indexes and keys. The old files are fixtures in `tests/fixtures/migrations/`.
//!
//! The databases come from the environment only: `DATABASE_URL` (MySQL or MariaDB, as the
//! other tests), `IGNITE_TEST_POSTGRES_URL` and `IGNITE_TEST_SQLITE_URL`. The PostgreSQL and
//! SQLite tests build with the `postgres` and `sqlite` features, and skip with a notice when
//! their variable is not set.

mod common;

use std::process::Command;

use common::{database_url, LOCK};
use ignite_panel::{migrate, migration_status, pending_migrations, rollback};
use orbit_orm::{raw, Database, Driver, Migration, Migrator, Value};
use rocket::tokio::runtime::Runtime;

/// Every migration of the panel, in order.
const NAMES: [&str; 9] = [
    "20261108000000_create_users",
    "20261108010000_create_sessions",
    "20261108020000_create_login_throttles",
    "20261110000000_add_user_theme",
    "20261111000000_profile",
    "20261112000000_login_throttles_window_index",
    "20261113000000_roles_permissions",
    "20261114000000_account_lifecycle",
    "20261115000000_password_reset_tokens",
];

/// The tables the migrations create, each one before the tables it references.
const TABLES: [&str; 9] = [
    "role_user",
    "role_permission",
    "permissions",
    "roles",
    "user_avatars",
    "password_reset_tokens",
    "sessions",
    "login_throttles",
    "users",
];

fn runtime() -> Runtime {
    Runtime::new().expect("a runtime for the test")
}

/// Reverts every batch, then drops whatever a failed run left behind, so each test starts
/// from a database without the panel's tables.
async fn clean(db: &Database) {
    while !rollback(db).await.expect("rollback").is_empty() {}
    for table in TABLES.iter().chain(["__diesel_schema_migrations"].iter()) {
        raw::execute(db, &format!("DROP TABLE IF EXISTS {table}"), &[])
            .await
            .unwrap_or_else(|e| panic!("drop {table}: {e}"));
    }
}

async fn count(db: &Database, sql: &str, binds: &[Value]) -> i64 {
    raw::fetch_scalar(db, sql, binds)
        .await
        .unwrap_or_else(|e| panic!("`{sql}` failed: {e}"))
}

async fn exec(db: &Database, sql: &str, binds: &[Value]) {
    raw::execute(db, sql, binds)
        .await
        .unwrap_or_else(|e| panic!("`{sql}` failed: {e}"));
}

/// Whether `table` exists in the connected database, read from its catalog.
async fn table_exists(db: &Database, table: &str) -> bool {
    let sql = match db.driver() {
        Driver::MySql => {
            "SELECT COUNT(*) FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_name = ?"
        }
        #[cfg(feature = "postgres")]
        Driver::Postgres => {
            "SELECT COUNT(*) FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_name = ?"
        }
        #[cfg(feature = "sqlite")]
        Driver::Sqlite => "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
    };
    count(db, sql, &[table.into()]).await > 0
}

#[cfg(feature = "sqlite")]
fn on_sqlite(db: &Database) -> bool {
    db.driver() == Driver::Sqlite
}

#[cfg(not(feature = "sqlite"))]
fn on_sqlite(_db: &Database) -> bool {
    false
}

/// Runs every migration on an empty database, checks what they built, and rolls them back.
async fn migrate_check_and_roll_back(db: &Database) {
    clean(db).await;

    let applied = migrate(db).await.expect("migrate");
    assert_eq!(applied, NAMES, "every migration runs, in order");
    assert!(
        migrate(db).await.expect("migrate again").is_empty(),
        "a second run applies nothing"
    );
    for table in TABLES {
        assert!(table_exists(db, table).await, "{table} exists");
    }

    // A user, with the defaults of the users table.
    exec(
        db,
        "INSERT INTO users (name, email, password_hash) VALUES (?, ?, ?)",
        &["Ann".into(), "ann@example.test".into(), "hash".into()],
    )
    .await;
    let ann = count(
        db,
        "SELECT id FROM users WHERE email = ?",
        &["ann@example.test".into()],
    )
    .await;
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM users WHERE id = ? AND theme = 'light' \
             AND created_at IS NOT NULL AND disabled_at IS NULL AND avatar_mime IS NULL",
            &[ann.into()],
        )
        .await,
        1,
        "a new user is enabled, on the light theme, with a creation time"
    );
    let duplicate = raw::execute(
        db,
        "INSERT INTO users (name, email, password_hash) VALUES ('Ann 2', 'ann@example.test', 'x')",
        &[],
    )
    .await;
    assert!(duplicate.is_err(), "the email is unique");

    // Her session, avatar, role and a reset link.
    exec(
        db,
        "INSERT INTO sessions (id, user_id, expires_at) VALUES ('session-ann', ?, '2026-12-01 00:00:00')",
        &[ann.into()],
    )
    .await;
    exec(
        db,
        "INSERT INTO user_avatars (user_id, path, mime) VALUES (?, 'avatars/1.png', 'image/png')",
        &[ann.into()],
    )
    .await;
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM roles WHERE name = 'super-admin'",
            &[]
        )
        .await,
        1,
        "the roles migration seeds the super admin role"
    );
    exec(
        db,
        "INSERT INTO role_user (user_id, role_id) \
         SELECT ?, id FROM roles WHERE name = 'super-admin'",
        &[ann.into()],
    )
    .await;
    exec(
        db,
        "INSERT INTO permissions (name) VALUES ('users.viewAny')",
        &[],
    )
    .await;
    exec(
        db,
        "INSERT INTO role_permission (role_id, permission_id) \
         SELECT roles.id, permissions.id FROM roles CROSS JOIN permissions \
         WHERE roles.name = 'super-admin' AND permissions.name = 'users.viewAny'",
        &[],
    )
    .await;
    exec(
        db,
        "INSERT INTO password_reset_tokens (email, token, created_at) \
         VALUES ('ann@example.test', 'hashed', '2026-11-14 10:20:30.123456')",
        &[],
    )
    .await;
    exec(
        db,
        "INSERT INTO login_throttles (throttle_key, hits, window_start) \
         VALUES ('ann@example.test', 1, '2026-11-14 10:20:30')",
        &[],
    )
    .await;

    // A session must belong to a user, except on SQLite, which cannot add the key to the
    // existing sessions table (the session cleanup is the panel's job there).
    let no_user = raw::execute(
        db,
        "INSERT INTO sessions (id, user_id, expires_at) VALUES ('no-user', 999999, '2026-12-01 00:00:00')",
        &[],
    )
    .await;
    if on_sqlite(db) {
        assert!(no_user.is_ok(), "SQLite has no session key");
        exec(db, "DELETE FROM sessions WHERE id = 'no-user'", &[]).await;
    } else {
        assert!(no_user.is_err(), "a session must belong to a user");
    }

    // Deleting Ann takes her avatar and roles with her, and her sessions where the key exists.
    exec(db, "DELETE FROM users WHERE id = ?", &[ann.into()]).await;
    for table in ["user_avatars", "role_user"] {
        assert_eq!(
            count(
                db,
                &format!("SELECT COUNT(*) FROM {table} WHERE user_id = ?"),
                &[ann.into()]
            )
            .await,
            0,
            "{table} rows go with the user"
        );
    }
    let sessions_left = if on_sqlite(db) { 1 } else { 0 };
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM sessions WHERE user_id = ?",
            &[ann.into()]
        )
        .await,
        sessions_left
    );

    // One migrate is one batch, so one rollback reverts all nine, last first.
    let reverted = rollback(db).await.expect("rollback");
    let mut expected = NAMES.to_vec();
    expected.reverse();
    assert_eq!(reverted, expected);
    for table in TABLES {
        assert!(!table_exists(db, table).await, "{table} is gone");
    }
    assert!(rollback(db).await.expect("rollback again").is_empty());

    clean(db).await;
}

/// Runs the `ignite` binary on `url`.
fn ignite(url: &str, args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ignite"))
        .args(args)
        .env("DATABASE_URL", url)
        .current_dir(std::env::temp_dir())
        .output()
        .expect("run the ignite binary");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    (
        output.status.code().unwrap_or(-1),
        format!("{stdout}{stderr}"),
    )
}

/// `ignite migrate` applies every migration, then nothing, and `ignite migrate:rollback`
/// reverts them, on the database at `url`, which `db` is connected to.
fn cli_migrate_and_roll_back(rt: &Runtime, db: &Database, url: &str) {
    rt.block_on(clean(db));

    let (code, out) = ignite(url, &["migrate"]);
    assert_eq!(code, 0, "{out}");
    for name in NAMES {
        assert!(out.contains(&format!("Applied {name}")), "{out}");
    }
    let (code, out) = ignite(url, &["migrate"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Nothing to migrate."), "{out}");
    for table in TABLES {
        assert!(rt.block_on(table_exists(db, table)), "{table} exists");
    }

    let (code, out) = ignite(url, &["migrate:rollback"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("Rolled back the last batch: 9 migrations."),
        "{out}"
    );
    for table in TABLES {
        assert!(!rt.block_on(table_exists(db, table)), "{table} is gone");
    }

    rt.block_on(clean(db));
}

// ---------------------------------------------------------------------------------------------
// MySQL and MariaDB
// ---------------------------------------------------------------------------------------------

#[test]
fn the_migrations_run_and_roll_back_on_mysql() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let rt = runtime();
    let db = rt
        .block_on(Database::connect(&database_url()))
        .expect("connect to DATABASE_URL");
    rt.block_on(migrate_check_and_roll_back(&db));
    cli_migrate_and_roll_back(&rt, &db, &database_url());
}

/// The old SQL files, as installs ran them, as one migrator.
fn old_migrator() -> Migrator {
    let migrations = NAMES
        .iter()
        .map(|name| {
            let read = |file: &str| {
                std::fs::read_to_string(format!("{}{name}/{file}", common::OLD_MIGRATIONS))
                    .unwrap_or_else(|e| panic!("read {name}/{file}: {e}"))
            };
            Migration::new(*name, read("up.sql"), read("down.sql"))
        })
        .collect();
    Migrator::new(migrations).expect("the old migrations")
}

/// `SHOW CREATE TABLE` for each table, without the `AUTO_INCREMENT=` counter, which depends
/// on the rows written rather than the schema.
async fn create_statements(db: &Database, tables: &[&str]) -> Vec<String> {
    let mut statements = Vec::new();
    for table in tables {
        let rows: Vec<(String, String)> =
            raw::fetch_all(db, &format!("SHOW CREATE TABLE {table}"), &[])
                .await
                .unwrap_or_else(|e| panic!("show {table}: {e}"));
        let sql = rows.into_iter().next().expect("one row").1;
        let sql = sql
            .split(' ')
            .filter(|word| !word.starts_with("AUTO_INCREMENT="))
            .collect::<Vec<_>>()
            .join(" ");
        statements.push(sql);
    }
    statements
}

#[test]
fn an_install_from_the_old_sql_is_not_migrated_twice_and_rolls_back() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let rt = runtime();
    let db = rt
        .block_on(Database::connect(&database_url()))
        .expect("connect to DATABASE_URL");
    rt.block_on(async {
        clean(&db).await;

        // An existing install: every old migration ran and is recorded.
        let applied = old_migrator().run(&db).await.expect("the old SQL runs");
        assert_eq!(applied, NAMES);
        exec(
            &db,
            "INSERT INTO users (name, email, password_hash) VALUES ('Old', 'old@example.test', 'x')",
            &[],
        )
        .await;

        // The new migrations have the same names, so nothing is pending.
        assert!(
            migrate(&db).await.expect("migrate").is_empty(),
            "nothing runs twice"
        );
        let status = migration_status(&db).await.expect("status");
        assert_eq!(
            status,
            NAMES
                .iter()
                .map(|name| (name.to_string(), true))
                .collect::<Vec<_>>()
        );
        assert!(pending_migrations(&db).await.expect("pending").is_empty());

        // Every table but the avatars (which moved to disk) is the one the new migrations
        // build, so the old schema is kept to compare below.
        let compared: Vec<&str> = TABLES
            .iter()
            .copied()
            .filter(|t| *t != "user_avatars")
            .collect();
        let old = create_statements(&db, &compared).await;

        // The new down migrations revert the old schema: the index and key names match.
        let reverted = rollback(&db).await.expect("the new rollback");
        assert_eq!(reverted.len(), 9);
        for table in TABLES {
            assert!(!table_exists(&db, table).await, "{table} is gone");
        }

        // The new migrations build the same tables as the old SQL did.
        assert_eq!(migrate(&db).await.expect("migrate").len(), 9);
        assert_eq!(create_statements(&db, &compared).await, old);
        let avatar_columns: Vec<(String,)> = raw::fetch_all(
            &db,
            "SELECT CAST(column_name AS CHAR) FROM information_schema.columns \
             WHERE table_schema = DATABASE() AND table_name = 'user_avatars' \
             ORDER BY ordinal_position",
            &[],
        )
        .await
        .expect("the avatar columns");
        assert_eq!(
            avatar_columns,
            vec![
                ("user_id".to_owned(),),
                ("path".to_owned(),),
                ("mime".to_owned(),)
            ]
        );

        clean(&db).await;
    });
}

// ---------------------------------------------------------------------------------------------
// PostgreSQL
// ---------------------------------------------------------------------------------------------

#[cfg(feature = "postgres")]
#[test]
fn the_migrations_run_and_roll_back_on_postgres() {
    let Ok(url) = std::env::var("IGNITE_TEST_POSTGRES_URL") else {
        eprintln!("skipped: set IGNITE_TEST_POSTGRES_URL to run the migrations on PostgreSQL");
        return;
    };
    let rt = runtime();
    let db = rt
        .block_on(Database::connect(&url))
        .expect("connect to IGNITE_TEST_POSTGRES_URL");
    assert_eq!(db.driver(), Driver::Postgres);
    rt.block_on(migrate_check_and_roll_back(&db));
    cli_migrate_and_roll_back(&rt, &db, &url);
}

// ---------------------------------------------------------------------------------------------
// SQLite
// ---------------------------------------------------------------------------------------------

#[cfg(feature = "sqlite")]
#[test]
fn the_migrations_run_and_roll_back_on_sqlite() {
    let Ok(url) = std::env::var("IGNITE_TEST_SQLITE_URL") else {
        eprintln!("skipped: set IGNITE_TEST_SQLITE_URL to run the migrations on SQLite");
        return;
    };
    let rt = runtime();
    let db = rt
        .block_on(Database::connect(&url))
        .expect("connect to IGNITE_TEST_SQLITE_URL");
    assert_eq!(db.driver(), Driver::Sqlite);
    rt.block_on(migrate_check_and_roll_back(&db));
    cli_migrate_and_roll_back(&rt, &db, &url);
}
