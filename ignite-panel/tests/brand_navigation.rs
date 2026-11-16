//! The panel's brand and navigation options, as Filament's panel and resources set them: the
//! brand name, logo and favicon, the navigation groups, the sort and the icons. Runs against
//! the `ignite_test` database. Run it with `--release` too: a release build renders the views
//! compiled into the panel, so the options must be in them.

mod common;

use common::*;
use rocket::http::Status;
use rocket::local::blocking::Client;

/// The HTML of `path`.
fn page(client: &Client, path: &str) -> String {
    let response = client.get(path).dispatch();
    assert_eq!(response.status(), Status::Ok, "GET {path}");
    response.into_string().unwrap()
}

/// The text of the `<title>` element, without the line breaks of the title section.
fn title(html: &str) -> String {
    let start = html.find("<title>").expect("a title") + "<title>".len();
    let end = html.find("</title>").expect("the end of the title");
    html[start..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

const MARK: &str =
    r#"<img src="/admin/build/resources/images/ignite-mark.png" alt="" class="h-9 w-auto">"#;

#[test]
fn the_default_brand_is_ignite_with_its_mark_and_favicons() {
    let _guard = reset();
    let client = signed_in(None);
    let html = page(&client, "/admin/widgets");
    assert_eq!(title(&html), "Widgets · Ignite");
    assert!(html.contains(MARK), "{html}");
    assert!(html.contains(r#"<span class="text-xl">Ignite</span>"#));
    assert!(html.contains(
        r#"<link rel="icon" href="/admin/build/resources/images/favicon.ico" sizes="any">"#
    ));
    assert!(html.contains(r#"<link rel="apple-touch-icon""#));

    let guest = client_with(|ignite| ignite);
    let html = page(&guest, "/admin/login");
    assert_eq!(title(&html), "Sign in · Ignite");
    assert!(html.contains(MARK));
}

#[test]
fn a_brand_name_replaces_the_name_and_the_mark() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.brand_name("Mission Control"));
    let html = page(&client, "/admin");
    assert_eq!(title(&html), "Dashboard · Mission Control");
    assert!(html.contains(r#"<span class="text-xl">Mission Control</span>"#));
    assert!(!html.contains("ignite-mark.png"), "{html}");

    let guest = client_with(|ignite| ignite.brand_name("Mission Control"));
    let html = page(&guest, "/admin/login");
    assert_eq!(title(&html), "Sign in · Mission Control");
    assert!(
        html.contains("Mission Control\n"),
        "the sign-in bar shows the name"
    );
    assert!(!html.contains("ignite-mark.png"));
}

#[test]
fn a_brand_logo_and_favicon_appear_in_the_page() {
    let _guard = reset();
    let configure = |ignite: ignite_panel::Ignite| {
        ignite
            .brand_name("Mission Control")
            .brand_logo("/mc-includes/images/mission-control-logo.webp")
            .brand_logo_height("2.5rem")
            .favicon("/mc-includes/images/favicon.png")
    };
    let logo = r#"<img src="/mc-includes/images/mission-control-logo.webp" alt="Mission Control" class="w-auto" style="height: 2.5rem">"#;
    let favicon = r#"<link rel="icon" href="/mc-includes/images/favicon.png">"#;

    let client = signed_in_with(configure);
    let html = page(&client, "/admin/widgets");
    assert!(html.contains(logo), "{html}");
    assert!(html.contains(favicon));
    assert!(
        !html.contains(r#"<span class="text-xl">"#),
        "the logo replaces the name"
    );
    assert!(!html.contains("ignite-mark.png"));
    assert!(
        !html.contains("favicon.ico"),
        "the app's favicon replaces Ignite's"
    );
    assert!(!html.contains("apple-touch-icon"));

    let guest = client_with(configure);
    let html = page(&guest, "/admin/login");
    assert!(
        html.contains(logo),
        "the sign-in bar shows the logo: {html}"
    );
    assert!(html.contains(favicon));
    assert_eq!(title(&html), "Sign in · Mission Control");
}

#[test]
fn a_logo_without_a_height_takes_filaments_default() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.brand_logo("/logo.svg"));
    let html = page(&client, "/admin");
    assert!(
        html.contains(
            r#"<img src="/logo.svg" alt="Ignite" class="w-auto" style="height: 1.5rem">"#
        ),
        "{html}"
    );
}

#[test]
fn brand_text_is_escaped() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.brand_name("<b>Acme</b> & Co"));
    let html = page(&client, "/admin");
    assert!(html.contains("&lt;b&gt;Acme&lt;/b&gt; &amp; Co"), "{html}");
    assert!(!html.contains("<b>Acme</b>"));
}

#[test]
#[should_panic(expected = "is not a CSS length")]
fn a_logo_height_that_is_not_a_length_is_refused() {
    ensure_secret();
    std::env::set_var("DATABASE_URL", database_url());
    let _ = ignite_panel::Ignite::new().brand_logo_height("2rem;background:red");
}
