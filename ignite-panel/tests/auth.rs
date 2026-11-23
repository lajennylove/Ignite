//! Integration tests for sign-in, sign-out, the session guard and flash messages.

mod common;

use common::*;
use orbit_orm::raw;
use rocket::http::{ContentType, Cookie, Status};

const SESSION: &str = "ignite_session";

fn login(client: &rocket::local::blocking::Client, email: &str, password: &str) -> Status {
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
    client.get("/admin/login").dispatch();
    let response = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            &format!("email={EMAIL}&password=wrong-password"),
        ))
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
    client.get("/admin/login").dispatch();
    let response = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            &format!("email=nobody@example.test&password={PASSWORD}"),
        ))
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

    let dashboard = client.get("/admin").dispatch();
    assert_eq!(dashboard.status(), Status::Ok);
    let body = dashboard.into_string().unwrap();
    assert!(body.contains("Sign out"));
    assert!(
        body.contains("/admin/logout-everywhere\""),
        "the one sign-out button ends every session"
    );
    assert!(
        !body.contains("/admin/logout\""),
        "there is no separate single-session sign-out"
    );
    // The Widgets resource provides a card with the number of rows it holds.
    assert!(body.contains("Widgets"));
    assert!(body.contains(">3</p>"), "the widget card shows the count");
    // The declared table shows its title, its headers, and the two newest rows, newest first.
    assert!(body.contains("Latest widgets"));
    assert!(body.contains(">Name</th>") && body.contains(">Price</th>"));
    let cherry = body.find("Cherry").expect("newest row shown");
    let banana = body.find("Banana").expect("second row shown");
    assert!(cherry < banana, "newest first");
    assert!(!body.contains("Apple"), "only the limit's records show");
}

#[test]
fn the_dashboard_needs_a_session() {
    let _guard = reset();
    let client = client(None);
    let response = client.get("/admin").dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin/login"));
}

#[test]
fn logout_ends_the_session() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .post("/admin/logout")
        .header(ContentType::Form)
        .body(with_token(&client, ""))
        .dispatch();

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
    // A second instance with its own key: the cookie of the first must not open it.
    std::env::set_var("IGNITE_SECRET_KEY", random_key());
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
        .body(with_token(&client, "name=Date&price=5"))
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
    let conn = test_db();
    let hash: String = conn
        .run(raw::fetch_scalar(
            &conn.db,
            "SELECT password_hash FROM users WHERE email = ?",
            &[EMAIL.into()],
        ))
        .expect("read the stored hash");

    assert!(hash.starts_with("$argon2id$"), "unexpected hash: {hash}");
    assert!(!hash.contains(PASSWORD));
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

fn session_value(client: &rocket::local::blocking::Client) -> String {
    client
        .cookies()
        .get(SESSION)
        .expect("session cookie")
        .value()
        .to_string()
}

#[test]
fn a_copied_cookie_stops_working_after_logout() {
    let _guard = reset();
    let signed = signed_in(None);
    let copied = session_value(&signed);
    assert_eq!(
        signed
            .post("/admin/logout")
            .header(ContentType::Form)
            .body(with_token(&signed, ""))
            .dispatch()
            .status(),
        Status::SeeOther
    );

    // The server ended the session, so the copied cookie no longer opens the panel.
    let replay = client(None);
    replay.cookies().add(Cookie::new(SESSION, copied));
    let response = replay.get("/admin/widgets").dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin/login"));
}

