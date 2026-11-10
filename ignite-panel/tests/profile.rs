//! The profile: the details form, the app's own fields, the password, and the picture.

mod common;

use common::{reset, signed_in, with_token, PASSWORD};
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::sql_query;
use rocket::http::{ContentType, Header, Status};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

/// A multipart body with the token and one picture, and its Content-Type header.
fn picture_upload(token: &str, file: &[u8]) -> (Header<'static>, Vec<u8>) {
    let boundary = "igtestboundary";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"_token\"\r\n\r\n{token}\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"me.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(file);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let header = Header::new(
        "Content-Type",
        format!("multipart/form-data; boundary={boundary}"),
    );
    (header, body)
}

fn another_user(email: &str) {
    use diesel::mysql::MysqlConnection;
    let mut conn =
        MysqlConnection::establish(&common::database_url()).expect("connect to ignite_test");
    ignite_panel::users::create_user(&mut conn, "Other", email, "another password")
        .expect("create the other user");
}

#[test]
fn saving_the_details_stores_the_name_email_and_the_app_field() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .post("/admin/profile")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            "name=Ada+Lovelace&email=ada%40example.test&job_title=Engineer",
        ))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);

    let page = client
        .get("/admin/profile")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(page.contains("value=\"Ada Lovelace\""));
    assert!(page.contains("value=\"ada@example.test\""));
    assert!(
        page.contains("value=\"Engineer\""),
        "the app's field keeps its value"
    );
    assert!(page.contains("Profile saved."));
}

#[test]
fn an_email_another_user_has_is_refused() {
    let _guard = reset();
    another_user("other@example.test");
    let client = signed_in(None);
    let response = client
        .post("/admin/profile")
        .header(ContentType::Form)
        .body(with_token(&client, "name=Admin&email=other%40example.test"))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    let page = response.into_string().unwrap();
    assert!(page.contains("Another user already has this email."));
}

/// Two saves race: the other user's email is not yet visible to the check, because
/// that user's insert is still uncommitted on another connection. The save's update
/// then waits on the unique index, and it fails with a duplicate key once the insert
/// commits. That must be the same field error (422), not a server error.
#[test]
fn an_email_taken_between_the_check_and_the_update_is_a_field_error() {
    let _guard = reset();
    let client = signed_in(None);

    let mut other =
        MysqlConnection::establish(&common::database_url()).expect("connect to ignite_test");
    sql_query("START TRANSACTION").execute(&mut other).unwrap();
    sql_query(
        "INSERT INTO users (name, email, password_hash) VALUES ('Racer', 'race@example.test', 'x')",
    )
    .execute(&mut other)
    .unwrap();
    let committer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        sql_query("COMMIT").execute(&mut other).unwrap();
    });

    let response = client
        .post("/admin/profile")
        .header(ContentType::Form)
        .body(with_token(&client, "name=Admin&email=race%40example.test"))
        .dispatch();
    committer.join().unwrap();

    assert_eq!(response.status(), Status::UnprocessableEntity);
    let page = response.into_string().unwrap();
    assert!(page.contains("Another user already has this email."));
}

#[test]
fn a_password_change_needs_the_current_password() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .post("/admin/profile/password")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            "current_password=wrong-one&new_password=a-new-password&confirm_password=a-new-password",
        ))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    let page = response.into_string().unwrap();
    assert!(page.contains("The current password is incorrect."));
}

#[test]
fn a_new_password_signs_out_the_other_devices() {
    let _guard = reset();
    let phone = signed_in(None);
    let laptop = signed_in(None);
    let body = format!(
        "current_password={PASSWORD}&new_password=a-new-password&confirm_password=a-new-password"
    );
    let response = laptop
        .post("/admin/profile/password")
        .header(ContentType::Form)
        .body(with_token(&laptop, &body))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);

    assert_eq!(
        phone.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
    assert_eq!(laptop.get("/admin/widgets").dispatch().status(), Status::Ok);
}

