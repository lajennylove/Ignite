//! Tests for the account lifecycle: disabling and enabling a user, the password reset from
//! the CLI, and the session rows that a deleted user leaves behind. They run against `ignite_test`.

mod common;

use std::io::Write;
use std::process::{Command, Stdio};

use common::*;
use ignite_panel::authz::{self, SUPER_ADMIN};
use ignite_panel::users;
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;

const ADA: &str = "ada@example.test";
const ADA_PASSWORD: &str = "a-long-password";

/// Runs `ignite` with the test database, the given arguments and standard input, from the
/// temp dir so no `.env` file is read.
fn ignite(args: &[&str], stdin: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ignite"))
        .args(args)
        .env("DATABASE_URL", database_url())
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run the ignite binary");
    // A command that does not read standard input may exit first, which closes the pipe.
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child
        .wait_with_output()
        .expect("wait for the ignite binary")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn connection() -> TestDb {
    test_db()
}

/// The number of session rows of the user with `user_id`.
fn sessions_of(conn: &TestDb, user_id: i64) -> i64 {
    conn.count(
        "SELECT COUNT(*) FROM sessions WHERE user_id = ?",
        &[user_id.into()],
    )
}

fn user_id(conn: &TestDb, email: &str) -> i64 {
    conn.run(users::find_by_email(&conn.db, email))
        .unwrap()
        .expect("the user exists")
        .id
}

fn sign_in(client: &Client, email: &str, password: &str) -> Status {
    client.get("/admin/login").dispatch();
    let body = with_token(client, &format!("email={email}&password={password}"));
    client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(body)
        .dispatch()
        .status()
}

/// A test user, Ada, who is also a super-admin, so she can open the widgets list.
fn reset_with_ada() -> (std::sync::MutexGuard<'static, ()>, TestDb) {
    let guard = reset();
    let conn = connection();
    conn.run(users::create_user(&conn.db, "Ada", ADA, ADA_PASSWORD))
        .expect("create Ada");
    conn.run(authz::assign_role(&conn.db, ADA, SUPER_ADMIN))
        .expect("make Ada a super-admin");
    (guard, conn)
}

#[test]
fn a_disabled_user_gets_the_wrong_password_message_and_cannot_sign_in() {
    let (_guard, _conn) = reset_with_ada();
    let disabled = ignite(&["user", "disable", ADA], "");
    assert_eq!(
        disabled.status.code(),
        Some(0),
        "stderr: {}",
        text(&disabled.stderr)
    );

    let client = client(None);
    client.get("/admin/login").dispatch();
    let response = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            &format!("email={ADA}&password={ADA_PASSWORD}"),
        ))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    let body = response.into_string().unwrap();
    assert!(
        body.contains("The email or password is incorrect."),
        "a disabled user sees the normal message, so the account is not revealed"
    );
    assert!(client.cookies().get("ignite_session").is_none());

    let enabled = ignite(&["user", "enable", ADA], "");
    assert_eq!(enabled.status.code(), Some(0));
    assert_eq!(sign_in(&client, ADA, ADA_PASSWORD), Status::SeeOther);
}

#[test]
fn disabling_a_user_ends_their_sessions_at_once() {
    let (_guard, conn) = reset_with_ada();
    let ada = client(None);
    assert_eq!(sign_in(&ada, ADA, ADA_PASSWORD), Status::SeeOther);
    assert_eq!(ada.get("/admin/widgets").dispatch().status(), Status::Ok);
    let ada_id = user_id(&conn, ADA);
    assert_eq!(sessions_of(&conn, ada_id), 1);

    let disabled = ignite(&["user", "disable", ADA], "");
    assert_eq!(
        disabled.status.code(),
        Some(0),
        "stderr: {}",
        text(&disabled.stderr)
    );
    assert!(text(&disabled.stdout).contains("Disabled ada@example.test."));

    let refused = ada.get("/admin/widgets").dispatch();
    assert_eq!(refused.status(), Status::SeeOther);
    assert_eq!(
        refused.headers().get_one("Location"),
        Some("/admin/login"),
        "the old cookie is refused like a signed-out request"
    );
    assert_eq!(
        sessions_of(&conn, ada_id),
        0,
        "the session rows are deleted"
    );
}

