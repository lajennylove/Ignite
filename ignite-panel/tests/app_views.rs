//! Debug builds only: this test renders an app view through the dev renderer.
//! A release build has no dev renderer; an app compiles its own views with its
//! build script (see `propel_compiler::build::Views`).
#![cfg(debug_assertions)]

//! An app's own view extends the panel layout with only the app's context. The
//! panel layout must not need any variable the app does not provide.

use ignite_panel::Ignite;
use propel_rocket::{register_namespace, PropelView};
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

#[test]
fn an_app_view_extending_the_layout_renders_with_only_the_app_context() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/app_views");
    register_namespace("appviews", root);
    let client = Client::tracked(
        rocket::build()
            .attach(Ignite::new().fairing())
            .mount("/", routes![app_page]),
    )
    .expect("valid rocket");

    let response = client.get("/app-page").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_string().unwrap();
    assert!(body.contains("<p>Hello Ada</p>"));
    assert!(
        body.contains(r#"href="/admin""#),
        "the brand falls back to /admin"
    );
    assert!(
        !body.contains("Sign out"),
        "no panel navigation without the panel's context"
    );
}