#[test]
fn a_picture_is_stored_shown_and_removed() {
    let _guard = reset();
    let client = signed_in(None);
    let token = common::csrf_token(&client);
    let (header, body) = picture_upload(&token, PNG);
    let response = client
        .post("/admin/profile/avatar")
        .header(header)
        .body(body)
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);

    let served = client.get("/admin/profile/avatar?v=1").dispatch();
    assert_eq!(served.status(), Status::Ok);
    assert_eq!(served.content_type(), Some(ContentType::PNG));
    assert_eq!(served.into_bytes().unwrap(), PNG);

    let page = client
        .get("/admin/profile")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        page.contains("/admin/profile/avatar?v="),
        "the picture replaces the initials"
    );

    let token = common::csrf_token(&client);
    client
        .post("/admin/profile/avatar/remove")
        .header(ContentType::Form)
        .body(format!("_token={token}"))
        .dispatch();
    assert_eq!(
        client.get("/admin/profile/avatar?v=2").dispatch().status(),
        Status::NotFound
    );
}

#[test]
fn a_file_that_is_not_a_picture_is_not_stored() {
    let _guard = reset();
    let client = signed_in(None);
    let token = common::csrf_token(&client);
    let (header, body) = picture_upload(&token, b"<script>alert(1)</script>");
    client
        .post("/admin/profile/avatar")
        .header(header)
        .body(body)
        .dispatch();
    assert_eq!(
        client.get("/admin/profile/avatar?v=1").dispatch().status(),
        Status::NotFound
    );
}

/// Posts a password change with the given current password, from the signed-in client.
fn change_with<'c>(
    client: &'c rocket::local::blocking::Client,
    current: &str,
    new: &str,
) -> rocket::local::blocking::LocalResponse<'c> {
    let body = with_token(
        client,
        &format!("current_password={current}&new_password={new}&confirm_password={new}"),
    );
    client
        .post("/admin/profile/password")
        .header(ContentType::Form)
        .body(body)
        .dispatch()
}

#[test]
fn a_password_change_is_refused_after_repeated_wrong_current_passwords() {
    let _guard = reset();
    let client = signed_in(None);
    for _ in 0..5 {
        assert_eq!(
            change_with(&client, "wrong-one", "a-new-password").status(),
            Status::UnprocessableEntity
        );
    }

    // Even the right current password is refused while the limit holds.
    let refused = change_with(&client, PASSWORD, "a-new-password");
    assert_eq!(refused.status(), Status::TooManyRequests);
    assert!(refused.headers().get_one("Retry-After").is_some());
    assert!(refused.into_string().unwrap().contains("Too many attempts"));
}

#[test]
fn a_successful_password_change_clears_the_count() {
    let _guard = reset();
    let client = signed_in(None);
    for _ in 0..4 {
        change_with(&client, "wrong-one", "a-new-password");
    }
    assert_eq!(
        change_with(&client, PASSWORD, "a-new-password").status(),
        Status::SeeOther
    );

    // The count started again, so four more wrong guesses are still allowed.
    for _ in 0..4 {
        assert_eq!(
            change_with(&client, "wrong-one", "another-password").status(),
            Status::UnprocessableEntity
        );
    }
}

#[test]
fn a_password_change_does_not_clear_the_sign_in_count() {
    let _guard = reset();
    let client = signed_in(None);
    // Four failed sign-ins from this client's address leave the sign-in count at four.
    for _ in 0..4 {
        client.get("/admin/login").dispatch();
        let body = with_token(
            &client,
            &format!("email={}&password=wrong-one", common::EMAIL),
        );
        assert_eq!(
            client
                .post("/admin/login")
                .header(ContentType::Form)
                .body(body)
                .dispatch()
                .status(),
            Status::UnprocessableEntity
        );
    }

    assert_eq!(
        change_with(&client, PASSWORD, "a-new-password").status(),
        Status::SeeOther
    );

    // The sign-in count is still four: one more failure is the fifth, and the next is refused.
    let sign_in = |password: &str| {
        client.get("/admin/login").dispatch();
        let body = with_token(
            &client,
            &format!("email={}&password={password}", common::EMAIL),
        );
        client
            .post("/admin/login")
            .header(ContentType::Form)
            .body(body)
            .dispatch()
            .status()
    };
    assert_eq!(sign_in("wrong-one"), Status::UnprocessableEntity);
    assert_eq!(sign_in("wrong-one"), Status::TooManyRequests);
}
