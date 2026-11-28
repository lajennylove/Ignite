//! The panel's 404 page, as Laravel's `errors/404.blade.php` inside the app's layout: a path
//! under the panel's mount that finds nothing answers 404 inside the panel's layout (the
//! navigation, the top bar, a message and a link to the dashboard), and a 404 elsewhere keeps
//! the app's own page.

mod common;

use common::{reset, signed_in_with};
use ignite_panel::{Ignite, NotFound};
use rocket::http::Status;
use rocket::local::blocking::Client;
use rocket::{catch, catchers, get, routes};

/// The navigation of a signed-in page: the menu toggle, the sidebar and its "Menu" caption.
const SIDEBAR: &str = r#"<input id="ig-nav" type="checkbox" class="peer sr-only">"#;
/// The top bar's sign-out form.
const TOP_BAR: &str = "Sign out";

fn assert_in_layout(body: &str) {
    assert!(body.contains(SIDEBAR), "the sidebar shows: {body}");
    assert!(
        body.contains(r#"aria-label="Main""#),
        "the navigation shows"
    );
    assert!(body.contains(TOP_BAR), "the top bar shows");
    assert!(body.contains("data-ignite-not-found"), "the 404 page shows");
}

#[test]
fn an_unknown_panel_path_is_404_inside_the_layout() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite);
    let response = client.get("/admin/does-not-exist/at/all").dispatch();
    assert_eq!(response.status(), Status::NotFound);
    let body = response.into_string().unwrap();
    assert_in_layout(&body);
    let title: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        title.contains("<title> Not Found · Ignite</title>"),
        "{body}"
    );
    assert!(body.contains("The page you are looking for could not be found."));
    assert!(
        body.contains(r#"<a class="ig-btn ig-btn-primary" href="/admin">Go to the dashboard</a>"#)
    );
    // The resources stay in the navigation, and none of them is marked current.
    assert!(body.contains(r#"href="/admin/widgets""#));
}

#[test]
fn an_unknown_resource_slug_is_404_inside_the_layout() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite);
    let response = client.get("/admin/no-such-resource").dispatch();
    assert_eq!(response.status(), Status::NotFound);
    assert_in_layout(&response.into_string().unwrap());
}

#[test]
fn an_edit_page_of_an_unknown_record_is_404_inside_the_layout() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite);
    let response = client.get("/admin/widgets/999999/edit").dispatch();
    assert_eq!(response.status(), Status::NotFound);
    let body = response.into_string().unwrap();
    assert_in_layout(&body);
    assert!(body.contains("Go to the dashboard"));
}

#[test]
fn the_app_sets_the_message_and_the_link_and_both_are_escaped() {
    let _guard = reset();
    let client = signed_in_with(|ignite| {
        ignite.not_found(
            NotFound::make()
                .title("Nothing <here>")
                .message("Sorry, <script>alert(1)</script> & no page.")
                .link_label("Back to the \"Dashboard\""),
        )
    });
    let response = client.get("/admin/missing").dispatch();
    assert_eq!(response.status(), Status::NotFound);
    let body = response.into_string().unwrap();
    assert_in_layout(&body);
    assert!(
        !body.contains("<script>alert(1)</script>"),
        "the message is escaped"
    );
    assert!(body.contains("Sorry, &lt;script&gt;alert(1)&lt;/script&gt; &amp; no page."));
    assert!(body.contains("Nothing &lt;here&gt;"));
    assert!(body.contains("Back to the &quot;Dashboard&quot;"));
    assert!(
        !body.contains("could not be found"),
        "the default message is replaced"
    );
}

#[test]
fn a_signed_out_visitor_gets_the_404_without_the_navigation() {
    let _guard = reset();
    common::ensure_secret();
    let client = Client::tracked(rocket::build().attach(Ignite::new().fairing())).unwrap();
    // A path no route of the panel matches. One segment, such as `/admin/x`, is a resource's
    // list, which needs a sign-in, so it goes to the sign-in page as before.
    let response = client.get("/admin/does/not/exist/here").dispatch();
    assert_eq!(response.status(), Status::NotFound);
    let body = response.into_string().unwrap();
    assert!(body.contains("data-ignite-not-found"));
    assert!(!body.contains(SIDEBAR), "no navigation for a visitor");
    assert!(!body.contains(TOP_BAR));
    // A page of the panel that needs a sign-in still goes to the sign-in page.
    let response = client.get("/admin/profile").dispatch();
    assert_eq!(response.status(), Status::SeeOther);
}

#[get("/")]
fn home() -> &'static str {
    "home"
}

#[catch(404)]
fn public_not_found() -> &'static str {
    "the site's own 404"
}

/// The app with a public route, its own 404 catcher and the panel.
fn app(with_catcher: bool) -> Client {
    common::ensure_secret();
    let mut rocket = rocket::build()
        .mount("/", routes![home])
        .attach(Ignite::new().fairing());
    if with_catcher {
        rocket = rocket.register("/", catchers![public_not_found]);
    }
    Client::tracked(rocket).unwrap()
}

#[test]
fn a_public_404_keeps_the_apps_own_page() {
    let _guard = reset();
    let client = app(true);
    let response = client.get("/no-such-page").dispatch();
    assert_eq!(response.status(), Status::NotFound);
    assert_eq!(response.into_string().unwrap(), "the site's own 404");
    // A path that only starts like the mount is not the panel's.
    let response = client.get("/administrator").dispatch();
    assert_eq!(response.into_string().unwrap(), "the site's own 404");
    // The panel's path gets the panel's page.
    let response = client.get("/admin/no/such/page/here").dispatch();
    assert_eq!(response.status(), Status::NotFound);
    assert!(response
        .into_string()
        .unwrap()
        .contains("data-ignite-not-found"));
}

#[test]
fn a_public_404_without_an_app_catcher_is_rockets_default() {
    let _guard = reset();
    let client = app(false);
    let response = client.get("/no-such-page").dispatch();
    assert_eq!(response.status(), Status::NotFound);
    let body = response.into_string().unwrap();
    assert!(!body.contains("data-ignite-not-found"), "{body}");
    assert!(body.contains("404"), "{body}");
}

#[test]
fn the_404_follows_a_custom_mount() {
    let _guard = reset();
    common::ensure_secret();
    let client = common::signed_in(Some("/mc-admin"));
    let response = client.get("/mc-admin/does-not-exist").dispatch();
    assert_eq!(response.status(), Status::NotFound);
    let body = response.into_string().unwrap();
    assert_in_layout(&body);
    assert!(body.contains(r#"href="/mc-admin">Go to the dashboard</a>"#));
    // The default mount is not the panel's here.
    let response = client.get("/admin/does-not-exist").dispatch();
    assert_eq!(response.status(), Status::NotFound);
    assert!(!response
        .into_string()
        .unwrap()
        .contains("data-ignite-not-found"));
}
