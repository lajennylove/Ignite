//! The profile: the details form, the app's own fields, the password, and the picture.

mod common;

use common::{reset, signed_in, storage_dir, with_token, EMAIL, PASSWORD};
use rocket::http::{ContentType, Header, Status};
use rocket::local::blocking::Client;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0\0\x10JFIF";

/// The id of the test user.
fn test_user_id() -> i64 {
    let conn = common::test_db();
    conn.run(ignite_panel::users::find_by_email(&conn.db, EMAIL))
        .expect("read the test user")
        .expect("the test user exists")
        .id
}

/// The `user_avatars` row of the user with `id`: its path and its type.
fn avatar_row(id: i64) -> Option<(String, String)> {
    let conn = common::test_db();
    conn.run(orbit_orm::raw::fetch_all::<(String, String)>(
        &conn.db,
        "SELECT path, mime FROM user_avatars WHERE user_id = ?",
        &[id.into()],
    ))
    .expect("read the avatar row")
    .pop()
}

/// Uploads `file` as the signed-in user's picture.
fn upload(client: &Client, file: &[u8]) {
    let token = common::csrf_token(client);
    let (header, body) = picture_upload(&token, file);
    let response = client
        .post("/admin/profile/avatar")
        .header(header)
        .body(body)
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
}

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
    let conn = common::test_db();
    conn.run(ignite_panel::users::create_user(
        &conn.db,
        "Other",
        email,
        "another password",
    ))
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

    // The racer's insert is uncommitted until its transaction ends, half a second after it
    // signals that the insert ran. The save goes ahead meanwhile, so its update waits on it.
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let committer = std::thread::spawn(move || {
        let racer = common::test_db();
        racer.run(async {
            racer
                .db
                .transaction(async |tx| {
                    orbit_orm::raw::execute(
                        &mut *tx,
                        "INSERT INTO users (name, email, password_hash) VALUES ('Racer', 'race@example.test', 'x')",
                        &[],
                    )
                    .await?;
                    ready_tx.send(()).expect("signal the insert");
                    rocket::tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    Ok::<(), orbit_orm::Error>(())
                })
                .await
                .expect("commit the racer");
        });
    });
    ready_rx.recv().expect("the insert ran");

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

    // The file is on disk, named from the user's id; the row keeps its path, relative to the
    // storage directory, and its type.
    let id = test_user_id();
    let file = storage_dir().join(format!("avatars/{id}.png"));
    assert_eq!(std::fs::read(&file).expect("the picture is on disk"), PNG);
    assert_eq!(
        avatar_row(id),
        Some((format!("avatars/{id}.png"), "image/png".to_string()))
    );

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
    assert!(!file.exists(), "removing the picture removes its file");
    assert_eq!(avatar_row(id), None);
}

#[test]
fn a_picture_of_another_type_replaces_the_file() {
    let _guard = reset();
    let client = signed_in(None);
    let id = test_user_id();
    upload(&client, PNG);
    upload(&client, JPEG);

    let png = storage_dir().join(format!("avatars/{id}.png"));
    let jpg = storage_dir().join(format!("avatars/{id}.jpg"));
    assert!(!png.exists(), "the old file is removed");
    assert_eq!(std::fs::read(&jpg).expect("the new file"), JPEG);
    assert_eq!(
        avatar_row(id),
        Some((format!("avatars/{id}.jpg"), "image/jpeg".to_string()))
    );
    let served = client.get("/admin/profile/avatar?v=2").dispatch();
    assert_eq!(served.status(), Status::Ok);
    assert_eq!(served.content_type(), Some(ContentType::JPEG));
    assert_eq!(served.into_bytes().unwrap(), JPEG);
    let names: Vec<_> = std::fs::read_dir(storage_dir().join("avatars"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, [format!("{id}.jpg")], "no temporary file is left");
}

#[test]
fn a_failed_save_keeps_the_earlier_picture() {
    let _guard = reset();
    let client = signed_in(None);
    let id = test_user_id();
    upload(&client, PNG);
    let file = storage_dir().join(format!("avatars/{id}.png"));

    // The row cannot be written while its `mime` column is missing, so the transaction fails.
    let conn = common::test_db();
    conn.exec("ALTER TABLE user_avatars CHANGE mime mime_gone VARCHAR(32) NOT NULL");
    let other_png: Vec<u8> = [PNG, b"another picture"].concat();
    let token = common::csrf_token(&client);
    let (header, body) = picture_upload(&token, &other_png);
    let response = client
        .post("/admin/profile/avatar")
        .header(header)
        .body(body)
        .dispatch();
    conn.exec("ALTER TABLE user_avatars CHANGE mime_gone mime VARCHAR(32) NOT NULL");
    assert_eq!(response.status(), Status::InternalServerError);

    assert_eq!(
        std::fs::read(&file).expect("the earlier picture is kept"),
        PNG,
        "the earlier file keeps its bytes"
    );
    let names: Vec<_> = std::fs::read_dir(storage_dir().join("avatars"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, [format!("{id}.png")], "no temporary file is left");
    let served = client.get("/admin/profile/avatar?v=2").dispatch();
    assert_eq!(served.into_bytes().unwrap(), PNG);
}

#[test]
fn a_stored_path_outside_the_storage_directory_is_not_served() {
    let _guard = reset();
    let client = signed_in(None);
    let id = test_user_id();
    upload(&client, PNG);

    // A file next to the storage directory, which a path with `..` would reach.
    let outside = format!("ignite-test-outside-{}.png", std::process::id());
    let outside_file = storage_dir().parent().unwrap().join(&outside);
    std::fs::write(&outside_file, PNG).unwrap();
    let conn = common::test_db();
    for path in [
        format!("../{outside}"),
        format!("avatars/../../{outside}"),
        outside_file.to_string_lossy().into_owned(),
    ] {
        conn.exec_with(
            "UPDATE user_avatars SET path = ? WHERE user_id = ?",
            &[path.as_str().into(), id.into()],
        );
        assert_eq!(
            client.get("/admin/profile/avatar?v=1").dispatch().status(),
            Status::NotFound,
            "{path} is refused"
        );
    }
    std::fs::remove_file(outside_file).unwrap();
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
