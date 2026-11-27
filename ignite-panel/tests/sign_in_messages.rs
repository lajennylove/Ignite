//! The sign-in form's messages for an empty field, as WordPress's `wp-login.php` words them
//! ("ERROR: The email field is empty." and "ERROR: The password field is empty."), in each
//! field's error slot, with 422, keeping the email typed. Wrong credentials keep their own
//! message. Runs against the test database.

mod common;

use common::*;
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;

const EMPTY_EMAIL: &str = "ERROR: The email field is empty.";
const EMPTY_PASSWORD: &str = "ERROR: The password field is empty.";

fn attempt(client: &Client, body: &str) -> (Status, String) {
    client.get("/admin/login").dispatch();
    let response = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(with_token(client, body))
        .dispatch();
    let status = response.status();
    (status, response.into_string().unwrap_or_default())
}

fn email_slot() -> String {
    format!(
        r#"<p class="mt-1.5 mb-0 text-sm text-error-emphasis" id="email-error">{EMPTY_EMAIL}</p>"#
    )
}

fn password_slot() -> String {
    format!(
        r#"<p class="mt-1.5 mb-0 text-sm text-error-emphasis" id="password-error">{EMPTY_PASSWORD}</p>"#
    )
}

#[test]
fn an_empty_password_shows_its_message_under_the_password_and_keeps_the_email() {
    let _guard = reset();
    let client = client(None);
    let (status, html) = attempt(&client, "email=someone%40example.test&password=");
    assert_eq!(status, Status::UnprocessableEntity);
    let input = html.find(r#"id="password" name="password""#).unwrap();
    assert!(html[input..].starts_with(r#"id="password" name="password" type="password" autocomplete="current-password" aria-invalid="true" aria-describedby="password-error">"#), "{html}");
    assert!(html.find(&password_slot()).unwrap() > input, "{html}");
    assert!(!html.contains(EMPTY_EMAIL), "{html}");
    assert!(html.contains(r#"value="someone@example.test""#), "{html}");
    // Not the credentials message, and no session.
    assert!(
        !html.contains("The email or password is incorrect."),
        "{html}"
    );
    assert!(client.cookies().get("ignite_session").is_none());
}

#[test]
fn an_empty_email_shows_its_message_under_the_email() {
    let _guard = reset();
    let client = client(None);
    let (status, html) = attempt(&client, "email=&password=secret");
    assert_eq!(status, Status::UnprocessableEntity);
    let input = html.find(r#"id="email" name="email""#).unwrap();
    assert!(html.find(&email_slot()).unwrap() > input, "{html}");
    assert!(html.find(&email_slot()).unwrap() < html.find(r#"id="password""#).unwrap());
    assert!(!html.contains(EMPTY_PASSWORD), "{html}");
}

#[test]
fn both_empty_show_both_messages() {
    let _guard = reset();
    let client = client(None);
    let (status, html) = attempt(&client, "email=&password=");
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(html.contains(&email_slot()), "{html}");
    assert!(html.contains(&password_slot()), "{html}");
    // The form's general alert is not shown: the messages are on the fields.
    assert!(!html.contains(r#"role="alert""#), "{html}");
}

#[test]
fn wrong_credentials_keep_their_message() {
    let _guard = reset();
    let client = client(None);
    let (status, html) = attempt(&client, &format!("email={EMAIL}&password=wrong"));
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(
        html.contains("The email or password is incorrect."),
        "{html}"
    );
    assert!(!html.contains("ERROR:"), "{html}");
}

#[test]
fn an_empty_field_does_not_count_against_the_throttle() {
    let _guard = reset();
    let client = client(None);
    for _ in 0..8 {
        let (status, _) = attempt(&client, &format!("email={EMAIL}&password="));
        assert_eq!(status, Status::UnprocessableEntity);
    }
    let t = test_db();
    assert_eq!(t.count("SELECT COUNT(*) FROM login_throttles", &[]), 0);
    // The user can still sign in.
    let (status, _) = attempt(&client, &format!("email={EMAIL}&password={PASSWORD}"));
    assert_eq!(status, Status::SeeOther);
}

#[test]
fn the_sign_in_page_shows_no_field_error_at_first() {
    let _guard = reset();
    let client = client(None);
    let html = client.get("/admin/login").dispatch().into_string().unwrap();
    assert!(!html.contains("ERROR:"));
    assert!(!html.contains("aria-invalid"));
}
