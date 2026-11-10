//! A database failure while the panel reads the session is a 503, not a sign-out.
//! A missing or unknown session is still a 401, which the panel answers with the
//! login redirect.

mod common;

use common::{client, reset, signed_in};
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::sql_query;
use rocket::http::Status;

#[test]
fn a_failed_session_query_is_503_and_does_not_redirect_to_login() {
    let _guard = reset();
    let client = signed_in(None);
    let mut conn = MysqlConnection::establish(&common::database_url()).expect("connect");

    // Make the session lookup fail. The table comes back before any assertion runs,
    // so a failed assertion does not leave the next test without its table.
    sql_query("RENAME TABLE sessions TO sessions_away")
        .execute(&mut conn)
        .unwrap();
    let response = client.get("/admin").dispatch();
    let status = response.status();
    let location = response.headers().get_one("Location").map(str::to_string);
    sql_query("RENAME TABLE sessions_away TO sessions")
        .execute(&mut conn)
        .unwrap();

    assert_eq!(status, Status::ServiceUnavailable);
    assert!(
        location.is_none(),
        "an outage must not redirect to the login page, got {location:?}"
    );
}

#[test]
fn a_missing_session_is_still_a_redirect_to_login() {
    let _guard = reset();
    let client = client(None);
    let response = client.get("/admin").dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    assert!(response
        .headers()
        .get_one("Location")
        .unwrap_or_default()
        .ends_with("/admin/login"));
}
