//! Debug builds only: an app's own view extends the panel layout and renders through
//! the dev renderer. The panel registers its own assets, so the page is styled with no
//! step in the app. A release build renders compiled views, which an app builds itself.
#![cfg(debug_assertions)]

use ignite_panel::Ignite;
use propel_rocket::PropelView;
use rocket::http::Status;
use rocket::local::blocking::Client;
use rocket::{get, routes};
use serde::Serialize;

#[derive(Serialize)]
struct AppContext {
    name: &'static str,
}

#[get("/app-page")]
fn app_page() -> PropelView {
    PropelView::render("appviews::hello", &AppContext { name: "Ada" })
}

fn client() -> Client {
    if std::env::var_os("IGNITE_SECRET_KEY").is_none() {
        std::env::set_var(
            "IGNITE_SECRET_KEY",
            "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=",
        );
    }
    propel_rocket::register_namespace(
        "appviews",
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/app_views"),
    );
    Client::tracked(
        rocket::build()
            .attach(Ignite::new().fairing())
            .mount("/", routes![app_page]),
    )
    .expect("valid rocket")
}

#[test]
fn an_app_view_extending_the_layout_is_styled_with_no_extra_step() {
    let client = client();
    let response = client.get("/app-page").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_string().unwrap();
    assert!(body.contains("<p>Hello Ada</p>"));
    assert!(
        body.contains("/admin/build/assets/app-"),
        "the panel's stylesheet is linked"
    );
    assert!(body.contains(".js\""), "and its script");
    assert!(
        !body.contains("Sign out"),
        "no panel navigation without the panel's context"
    );
}