#[test]
fn disabling_or_enabling_twice_is_reported_and_is_not_an_error() {
    let (_guard, _conn) = reset_with_ada();
    assert_eq!(ignite(&["user", "disable", ADA], "").status.code(), Some(0));

    let again = ignite(&["user", "disable", ADA], "");
    assert_eq!(again.status.code(), Some(0));
    assert!(text(&again.stdout).contains("ada@example.test is already disabled."));

    assert_eq!(ignite(&["user", "enable", ADA], "").status.code(), Some(0));
    let enabled_again = ignite(&["user", "enable", ADA], "");
    assert_eq!(enabled_again.status.code(), Some(0));
    assert!(text(&enabled_again.stdout).contains("ada@example.test is already enabled."));

    let unknown = ignite(&["user", "disable", "nobody@example.test"], "");
    assert_eq!(unknown.status.code(), Some(1));
    assert!(text(&unknown.stderr).contains("The user nobody@example.test does not exist."));
}

#[test]
fn the_last_enabled_super_admin_cannot_be_disabled_or_lose_the_role() {
    let (_guard, conn) = reset_with_ada();
    // Ada and the test user are both super-admins, so either one can be disabled.
    assert_eq!(ignite(&["user", "disable", ADA], "").status.code(), Some(0));

    // Ada is disabled, so the test user is now the only enabled super-admin.
    let refused = ignite(&["user", "disable", EMAIL], "");
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        text(&refused.stderr).contains("last super-admin who is enabled"),
        "stderr: {}",
        text(&refused.stderr)
    );
    assert!(conn
        .run(users::find_by_email(&conn.db, EMAIL))
        .unwrap()
        .unwrap()
        .disabled_at
        .is_none());

    // A disabled super-admin does not count, so the role cannot go from the enabled one either.
    let removed = ignite(&["user", "role", "remove", EMAIL, SUPER_ADMIN], "");
    assert_eq!(removed.status.code(), Some(1));
    assert!(text(&removed.stderr).contains("last super-admin"));
}

#[test]
fn a_super_admin_can_be_disabled_while_another_enabled_super_admin_remains() {
    let (_guard, _conn) = reset_with_ada();
    assert_eq!(ignite(&["user", "disable", ADA], "").status.code(), Some(0));
    // Ada is disabled and the test user is enabled, so enabling Ada leaves two enabled
    // super-admins, and either one may be disabled.
    assert_eq!(ignite(&["user", "enable", ADA], "").status.code(), Some(0));
    assert_eq!(
        ignite(&["user", "disable", EMAIL], "").status.code(),
        Some(0)
    );
}

#[test]
fn a_password_reset_from_the_cli_replaces_the_password_and_ends_sessions() {
    let (_guard, conn) = reset_with_ada();
    let ada = client(None);
    assert_eq!(sign_in(&ada, ADA, ADA_PASSWORD), Status::SeeOther);
    let ada_id = user_id(&conn, ADA);
    assert_eq!(sessions_of(&conn, ada_id), 1);

    let reset = ignite(
        &["user", "password", ADA, "--password-stdin"],
        "a-new-password\n",
    );
    assert_eq!(
        reset.status.code(),
        Some(0),
        "stderr: {}",
        text(&reset.stderr)
    );
    assert!(text(&reset.stdout)
        .contains("Changed the password of ada@example.test. Their sessions have ended."));
    assert_eq!(sessions_of(&conn, ada_id), 0);
    assert_eq!(
        ada.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );

    let fresh = client(None);
    assert_eq!(
        sign_in(&fresh, ADA, ADA_PASSWORD),
        Status::UnprocessableEntity,
        "the old password no longer works"
    );
    assert_eq!(sign_in(&fresh, ADA, "a-new-password"), Status::SeeOther);
}

#[test]
fn a_password_reset_refuses_a_short_password_and_an_unknown_user() {
    let (_guard, _conn) = reset_with_ada();

    let short = ignite(&["user", "password", ADA, "--password-stdin"], "short\n");
    assert_eq!(short.status.code(), Some(1));
    assert!(text(&short.stderr).contains("at least 8 characters"));

    let unknown = ignite(
        &[
            "user",
            "password",
            "nobody@example.test",
            "--password-stdin",
        ],
        "a-new-password\n",
    );
    assert_eq!(unknown.status.code(), Some(1));
    assert!(text(&unknown.stderr).contains("The user nobody@example.test does not exist."));

    let missing = ignite(&["user", "password"], "");
    assert_eq!(missing.status.code(), Some(2), "an email is required");

    let client = client(None);
    assert_eq!(
        sign_in(&client, ADA, ADA_PASSWORD),
        Status::SeeOther,
        "a refused reset changes nothing"
    );
}

