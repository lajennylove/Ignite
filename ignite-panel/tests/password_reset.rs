//! Tests for the password reset by email. Every test sends through a `RecordingMailer`, so
//! nothing reaches the network. They run against `ignite_test`.

mod common;

use common::*;
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text};
use ignite_panel::authz::{self, SUPER_ADMIN};
use ignite_panel::{users, Message, RecordingMailer};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;

const ADA: &str = "ada@example.test";
const ADA_PASSWORD: &str = "a-long-password";
const NEW_PASSWORD: &str = "a-brand-new-password";

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn connection() -> MysqlConnection {
    MysqlConnection::establish(&database_url()).expect("connect to ignite_test")
}

/// Creates Ada, a super-admin, next to the test user.
fn reset_with_ada() -> (std::sync::MutexGuard<'static, ()>, MysqlConnection) {
    let guard = reset();
    let mut conn = connection();
    users::create_user(&mut conn, "Ada", ADA, ADA_PASSWORD).expect("create Ada");
    authz::assign_role(&mut conn, ADA, SUPER_ADMIN).expect("make Ada a super-admin");
    (guard, conn)
}

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

/// Posts the forgot form for `email` and returns the status and the redirect target.
fn request_link(client: &Client, email: &str) -> (Status, Option<String>) {
    client.get("/admin/password/forgot").dispatch();
    let body = with_token(client, &format!("email={email}"));
    let response = client
        .post("/admin/password/forgot")
        .header(ContentType::Form)
        .body(body)
        .dispatch();
    let location = response.headers().get_one("Location").map(str::to_string);
    (response.status(), location)
}