#[test]
fn sign_out_everywhere_ends_every_session_of_the_user() {
    let _guard = reset();
    let phone = signed_in(None);
    let laptop = signed_in(None);
    assert_eq!(laptop.get("/admin/widgets").dispatch().status(), Status::Ok);

    let response = phone
        .post("/admin/logout-everywhere")
        .header(ContentType::Form)
        .body(with_token(&phone, ""))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);

    assert_eq!(
        laptop.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
    assert_eq!(
        phone.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

#[test]
fn an_expired_session_row_is_refused() {
    let _guard = reset();
    let client = signed_in(None);
    assert_eq!(client.get("/admin/widgets").dispatch().status(), Status::Ok);

    let conn = test_db();
    conn.exec("UPDATE sessions SET expires_at = '2000-01-01 00:00:00'");

    assert_eq!(
        client.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

#[test]
fn the_sidebar_links_each_resource_with_an_add_link_and_marks_the_current_one() {
    let _guard = reset();
    let client = signed_in(None);
    let body = client
        .get("/admin/widgets")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.contains("href=\"/admin/widgets\""), "the list link");
    assert!(
        body.contains("href=\"/admin/widgets/new\""),
        "the Add new link"
    );
    assert!(
        body.contains("aria-current=\"page\""),
        "the current section is marked"
    );
}

#[test]
fn the_top_bar_links_the_user_to_a_profile_page() {
    let _guard = reset();
    let client = signed_in(None);
    let body = client.get("/admin").dispatch().into_string().unwrap();
    assert!(body.contains("href=\"/admin/profile\""));
    assert!(body.contains(">Admin</a>"), "the user's name is the link");

    let profile = client.get("/admin/profile").dispatch();
    assert_eq!(profile.status(), Status::Ok);
    let page = profile.into_string().unwrap();
    assert!(page.contains(EMAIL), "the profile shows the email");
}

#[test]
fn the_profile_needs_a_session() {
    let _guard = reset();
    let client = client(None);
    let response = client.get("/admin/profile").dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin/login"));
}

#[test]
fn every_user_starts_on_the_light_theme() {
    let _guard = reset();
    let client = signed_in(None);
    let body = client.get("/admin").dispatch().into_string().unwrap();
    assert!(body.contains("data-theme=\"light\""));
}

#[test]
fn a_saved_theme_is_shown_on_the_next_page() {
    let _guard = reset();
    let client = signed_in(None);

    let response = client
        .post("/admin/theme")
        .header(ContentType::Form)
        .body(with_token(&client, "theme=dark"))
        .dispatch();
    assert_eq!(response.status(), Status::NoContent);

    let body = client.get("/admin").dispatch().into_string().unwrap();
    assert!(body.contains("data-theme=\"dark\""));
}

#[test]
fn an_unknown_theme_is_refused() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .post("/admin/theme")
        .header(ContentType::Form)
        .body(with_token(&client, "theme=purple"))
        .dispatch();
    assert_eq!(response.status(), Status::BadRequest);
}

// ---------------------------------------------------------------------------------------------
// "Remember me", as Laravel's: a recaller cookie with the user's `remember_token`, which signs
// the user in again when the session has ended, and a new token on sign-out and on a password
// change.
// ---------------------------------------------------------------------------------------------

const REMEMBER: &str = "ignite_remember";

/// Signs the test user in with the "Remember me" box ticked.
fn remembered() -> rocket::local::blocking::Client {
    let client = client(None);
    client.get("/admin/login").dispatch();
    let body = with_token(
        &client,
        &format!("email={EMAIL}&password={PASSWORD}&remember=on"),
    );
    let status = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(body)
        .dispatch()
        .status();
    assert_eq!(status, Status::SeeOther, "remembered sign-in failed");
    client
}

fn remember_token() -> Option<String> {
    let conn = test_db();
    conn.run(raw::fetch_scalar::<Option<String>>(
        &conn.db,
        "SELECT remember_token FROM users WHERE email = ?",
        &[EMAIL.into()],
    ))
    .expect("read the remember token")
}

/// Ends every session row, as if the normal lifetime had passed.
fn expire_sessions() {
    test_db().exec("UPDATE sessions SET expires_at = '2000-01-01 00:00:00'");
}

/// A fresh browser with a copy of `client`'s remember cookie and nothing else.
fn with_copied_remember_cookie(
    client: &rocket::local::blocking::Client,
) -> rocket::local::blocking::Client {
    let copied = client
        .cookies()
        .get(REMEMBER)
        .expect("remember cookie")
        .value()
        .to_string();
    let other = common::client(None);
    other.cookies().add(Cookie::new(REMEMBER, copied));
    other
}

#[test]
fn the_login_page_has_an_unticked_remember_me_box() {
    let _guard = reset();
    let body = client(None)
        .get("/admin/login")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.contains(r#"name="remember""#));
    assert!(body.contains("Remember me"));
    assert!(
        !body.contains(r#"value="on" checked"#),
        "the box starts unticked"
    );
}

#[test]
fn a_failed_attempt_keeps_the_remember_me_box_ticked() {
    let _guard = reset();
    let client = client(None);
    client.get("/admin/login").dispatch();
    let response = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            &format!("email={EMAIL}&password=wrong-password&remember=on"),
        ))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains(r#"name="remember" value="on" checked"#));
    assert!(client.cookies().get(REMEMBER).is_none());
    assert_eq!(remember_token(), None, "no token for a failed attempt");
}

#[test]
fn an_unticked_sign_in_sets_no_remember_cookie_and_ends_with_the_session() {
    let _guard = reset();
    let client = signed_in(None);
    assert!(client.cookies().get(REMEMBER).is_none());
    assert_eq!(remember_token(), None);

    expire_sessions();
    let response = client.get("/admin/widgets").dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin/login"));
}

#[test]
fn a_ticked_sign_in_sets_a_long_lived_remember_cookie() {
    let _guard = reset();
    let client = client(None);
    client.get("/admin/login").dispatch();
    let response = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            &format!("email={EMAIL}&password={PASSWORD}&remember=on"),
        ))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    let set_cookie: Vec<&str> = response.headers().get("Set-Cookie").collect();
    let remember = set_cookie
        .iter()
        .find(|c| c.starts_with("ignite_remember="))
        .expect("the remember cookie is set");
    // 400 days, Laravel's remember duration of 576000 minutes.
    assert!(remember.contains("Max-Age=34560000"), "{remember}");
    assert!(remember.contains("HttpOnly"), "{remember}");
    let session = set_cookie
        .iter()
        .find(|c| c.starts_with("ignite_session="))
        .expect("the session cookie is set");
    assert!(
        !session.contains("Max-Age"),
        "the session cookie is unchanged: {session}"
    );
    let token = remember_token().expect("the user has a remember token");
    assert_eq!(token.len(), 60);
}

#[test]
fn a_ticked_sign_in_keeps_the_user_signed_in_past_the_session_lifetime() {
    let _guard = reset();
    let client = remembered();
    let first_session = session_value(&client);

    expire_sessions();
    let response = client.get("/admin/widgets").dispatch();
    assert_eq!(response.status(), Status::Ok);
    // The remember cookie started a new session, with the normal lifetime.
    assert_ne!(session_value(&client), first_session);
    let conn = test_db();
    assert_eq!(
        conn.count(
            "SELECT COUNT(*) FROM sessions WHERE expires_at > '2001-01-01 00:00:00'",
            &[]
        ),
        1
    );
    assert_eq!(client.get("/admin").dispatch().status(), Status::Ok);
}

#[test]
fn a_second_remembered_browser_shares_the_token() {
    let _guard = reset();
    let laptop = remembered();
    let token = remember_token();
    let phone = remembered();
    assert_eq!(remember_token(), token, "a new sign-in keeps the token");

    expire_sessions();
    assert_eq!(laptop.get("/admin/widgets").dispatch().status(), Status::Ok);
    assert_eq!(phone.get("/admin/widgets").dispatch().status(), Status::Ok);
}

#[test]
fn a_forged_remember_cookie_is_rejected() {
    let _guard = reset();
    let client = client(None);
    client.cookies().add(Cookie::new(
        REMEMBER,
        "eyJ1aWQiOjEsInRva2VuIjoieCJ9.bm90LWEtdGFn",
    ));
    assert_eq!(
        client.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

#[test]
fn logout_rotates_the_remember_token_and_forgets_the_cookie() {
    let _guard = reset();
    let client = remembered();
    let copy = with_copied_remember_cookie(&client);
    let before = remember_token();

    let response = client
        .post("/admin/logout")
        .header(ContentType::Form)
        .body(with_token(&client, ""))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    assert!(client.cookies().get(REMEMBER).is_none());
    assert_ne!(remember_token(), before, "a new token");
    assert!(remember_token().is_some());

    // The copied cookie holds the old token, so it no longer signs anyone in.
    assert_eq!(
        copy.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

#[test]
fn logout_after_the_session_ended_still_rotates_the_token() {
    let _guard = reset();
    let client = remembered();
    let copy = with_copied_remember_cookie(&client);
    let before = remember_token();

    test_db().exec("DELETE FROM sessions");
    let response = client
        .post("/admin/logout")
        .header(ContentType::Form)
        .body(with_token(&client, ""))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    assert_ne!(remember_token(), before);
    assert_eq!(
        copy.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

#[test]
fn sign_out_everywhere_ends_every_remembered_sign_in() {
    let _guard = reset();
    let phone = remembered();
    let laptop = remembered();
    let before = remember_token();

    let response = phone
        .post("/admin/logout-everywhere")
        .header(ContentType::Form)
        .body(with_token(&phone, ""))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    assert_ne!(remember_token(), before);
    assert!(phone.cookies().get(REMEMBER).is_none());

    // The laptop still has its remember cookie, with the old token.
    assert!(laptop.cookies().get(REMEMBER).is_some());
    assert_eq!(
        laptop.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

#[test]
fn a_password_change_rotates_the_remember_token() {
    let _guard = reset();
    let client = remembered();
    let copy = with_copied_remember_cookie(&client);
    let before = remember_token();

    let body = format!(
        "current_password={PASSWORD}&new_password=a-new-password&confirm_password=a-new-password"
    );
    let response = client
        .post("/admin/profile/password")
        .header(ContentType::Form)
        .body(with_token(&client, &body))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    assert_ne!(remember_token(), before, "a new token");

    // The current session goes on, but no remember cookie from before the change signs in.
    assert_eq!(client.get("/admin/widgets").dispatch().status(), Status::Ok);
    assert_eq!(
        copy.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
    expire_sessions();
    assert_eq!(
        client.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

#[test]
fn a_disabled_user_is_not_signed_in_by_the_remember_cookie() {
    let _guard = reset();
    let client = remembered();
    let conn = test_db();
    conn.exec_with(
        "UPDATE users SET disabled_at = '2026-01-01 00:00:00' WHERE email = ?",
        &[EMAIL.into()],
    );
    expire_sessions();
    assert_eq!(
        client.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

// ---------------------------------------------------------------------------------------------
// The sign-in page is for guests, as Laravel's `guest` middleware (`RedirectIfAuthenticated`):
// a signed-in user who opens it goes to the dashboard.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_signed_in_user_is_sent_from_the_login_page_to_the_dashboard() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client.get("/admin/login").dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin"));
}

#[test]
fn the_login_page_redirect_follows_the_mount_path() {
    let _guard = reset();
    let client = signed_in(Some("/panel"));
    let response = client.get("/panel/login").dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/panel"));
}

#[test]
fn a_signed_out_user_sees_the_login_page() {
    let _guard = reset();
    let client = signed_in(None);
    client
        .post("/admin/logout")
        .header(ContentType::Form)
        .body(with_token(&client, ""))
        .dispatch();
    let response = client.get("/admin/login").dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .unwrap()
        .contains(r#"type="password""#));
}

#[test]
fn a_user_whose_session_ended_without_remember_me_sees_the_login_page() {
    let _guard = reset();
    let client = signed_in(None);
    expire_sessions();
    let response = client.get("/admin/login").dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .unwrap()
        .contains(r#"type="password""#));
}

#[test]
fn a_user_signed_in_by_the_remember_cookie_is_sent_to_the_dashboard() {
    let _guard = reset();
    let client = remembered();
    let first_session = session_value(&client);
    expire_sessions();
    let response = client.get("/admin/login").dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin"));
    // The remember cookie started a new session, as on any other page.
    assert_ne!(session_value(&client), first_session);
    assert_eq!(client.get("/admin").dispatch().status(), Status::Ok);
}

#[test]
fn a_signed_in_user_can_still_post_a_sign_in_attempt() {
    let _guard = reset();
    let client = signed_in(None);
    // The POST is not behind the guest check: a fresh attempt signs in again.
    assert_eq!(login(&client, EMAIL, PASSWORD), Status::SeeOther);
    assert_eq!(client.get("/admin").dispatch().status(), Status::Ok);
}
