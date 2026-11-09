//! Integration tests for sign-in, sign-out, the session guard and flash messages.

mod common;

use common::*;
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use rocket::http::{ContentType, Cookie, Status};

const SESSION: &str = "ignite_session";

fn login(client: &rocket::local::blocking::Client, email: &str, password: &str) -> Status {
    client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(format!("email={email}&password={password}"))
        .dispatch()
        .status()
}

#[test]
fn a_missing_session_redirects_to_the_login_page() {
    let _guard = reset();
    let client = client(None);
    let response = client.get("/admin/widgets").dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin/login"));
}

#[test]
fn a_write_without_a_session_is_refused_and_changes_nothing() {
    let _guard = reset();
    let client = client(None);
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body("name=Date&price=5")
        .dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin/login"));
    assert_eq!(count_widgets(), 3);
}

#[test]
fn a_wrong_password_is_rejected_and_sets_no_session() {
    let _guard = reset();
    let client = client(None);
    let response = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(format!("email={EMAIL}&password=wrong-password"))
        .dispatch();

    assert_eq!(response.status(), Status::UnprocessableEntity);
    let body = response.into_string().unwrap();
    assert!(body.contains("The email or password is incorrect."));
    assert!(
        !body.contains("wrong-password"),
        "the password is never echoed"
    );
    assert!(client.cookies().get(SESSION).is_none());
    assert_eq!(
        client.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

#[test]
fn an_unknown_email_gets_the_same_message() {
    let _guard = reset();
    let client = client(None);
    let response = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(format!("email=nobody@example.test&password={PASSWORD}"))
        .dispatch();

    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("The email or password is incorrect."));
}

#[test]
fn a_signed_in_user_reaches_the_panel() {
    let _guard = reset();
    let client = client(None);
    assert_eq!(login(&client, EMAIL, PASSWORD), Status::SeeOther);

    let root = client.get("/admin").dispatch();
    assert_eq!(root.status(), Status::SeeOther);
    assert_eq!(root.headers().get_one("Location"), Some("/admin/widgets"));

    let body = client.get("/admin/widgets").dispatch();
    assert_eq!(body.status(), Status::Ok);
    assert!(body.into_string().unwrap().contains("Sign out"));
}

#[test]
fn logout_ends_the_session() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client.post("/admin/logout").dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin/login"));
    let after = client.get("/admin/widgets").dispatch();
    assert_eq!(after.status(), Status::SeeOther);
    assert_eq!(after.headers().get_one("Location"), Some("/admin/login"));
}

#[test]
fn a_forged_session_cookie_is_rejected() {
    let _guard = reset();
    let client = client(None);
    client.cookies().add(Cookie::new(
        SESSION,
        "eyJ1aWQiOjEsImV4cCI6OTk5OTk5OTk5OX0.bm90LWEtdGFn",
    ));

    assert_eq!(
        client.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

#[test]
fn a_session_from_another_app_instance_is_rejected() {
    // Each instance without IGNITE_SECRET_KEY signs with its own random key in
    // debug builds, so a cookie from one instance must not open the other.
    let _guard = reset();
    let first = signed_in(None);
    let cookie = first
        .cookies()
        .get(SESSION)
        .expect("session cookie")
        .value()
        .to_string();

    let second = client(None);
    second.cookies().add(Cookie::new(SESSION, cookie));
    assert_eq!(
        second.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

#[test]
fn the_flash_message_shows_once() {
    let _guard = reset();
    let client = signed_in(None);
    let created = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body("name=Date&price=5")
        .dispatch();
    assert_eq!(created.status(), Status::SeeOther);

    let first = client
        .get("/admin/widgets")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(first.contains("Record created."));
    let second = client
        .get("/admin/widgets")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(!second.contains("Record created."));
}

#[test]
fn the_panel_stores_argon2_hashes_not_passwords() {
    let _guard = reset();
    let mut conn = MysqlConnection::establish(&database_url()).unwrap();
    let hash: String =
        diesel::sql_query("SELECT password_hash AS value FROM users WHERE email = ?")
            .bind::<diesel::sql_types::Text, _>(EMAIL)
            .get_result::<StringValue>(&mut conn)
            .unwrap()
            .value;

    assert!(hash.starts_with("$argon2id$"), "unexpected hash: {hash}");
    assert!(!hash.contains(PASSWORD));
}

#[derive(diesel::QueryableByName)]
struct StringValue {
    #[diesel(sql_type = diesel::sql_types::Text)]
    value: String,
}

#[test]
fn the_login_page_is_reachable_without_a_session() {
    let _guard = reset();
    let client = client(None);
    let response = client.get("/admin/login").dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .unwrap()
        .contains(r#"type="password""#));
}

#[test]
fn the_redirect_follows_the_mount_path() {
    let _guard = reset();
    let client = client(Some("/panel"));
    let response = client.get("/panel/widgets").dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/panel/login"));
}