#[test]
fn deleting_a_user_deletes_their_session_rows() {
    let (_guard, conn) = reset_with_ada();
    let ada = client(None);
    assert_eq!(sign_in(&ada, ADA, ADA_PASSWORD), Status::SeeOther);
    let ada_id = user_id(&conn, ADA);
    assert_eq!(sessions_of(&conn, ada_id), 1);

    conn.exec_with("DELETE FROM users WHERE id = ?", &[ada_id.into()]);

    assert_eq!(
        sessions_of(&conn, ada_id),
        0,
        "the sessions go with the user"
    );
    assert_eq!(
        ada.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

/// Drops `sessions_user_fk` while it lives, the way a SQLite install has no session key, and
/// adds it back when dropped, so the rollback in the next `reset` finds the key to drop.
struct WithoutSessionKey<'a>(&'a TestDb);

impl<'a> WithoutSessionKey<'a> {
    fn new(conn: &'a TestDb) -> Self {
        conn.exec("ALTER TABLE sessions DROP FOREIGN KEY sessions_user_fk");
        WithoutSessionKey(conn)
    }
}

impl Drop for WithoutSessionKey<'_> {
    fn drop(&mut self) {
        // A failed test may leave rows whose user is gone, and the key refuses them.
        self.0
            .exec("DELETE FROM sessions WHERE user_id NOT IN (SELECT id FROM users)");
        self.0.exec(
            "ALTER TABLE sessions ADD CONSTRAINT sessions_user_fk \
             FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE",
        );
    }
}

/// Signs in Ada and the test user, and returns their ids.
fn ada_and_admin_signed_in(conn: &TestDb) -> (i64, i64) {
    assert_eq!(sign_in(&client(None), ADA, ADA_PASSWORD), Status::SeeOther);
    assert_eq!(sign_in(&client(None), EMAIL, PASSWORD), Status::SeeOther);
    let (ada_id, admin_id) = (user_id(conn, ADA), user_id(conn, EMAIL));
    assert_eq!(sessions_of(conn, ada_id), 1);
    assert_eq!(sessions_of(conn, admin_id), 1);
    (ada_id, admin_id)
}

#[test]
fn deleting_a_user_deletes_their_sessions_without_the_session_key() {
    let (_guard, conn) = reset_with_ada();
    let (ada_id, admin_id) = ada_and_admin_signed_in(&conn);
    let _no_key = WithoutSessionKey::new(&conn);

    assert_eq!(conn.run(users::delete_user(&conn.db, ada_id)).unwrap(), 1);

    assert_eq!(
        sessions_of(&conn, ada_id),
        0,
        "the sessions go with the user"
    );
    assert_eq!(sessions_of(&conn, admin_id), 1, "other users keep theirs");
}

