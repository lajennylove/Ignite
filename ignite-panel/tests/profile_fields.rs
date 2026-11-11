//! The app's own profile fields: a password is stored as a hash and never shown, a blank
//! password keeps the stored hash, and numbers and dates are checked as resource fields are.

mod common;

use common::{reset, test_db, with_token, Widgets, EMAIL, PASSWORD};
use ignite_core::{Field, FieldKind};
use ignite_panel::{users, Ignite};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;

const API_KEY: Field = Field {
    name: "api_key",
    label: "API key",
    kind: FieldKind::Password,
    required: false,
};
const SHOE_SIZE: Field = Field {
    name: "shoe_size",
    label: "Shoe size",
    kind: FieldKind::Number,
    required: false,
};
const BIRTHDAY: Field = Field {
    name: "birthday",
    label: "Birthday",
    kind: FieldKind::Date,
    required: false,
};

/// The panel with the `Widgets` resource and the three app fields above, signed in as the test user.
fn signed_in() -> Client {
    common::ensure_secret();
    let rocket = rocket::build().attach(
        Ignite::new()
            .resource::<Widgets>()
            .profile_field(API_KEY)
            .profile_field(SHOE_SIZE)
            .profile_field(BIRTHDAY)
            .fairing(),
    );
    let client = Client::tracked(rocket).expect("valid rocket");
    client.get("/admin/login").dispatch();
    let body = with_token(&client, &format!("email={EMAIL}&password={PASSWORD}"));
    let status = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(body)
        .dispatch()
        .status();
    assert_eq!(status, Status::SeeOther);
    client
}

fn save<'c>(client: &'c Client, body: &str) -> rocket::local::blocking::LocalResponse<'c> {
    let body = with_token(client, &format!("name=Ada&email=ada%40example.test&{body}"));
    client
        .post("/admin/profile")
        .header(ContentType::Form)
        .body(body)
        .dispatch()
}

/// The stored profile JSON of the test user, read straight from the database.
fn stored_profile() -> String {
    let conn = test_db();
    conn.run(users::find_by_email(&conn.db, "ada@example.test"))
        .unwrap()
        .expect("the user")
        .profile
        .expect("a profile")
}

#[test]
fn a_password_profile_field_is_stored_as_a_hash_and_never_shown() {
    let _guard = reset();
    let client = signed_in();
    let response = save(
        &client,
        "api_key=plain-secret-xyz&shoe_size=42&birthday=2020-12-31",
    );
    assert_eq!(response.status(), Status::SeeOther);

    let stored = stored_profile();
    assert!(!stored.contains("plain-secret-xyz"), "stored: {stored}");
    assert!(stored.contains("$argon2id$"), "stored: {stored}");

    let page = client
        .get("/admin/profile")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(!page.contains("plain-secret-xyz"));
    assert!(!page.contains("$argon2id$"));
}

#[test]
fn a_blank_password_keeps_the_stored_hash() {
    let _guard = reset();
    let client = signed_in();
    save(
        &client,
        "api_key=plain-secret-xyz&shoe_size=42&birthday=2020-12-31",
    );
    let first: serde_json::Value = serde_json::from_str(&stored_profile()).unwrap();

    save(&client, "api_key=&shoe_size=43&birthday=2020-12-31");
    let second: serde_json::Value = serde_json::from_str(&stored_profile()).unwrap();

    assert_eq!(first["api_key"], second["api_key"], "the hash is kept");
    assert_eq!(second["shoe_size"], "43");
    let hash = second["api_key"].as_str().unwrap();
    assert!(ignite_core::verify_password(hash, "plain-secret-xyz"));
}

#[test]
fn a_number_and_a_date_are_checked_like_resource_fields() {
    let _guard = reset();
    let client = signed_in();

    let bad_number = save(&client, "shoe_size=big&birthday=2020-12-31");
    assert_eq!(bad_number.status(), Status::UnprocessableEntity);
    assert!(bad_number
        .into_string()
        .unwrap()
        .contains("Shoe size must be a number."));

    // A decimal is a number too, as in Filament's numeric input and Laravel's numeric rule.
    let decimal = save(&client, "shoe_size=42.5&birthday=2020-12-31");
    assert_eq!(decimal.status(), Status::SeeOther);
    assert!(stored_profile().contains("\"shoe_size\":\"42.5\""));
    let page = client
        .get("/admin/profile")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        page.contains(r#"type="number" step="any""#),
        "a number input accepts decimals in the browser"
    );

    let bad_date = save(&client, "shoe_size=42&birthday=31%2F12%2F2020");
    assert_eq!(bad_date.status(), Status::UnprocessableEntity);
    assert!(bad_date
        .into_string()
        .unwrap()
        .contains("Birthday must be a date, as YYYY-MM-DD."));
}