/// The path of the reset link in a sent message, such as `/admin/password/reset/<token>?email=...`.
fn link_path(message: &Message) -> String {
    let start = message
        .text
        .find("/admin/password/reset/")
        .expect("the message carries a reset link");
    let rest = &message.text[start..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    rest[..end].to_string()
}

fn token_of(path: &str) -> String {
    let after = path.trim_start_matches("/admin/password/reset/");
    after.split('?').next().unwrap().to_string()
}

/// Posts the reset form for `token` and `email` with `password` twice.
fn reset_with(client: &Client, token: &str, email: &str, password: &str) -> Status {
    client.get("/admin/password/forgot").dispatch();
    let body = with_token(
        client,
        &format!(
            "token={token}&email={email}&password={password}&password_confirmation={password}"
        ),
    );
    client
        .post("/admin/password/reset")
        .header(ContentType::Form)
        .body(body)
        .dispatch()
        .status()
}

/// Posts the sign-in form and returns the status it answers with.
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

#[test]
fn forgot_sends_one_message_to_an_existing_user_with_a_working_link() {
    let (_guard, _conn) = reset_with_ada();
    let mailer = RecordingMailer::new();
    let client = client_with_mailer(None, mailer.clone());

    let (status, location) = request_link(&client, ADA);
    assert_eq!(status, Status::SeeOther);
    assert_eq!(location.as_deref(), Some("/admin/password/forgot"));

    let sent = mailer.sent();
    assert_eq!(sent.len(), 1, "exactly one message is sent");
    let message = &sent[0];
    assert_eq!(message.to, ADA);
    assert_eq!(message.subject, "Reset Password Notification");
    assert!(
        message.idempotency_key.is_some(),
        "the send carries an idempotency key"
    );
    assert!(message.html.contains("/admin/password/reset/"));

    let path = link_path(message);
    let client2 = client_with_mailer(None, mailer.clone());
    let form = client2.get(path.clone()).dispatch();
    assert_eq!(form.status(), Status::Ok, "the link opens the reset form");

    let token = token_of(&path);
    let status = reset_with(&client2, &token, ADA, NEW_PASSWORD);
    assert_eq!(status, Status::SeeOther, "the link sets a new password");

    let client3 = common::client(None);
    assert_eq!(sign_in(&client3, ADA, NEW_PASSWORD), Status::SeeOther);
    let client4 = common::client(None);
    assert_eq!(
        sign_in(&client4, ADA, ADA_PASSWORD),
        Status::UnprocessableEntity,
        "the old password no longer works"
    );
}

#[test]
fn an_unknown_email_sends_nothing_and_gets_the_same_answer() {
    let (_guard, _conn) = reset_with_ada();
    let mailer = RecordingMailer::new();
    let client = client_with_mailer(None, mailer.clone());

    let (known_status, known_location) = request_link(&client, ADA);
    let other = client_with_mailer(None, mailer.clone());
    let (unknown_status, unknown_location) = request_link(&other, "nobody@example.test");

    assert_eq!(known_status, unknown_status);
    assert_eq!(known_location, unknown_location);
    assert_eq!(mailer.sent().len(), 1, "only the known email got a message");
    assert!(mailer.sent().iter().all(|m| m.to == ADA));

    let page = other.get("/admin/password/forgot").dispatch();
    let body = page.into_string().unwrap();
    assert!(body
        .contains("If an account exists for that email, we have emailed a password reset link."));
}

#[test]
fn a_second_request_within_a_minute_sends_nothing_new() {
    let (_guard, _conn) = reset_with_ada();
    let mailer = RecordingMailer::new();
    let client = client_with_mailer(None, mailer.clone());

    let (first, _) = request_link(&client, ADA);
    let (second, _) = request_link(&client, ADA);
    assert_eq!(first, Status::SeeOther);
    assert_eq!(second, Status::SeeOther, "a repeat gets the same answer");
    assert_eq!(
        mailer.sent().len(),
        1,
        "the repeat does not send a second link"
    );
}

#[test]
fn a_failing_mailer_does_not_change_the_answer() {
    let (_guard, _conn) = reset_with_ada();
    // The default mailer refuses to send, so the send fails for an existing user.
    let client = client(None);
    let (status, location) = request_link(&client, ADA);
    assert_eq!(status, Status::SeeOther);
    assert_eq!(location.as_deref(), Some("/admin/password/forgot"));
}

#[test]
fn an_expired_link_is_rejected() {
    let (_guard, mut conn) = reset_with_ada();
    let mailer = RecordingMailer::new();
    let client = client_with_mailer(None, mailer.clone());
    request_link(&client, ADA);
    let path = link_path(&mailer.sent()[0]);
    let token = token_of(&path);

    sql_query("UPDATE password_reset_tokens SET created_at = created_at - INTERVAL 2 HOUR WHERE email = ?")
        .bind::<Text, _>(ADA)
        .execute(&mut conn)
        .unwrap();

    let status = reset_with(&client, &token, ADA, NEW_PASSWORD);
    assert_eq!(status, Status::UnprocessableEntity);
    let id = user_id(&mut conn, ADA);
    assert!(users::verify_password(
        &users::find_by_id(&mut conn, id)
            .unwrap()
            .unwrap()
            .password_hash,
        ADA_PASSWORD
    ));
}

#[test]
fn a_link_works_once() {
    let (_guard, _conn) = reset_with_ada();
    let mailer = RecordingMailer::new();
    let client = client_with_mailer(None, mailer.clone());
    request_link(&client, ADA);
    let token = token_of(&link_path(&mailer.sent()[0]));

    assert_eq!(
        reset_with(&client, &token, ADA, NEW_PASSWORD),
        Status::SeeOther
    );
    assert_eq!(
        reset_with(&client, &token, ADA, "another-new-password"),
        Status::UnprocessableEntity,
        "the used link is refused"
    );
    let mut conn = connection();
    let rows = sql_query("SELECT COUNT(*) AS count FROM password_reset_tokens WHERE email = ?")
        .bind::<Text, _>(ADA)
        .get_result::<CountRow>(&mut conn)
        .unwrap()
        .count;
    assert_eq!(rows, 0, "the used link is deleted");
}

#[test]
fn a_reset_ends_every_session_of_the_user() {
    let (_guard, mut conn) = reset_with_ada();
    let id = user_id(&mut conn, ADA);
    let ada = signed_in_as(None, ADA, ADA_PASSWORD);
    assert!(sessions_of(&mut conn, id) > 0, "Ada is signed in");

    let mailer = RecordingMailer::new();
    let forgot = client_with_mailer(None, mailer.clone());
    request_link(&forgot, ADA);
    let token = token_of(&link_path(&mailer.sent()[0]));
    assert_eq!(
        reset_with(&forgot, &token, ADA, NEW_PASSWORD),
        Status::SeeOther
    );

    assert_eq!(sessions_of(&mut conn, id), 0, "every session of Ada ended");
    let page = ada.get("/admin").dispatch();
    assert_eq!(
        page.status(),
        Status::SeeOther,
        "Ada's old session no longer opens the panel"
    );
}

#[test]
fn the_forgot_endpoint_is_throttled() {
    let (_guard, _conn) = reset_with_ada();
    let mailer = RecordingMailer::new();
    let client = client_with_mailer(None, mailer.clone());

    for _ in 0..5 {
        let (status, _) = request_link(&client, ADA);
        assert_eq!(status, Status::SeeOther);
    }
    client.get("/admin/password/forgot").dispatch();
    let body = with_token(&client, &format!("email={ADA}"));
    let refused = client
        .post("/admin/password/forgot")
        .header(ContentType::Form)
        .body(body)
        .dispatch();
    assert_eq!(refused.status(), Status::TooManyRequests);
    assert!(refused.headers().get_one("Retry-After").is_some());
    assert_eq!(mailer.sent().len(), 1, "the refused request sends nothing");
}

#[test]
fn a_disabled_user_gets_no_link_and_the_same_answer() {
    let (_guard, mut conn) = reset_with_ada();
    let id = user_id(&mut conn, ADA);
    users::disable(&mut conn, id).expect("disable Ada");

    let mailer = RecordingMailer::new();
    let client = client_with_mailer(None, mailer.clone());
    let (status, location) = request_link(&client, ADA);
    assert_eq!(status, Status::SeeOther);
    assert_eq!(location.as_deref(), Some("/admin/password/forgot"));
    assert!(mailer.sent().is_empty(), "a disabled user is sent nothing");
}

#[test]
fn a_weak_new_password_is_refused_on_the_reset_form() {
    let (_guard, _conn) = reset_with_ada();
    let mailer = RecordingMailer::new();
    let client = client_with_mailer(None, mailer.clone());
    request_link(&client, ADA);
    let token = token_of(&link_path(&mailer.sent()[0]));

    let status = reset_with(&client, &token, ADA, "short");
    assert_eq!(status, Status::UnprocessableEntity);
    // The link is still good, so a corrected password works.
    assert_eq!(
        reset_with(&client, &token, ADA, NEW_PASSWORD),
        Status::SeeOther
    );
}

#[test]
fn the_sign_in_page_links_to_the_forgot_page() {
    let (_guard, _conn) = reset_with_ada();
    let client = client(None);
    let body = client.get("/admin/login").dispatch().into_string().unwrap();
    assert!(body.contains("href=\"/admin/password/forgot\""));
    assert!(body.contains("Forgot password?"));
}
