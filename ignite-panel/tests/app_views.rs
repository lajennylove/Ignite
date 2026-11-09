//! An app's own view extends the panel layout with only the app's own context. The
//! layout's stylesheet and script come from the panel's `ui` namespace, which the app
//! adds to its assets once, so the page is styled with no step in the view.

use ignite_panel::{panel_source, EmbeddedViews, Ignite};
use propel_rocket::PropelView;
use propel_runtime::{render_with_assets, Assets, FileLoader};
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
    // The app's loader: its own views, and the panel's embedded views as `ignite`.
    let loader = FileLoader::new(".")
        .namespace(
            "appviews",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/app_views"),
        )
        .namespace_loader("ignite", EmbeddedViews);
    // The app's assets: the panel's build as the `ui` namespace.
    let assets = Assets::new("public").namespace("ui", panel_source("/admin"));
    PropelView::from_result(render_with_assets(
        &loader,
        &assets,
        "appviews::hello",
        &AppContext { name: "Ada" },
    ))
}

fn client() -> Client {
    if std::env::var_os("IGNITE_SECRET_KEY").is_none() {
        std::env::set_var(
            "IGNITE_SECRET_KEY",
            "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=",
        );
    }
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
    assert!(
        body.contains("/admin/build/assets/app-") && body.contains(".js\""),
        "and its script"
    );
    assert!(
        body.contains(r#"href="/admin""#),
        "the brand falls back to /admin"
    );
    assert!(
        !body.contains("Sign out"),
        "no panel navigation without the panel's context"
    );
}