#[test]
fn deleting_a_user_from_the_command_library_deletes_their_sessions_without_the_session_key() {
    let (_guard, conn) = reset_with_ada();
    let (ada_id, admin_id) = ada_and_admin_signed_in(&conn);
    let _no_key = WithoutSessionKey::new(&conn);

    conn.run(authz::delete_user(&conn.db, ADA))
        .expect("delete Ada");

    assert_eq!(
        sessions_of(&conn, ada_id),
        0,
        "the sessions go with the user"
    );
    assert_eq!(sessions_of(&conn, admin_id), 1, "other users keep theirs");
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

/// Gives Ada a picture through a panel that stores files in the test's storage directory, and
/// returns her id and the picture's file.
fn ada_with_a_picture(conn: &TestDb) -> (i64, std::path::PathBuf) {
    // Building the panel makes its storage directory the process's.
    let _panel = client(None);
    let ada_id = user_id(conn, ADA);
    conn.run(users::set_avatar(&conn.db, ada_id, "image/png", PNG))
        .expect("store Ada's picture");
    let file = storage_dir().join(format!("avatars/{ada_id}.png"));
    assert!(file.exists(), "the picture is on disk");
    (ada_id, file)
}

#[test]
fn deleting_a_user_removes_their_picture_file() {
    let (_guard, conn) = reset_with_ada();
    let (ada_id, file) = ada_with_a_picture(&conn);

    assert_eq!(conn.run(users::delete_user(&conn.db, ada_id)).unwrap(), 1);

    assert!(!file.exists(), "the file goes with the user");
    assert_eq!(
        conn.count(
            "SELECT COUNT(*) FROM user_avatars WHERE user_id = ?",
            &[ada_id.into()]
        ),
        0
    );
}

#[test]
fn deleting_a_user_from_the_command_library_removes_their_picture_file() {
    let (_guard, conn) = reset_with_ada();
    let (_, file) = ada_with_a_picture(&conn);

    conn.run(authz::delete_user(&conn.db, ADA))
        .expect("delete Ada");

    assert!(!file.exists(), "the file goes with the user");
}

#[test]
fn deleting_a_user_whose_picture_file_is_missing_is_not_an_error() {
    let (_guard, conn) = reset_with_ada();
    let (ada_id, file) = ada_with_a_picture(&conn);
    std::fs::remove_file(&file).unwrap();

    assert_eq!(conn.run(users::delete_user(&conn.db, ada_id)).unwrap(), 1);
    assert_eq!(
        conn.count("SELECT COUNT(*) FROM users WHERE id = ?", &[ada_id.into()]),
        0
    );
}

#[test]
fn the_stock_command_removes_the_picture_from_the_default_storage_directory() {
    let (_guard, conn) = reset_with_ada();
    let ada_id = user_id(&conn, ADA);
    // The stock `ignite` has no builder, so its storage directory is `storage` in the working
    // directory. The picture is put there by hand.
    let work = std::env::temp_dir().join(format!("ignite-test-cwd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(work.join("storage/avatars")).unwrap();
    let file = work.join(format!("storage/avatars/{ada_id}.png"));
    std::fs::write(&file, PNG).unwrap();
    conn.exec_with(
        "INSERT INTO user_avatars (user_id, path, mime) VALUES (?, ?, 'image/png')",
        &[
            ada_id.into(),
            format!("avatars/{ada_id}.png").as_str().into(),
        ],
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ignite"))
        .args(["user", "delete", ADA])
        .env("DATABASE_URL", database_url())
        .current_dir(&work)
        .output()
        .expect("run the ignite binary");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));

    assert!(!file.exists(), "the file goes with the user");
    std::fs::remove_dir_all(&work).unwrap();
}

/// The migrations that ran before the account lifecycle migration.
const BEFORE_LIFECYCLE: [&str; 7] = [
    "20261108000000_create_users",
    "20261108010000_create_sessions",
    "20261108020000_create_login_throttles",
    "20261110000000_add_user_theme",
    "20261111000000_profile",
    "20261112000000_login_throttles_window_index",
    "20261113000000_roles_permissions",
];

#[test]
fn the_migration_removes_session_rows_that_have_no_user_and_then_keeps_new_ones_out() {
    let _guard = reset();
    let conn = connection();
    // Rewind to the state before the account lifecycle migration, so the test can leave an
    // orphan row the way an old database has one.
    common::rewind_to(&BEFORE_LIFECYCLE);
    conn.exec("INSERT INTO sessions (id, user_id, expires_at) VALUES ('orphan-session', 999999, NOW() + INTERVAL 1 DAY)");

    let migrated = ignite(&["migrate"], "");
    assert_eq!(
        migrated.status.code(),
        Some(0),
        "stderr: {}",
        text(&migrated.stderr)
    );
    let orphans = conn.count(
        "SELECT COUNT(*) FROM sessions WHERE id = 'orphan-session'",
        &[],
    );
    assert_eq!(
        orphans, 0,
        "the orphan row is removed before the key is added"
    );

    let no_user = conn.try_exec("INSERT INTO sessions (id, user_id, expires_at) VALUES ('no-user', 999999, NOW() + INTERVAL 1 DAY)");
    assert!(no_user.is_err(), "a session must belong to a user");
}
