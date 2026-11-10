//! Tests for the account lifecycle: disabling and enabling a user, the password reset from
//! the CLI, and the session rows that a deleted user leaves behind. They run against `ignite_test`.

mod common;

use std::io::Write;
use std::process::{Command, Stdio};

use common::*;
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::BigInt;
use ignite_panel::authz::{self, SUPER_ADMIN};
use ignite_panel::users;
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;

const ADA: &str = "ada@example.test";
const ADA_PASSWORD: &str = "a-long-password";

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

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

fn connection() -> MysqlConnection {
    MysqlConnection::establish(&database_url()).expect("connect to ignite_test")
}

/// The number of session rows of the user with `user_id`.
fn sessions_of(conn: &mut MysqlConnection, user_id: i64) -> i64 {
    sql_query("SELECT COUNT(*) AS count FROM sessions WHERE user_id = ?")
        .bind::<BigInt, _>(user_id)
        .get_result::<CountRow>(conn)
        .expect("count the sessions")
        .count
}

fn user_id(conn: &mut MysqlConnection, email: &str) -> i64 {
    users::find_by_email(conn, email)
        .unwrap()
        .expect("the user exists")
        .id
}

/// Posts the sign-in form from `client` and returns the status the panel answers with.
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
fn reset_with_ada() -> (std::sync::MutexGuard<'static, ()>, MysqlConnection) {
    let guard = reset();
    let mut conn = connection();
    users::create_user(&mut conn, "Ada", ADA, ADA_PASSWORD).expect("create Ada");
    authz::assign_role(&mut conn, ADA, SUPER_ADMIN).expect("make Ada a super-admin");
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
    let (_guard, mut conn) = reset_with_ada();
    let ada = client(None);
    assert_eq!(sign_in(&ada, ADA, ADA_PASSWORD), Status::SeeOther);
    assert_eq!(ada.get("/admin/widgets").dispatch().status(), Status::Ok);
    let ada_id = user_id(&mut conn, ADA);
    assert_eq!(sessions_of(&mut conn, ada_id), 1);

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
        sessions_of(&mut conn, ada_id),
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
    let (_guard, mut conn) = reset_with_ada();
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
    assert!(users::find_by_email(&mut conn, EMAIL)
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
    let (_guard, mut conn) = reset_with_ada();
    let ada = client(None);
    assert_eq!(sign_in(&ada, ADA, ADA_PASSWORD), Status::SeeOther);
    let ada_id = user_id(&mut conn, ADA);
    assert_eq!(sessions_of(&mut conn, ada_id), 1);

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
    assert_eq!(sessions_of(&mut conn, ada_id), 0);
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
    let (_guard, mut conn) = reset_with_ada();
    let ada = client(None);
    assert_eq!(sign_in(&ada, ADA, ADA_PASSWORD), Status::SeeOther);
    let ada_id = user_id(&mut conn, ADA);
    assert_eq!(sessions_of(&mut conn, ada_id), 1);

    sql_query("DELETE FROM users WHERE id = ?")
        .bind::<BigInt, _>(ada_id)
        .execute(&mut conn)
        .expect("delete the user");

    assert_eq!(
        sessions_of(&mut conn, ada_id),
        0,
        "the sessions go with the user"
    );
    assert_eq!(
        ada.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
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
    let mut conn = connection();
    // Rewind to the state before the account lifecycle migration, so the test can leave an
    // orphan row the way an old database has one.
    common::rewind_to(&BEFORE_LIFECYCLE);
    sql_query(
        "INSERT INTO sessions (id, user_id, expires_at) VALUES ('orphan-session', 999999, NOW() + INTERVAL 1 DAY)",
    )
    .execute(&mut conn)
    .expect("insert an orphan session");

    let migrated = ignite(&["migrate"], "");
    assert_eq!(
        migrated.status.code(),
        Some(0),
        "stderr: {}",
        text(&migrated.stderr)
    );
    let orphans = sql_query("SELECT COUNT(*) AS count FROM sessions WHERE id = 'orphan-session'")
        .get_result::<CountRow>(&mut conn)
        .unwrap()
        .count;
    assert_eq!(
        orphans, 0,
        "the orphan row is removed before the key is added"
    );

    let no_user = sql_query(
        "INSERT INTO sessions (id, user_id, expires_at) VALUES ('no-user', 999999, NOW() + INTERVAL 1 DAY)",
    )
    .execute(&mut conn);
    assert!(no_user.is_err(), "a session must belong to a user");
}
