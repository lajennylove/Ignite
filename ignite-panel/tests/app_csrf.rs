//! An app's own POST routes are checked with the panel's CSRF guard, as Laravel's
//! VerifyCsrfToken checks an app's routes. The exempt list skips the check.

mod common;

use ignite_panel::{CsrfForm, CsrfToken, Ignite};
use rocket::http::{ContentType, Header, Status};
use rocket::local::blocking::Client;
use rocket::{get, post, routes};

#[get("/app/form")]
fn form(token: CsrfToken) -> String {
    token.value().to_string()
}

#[post("/app/form", data = "<form>")]
fn submit(form: CsrfForm) -> String {
    format!("ok:{}", form.get("item").unwrap_or(""))
}

#[post("/app/hook/stripe", data = "<form>")]
fn hook(form: CsrfForm) -> String {
    format!("hook:{}", form.get("event").unwrap_or(""))
}

#[post("/app/callback/paypal", data = "<form>")]
fn callback(form: CsrfForm) -> String {
    format!("callback:{}", form.get("event").unwrap_or(""))
}

fn client() -> Client {
    common::ensure_secret();
    Client::tracked(
        rocket::build()
            .attach(Ignite::new().csrf_exempt(&["/app/hook/*"]).fairing())
            .mount("/", routes![form, submit, hook]),
    )
    .expect("valid rocket")
}

/// The token the app's form page shows, which the browser session keeps.
fn token(client: &Client) -> String {
    client.get("/app/form").dispatch().into_string().unwrap()
}

#[test]
fn an_app_post_without_a_token_is_refused_with_419() {
    let client = client();
    token(&client);
    let response = client
        .post("/app/form")
        .header(ContentType::Form)
        .body("item=book")
        .dispatch();

    assert_eq!(response.status().code, 419);
}

#[test]
fn an_app_post_with_a_wrong_token_is_refused_with_419() {
    let client = client();
    token(&client);
    let response = client
        .post("/app/form")
        .header(ContentType::Form)
        .body("item=book&_token=wrong")
        .dispatch();

    assert_eq!(response.status().code, 419);
}

#[test]
fn an_app_post_with_the_token_is_accepted() {
    let client = client();
    let token = token(&client);
    let response = client
        .post("/app/form")
        .header(ContentType::Form)
        .body(format!("item=book&_token={token}"))
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.into_string().unwrap(), "ok:book");
}

#[test]
fn an_app_post_may_send_the_token_in_a_header() {
    let client = client();
    let token = token(&client);
    let response = client
        .post("/app/form")
        .header(ContentType::Form)
        .header(Header::new("X-CSRF-TOKEN", token))
        .body("item=pen")
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.into_string().unwrap(), "ok:pen");
}

#[test]
fn an_exempt_route_skips_the_check() {
    let client = client();
    let response = client
        .post("/app/hook/stripe")
        .header(ContentType::Form)
        .body("event=paid")
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.into_string().unwrap(), "hook:paid");
}

/// A client whose exempt list comes from two `csrf_exempt` calls.
fn client_with_two_exempt_calls() -> Client {
    common::ensure_secret();
    Client::tracked(
        rocket::build()
            .attach(
                Ignite::new()
                    .csrf_exempt(&["/app/hook/*"])
                    .csrf_exempt(&["/app/callback/*"])
                    .fairing(),
            )
            .mount("/", routes![form, submit, hook, callback]),
    )
    .expect("valid rocket")
}

#[test]
fn a_second_csrf_exempt_call_keeps_the_first_list() {
    let client = client_with_two_exempt_calls();
    let first = client
        .post("/app/hook/stripe")
        .header(ContentType::Form)
        .body("event=paid")
        .dispatch();
    assert_eq!(
        first.status(),
        Status::Ok,
        "the first call's path is exempt"
    );
    assert_eq!(first.into_string().unwrap(), "hook:paid");

    let second = client
        .post("/app/callback/paypal")
        .header(ContentType::Form)
        .body("event=refund")
        .dispatch();
    assert_eq!(
        second.status(),
        Status::Ok,
        "the second call's path is exempt"
    );
    assert_eq!(second.into_string().unwrap(), "callback:refund");
}
