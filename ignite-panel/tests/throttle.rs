//! Failed sign-ins are limited per email and per IP, as Laravel's throttle does on
//! the login route. A refused attempt gets 429 and a Retry-After header.

mod common;

use common::{client, reset, with_token, EMAIL, PASSWORD};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::{Client, LocalResponse};

/// One sign-in attempt from `remote`, with a fresh token from the login page.
fn attempt<'c>(client: &'c Client, email: &str, password: &str, remote: &str) -> LocalResponse<'c> {
    client.get("/admin/login").dispatch();
    let body = with_token(client, &format!("email={email}&password={password}"));
    client
        .post("/admin/login")
        .remote(remote.parse::<std::net::SocketAddr>().unwrap())
        .header(ContentType::Form)
        .body(body)
        .dispatch()
}

const HOME: &str = "203.0.113.10:4000";

#[test]
fn five_failures_refuse_the_email_even_with_the_right_password() {
    let _guard = reset();
    let client = client(None);
    for _ in 0..5 {
        assert_eq!(
            attempt(&client, EMAIL, "wrong-password", HOME).status(),
            Status::UnprocessableEntity
        );
    }

    let locked = attempt(&client, EMAIL, PASSWORD, HOME);
    assert_eq!(locked.status(), Status::TooManyRequests);
    let retry: i64 = locked
        .headers()
        .get_one("Retry-After")
        .expect("a Retry-After header")
        .parse()
        .unwrap();
    assert!((1..=60).contains(&retry), "retry after {retry} seconds");
    assert!(locked
        .into_string()
        .unwrap()
        .contains("Too many sign-in attempts"));
}

#[test]
fn another_email_is_not_affected_by_a_locked_one() {
    let _guard = reset();
    let client = client(None);
    for _ in 0..5 {
        attempt(&client, EMAIL, "wrong-password", HOME);
    }

    let other = attempt(&client, "someone.else@example.test", "wrong-password", HOME);
    assert_eq!(
        other.status(),
        Status::UnprocessableEntity,
        "a different email is still allowed"
    );
}

#[test]
fn the_lock_holds_across_app_instances() {
    let _guard = reset();
    let first = client(None);
    for _ in 0..5 {
        attempt(&first, EMAIL, "wrong-password", HOME);
    }

    // A new instance stands for a restarted app or a second process: the count is in the database.
    let restarted = client(None);
    assert_eq!(
        attempt(&restarted, EMAIL, PASSWORD, HOME).status(),
        Status::TooManyRequests
    );
}

#[test]
fn a_successful_sign_in_clears_the_email_count() {
    let _guard = reset();
    let client = client(None);
    for _ in 0..4 {
        attempt(&client, EMAIL, "wrong-password", HOME);
    }
    assert_eq!(
        attempt(&client, EMAIL, PASSWORD, HOME).status(),
        Status::SeeOther
    );

    for _ in 0..4 {
        assert_eq!(
            attempt(&client, EMAIL, "wrong-password", HOME).status(),
            Status::UnprocessableEntity,
            "the count started again"
        );
    }
}

#[test]
fn an_address_is_limited_across_emails() {
    let _guard = reset();
    let client = client(None);
    let busy = "198.51.100.7:4000";
    for n in 0..20 {
        let email = format!("user{n}@example.test");
        assert_eq!(
            attempt(&client, &email, "wrong-password", busy).status(),
            Status::UnprocessableEntity
        );
    }

    let refused = attempt(&client, "user99@example.test", "wrong-password", busy);
    assert_eq!(
        refused.status(),
        Status::TooManyRequests,
        "the address reached its limit"
    );

    // Another address is unaffected.
    let quiet = attempt(
        &client,
        "user99@example.test",
        "wrong-password",
        "198.51.100.8:4000",
    );
    assert_eq!(quiet.status(), Status::UnprocessableEntity);
}
