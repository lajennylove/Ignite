//! Failed sign-ins are limited per email and address together, as Laravel's login
//! throttle keys them: five failures in a window refuse that pair, and a refused
//! attempt gets 429 and a Retry-After header.

mod common;

use chrono::{Duration, Utc};
use common::{client, reset, with_token, EMAIL, PASSWORD};
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Datetime, Text};
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
fn another_address_is_not_refused_for_the_same_email() {
    let _guard = reset();
    let client = client(None);
    for _ in 0..5 {
        attempt(&client, EMAIL, "wrong-password", HOME);
    }

    // The key is the email and the address, so the same email from elsewhere is allowed.
    let elsewhere = attempt(&client, EMAIL, PASSWORD, "198.51.100.8:4000");
    assert_eq!(elsewhere.status(), Status::SeeOther);
}

#[test]
fn one_address_does_not_lock_out_other_emails() {
    let _guard = reset();
    let client = client(None);
    let busy = "198.51.100.7:4000";
    for _ in 0..5 {
        attempt(&client, EMAIL, "wrong-password", busy);
    }

    let other = attempt(&client, "someone.else@example.test", "wrong-password", busy);
    assert_eq!(
        other.status(),
        Status::UnprocessableEntity,
        "a different email from the same address is still allowed"
    );
}

#[test]
fn a_failure_deletes_the_rows_whose_window_has_ended() {
    let _guard = reset();
    let mut conn = MysqlConnection::establish(&common::database_url()).unwrap();
    let long_ago = Utc::now().naive_utc() - Duration::hours(1);
    let recent = Utc::now().naive_utc() - Duration::seconds(10);
    for (key, start) in [
        ("stale@example.test|203.0.113.1", long_ago),
        ("live@example.test|203.0.113.1", recent),
    ] {
        sql_query(
            "INSERT INTO login_throttles (throttle_key, hits, window_start) VALUES (?, 2, ?)",
        )
        .bind::<Text, _>(key)
        .bind::<Datetime, _>(start)
        .execute(&mut conn)
        .unwrap();
    }

    let client = client(None);
    attempt(&client, "random@example.test", "wrong-password", HOME);

    let keys: Vec<String> = sql_query("SELECT throttle_key FROM login_throttles")
        .load::<KeyRow>(&mut conn)
        .unwrap()
        .into_iter()
        .map(|row| row.throttle_key)
        .collect();
    assert!(
        !keys.iter().any(|k| k.starts_with("stale@")),
        "stale row pruned: {keys:?}"
    );
    assert!(
        keys.iter().any(|k| k.starts_with("live@")),
        "live row kept: {keys:?}"
    );
}

#[derive(QueryableByName)]
struct KeyRow {
    #[diesel(sql_type = Text)]
    throttle_key: String,
}
