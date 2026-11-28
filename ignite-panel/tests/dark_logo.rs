//! A brand logo for dark mode, as Filament's `darkModeBrandLogo()`
//! (<https://filamentphp.com/docs/4.x/styling/overview>, "Adding a logo"): the dark logo shows
//! in dark mode, in the navigation and the sign-in bar, and the default logo in light mode. Both
//! are in the page, so the top bar's switch changes the logo without a reload; the panel's
//! stylesheet hides the one that does not match the page's `data-theme`. Runs against the test
//! database. Run it with `--release` too: a release build renders the compiled views.

mod common;

use common::*;
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;

const LIGHT: &str = r#"<span class="ig-logo-light flex items-center"><img src="/images/mc-logo.webp" alt="Mission Control" class="w-auto" style="height: 2rem"></span>"#;
const DARK: &str = r#"<img src="/images/mc-logo-dark.webp" alt="Mission Control" class="ig-logo-dark w-auto" style="height: 2rem">"#;

fn configure(ignite: ignite_panel::Ignite) -> ignite_panel::Ignite {
    ignite
        .brand_name("Mission Control")
        .brand_logo("/images/mc-logo.webp")
        .dark_mode_brand_logo("/images/mc-logo-dark.webp")
        .brand_logo_height("2rem")
}

fn page(client: &Client, path: &str) -> String {
    let response = client.get(path).dispatch();
    assert_eq!(response.status(), Status::Ok, "GET {path}");
    response.into_string().unwrap()
}

/// The panel's stylesheet the page links.
fn stylesheet(client: &Client, html: &str) -> String {
    let start = html
        .find("/admin/build/assets/app-")
        .expect("the panel's stylesheet");
    let end = start + html[start..].find('"').unwrap();
    page(client, &html[start..end])
}

fn set_theme(client: &Client, theme: &str) {
    let response = client
        .post("/admin/theme")
        .header(ContentType::Form)
        .body(with_token(client, &format!("theme={theme}")))
        .dispatch();
    assert_eq!(response.status(), Status::NoContent);
}

#[test]
fn the_dark_logo_shows_in_dark_mode_and_the_default_logo_in_light_mode() {
    let _guard = reset();
    let client = signed_in_with(configure);

    // Light mode: the page is light, the default logo is the light one.
    set_theme(&client, "light");
    let html = page(&client, "/admin");
    assert!(html.contains("data-theme=\"light\""), "{html}");
    assert!(html.contains(LIGHT), "{html}");
    assert!(html.contains(DARK), "{html}");

    // Dark mode: the page is dark, and the dark logo is there to show.
    set_theme(&client, "dark");
    let html = page(&client, "/admin/widgets");
    assert!(html.contains("data-theme=\"dark\""), "{html}");
    assert!(html.contains(DARK), "{html}");
    assert!(html.contains(LIGHT), "{html}");

    // The stylesheet shows each logo in its own mode only.
    let css = stylesheet(&client, &html);
    assert!(
        css.contains(r#":root:not([data-theme=dark]) .ig-logo-dark"#),
        "the dark logo is hidden in light mode"
    );
    assert!(
        css.contains(r#":root[data-theme=dark] .ig-logo-light{display:none}"#),
        "the light logo is hidden in dark mode"
    );
}

#[test]
fn the_sign_in_bar_has_both_logos() {
    let _guard = reset();
    let guest = client_with(configure);
    let html = page(&guest, "/admin/login");
    assert!(html.contains(DARK), "{html}");
    assert!(html.contains(LIGHT), "{html}");
}

#[test]
fn a_dark_logo_without_a_default_logo_keeps_the_name_in_light_mode() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.dark_mode_brand_logo("/dark.svg"));
    let html = page(&client, "/admin");
    assert!(
        html.contains(
            r#"<span class="ig-logo-light flex items-center"><span class="text-xl">Ignite</span></span><img src="/dark.svg" alt="Ignite" class="ig-logo-dark w-auto" style="height: 1.5rem">"#
        ),
        "{html}"
    );
    assert!(!html.contains("ignite-mark.png"));
}

#[test]
fn without_a_dark_logo_the_logo_shows_in_both_modes() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.brand_logo("/logo.svg"));
    set_theme(&client, "dark");
    let html = page(&client, "/admin");
    assert!(html.contains(
        r#"<img src="/logo.svg" alt="Ignite" class="w-auto" style="height: 1.5rem">"#
    ));
    assert!(!html.contains("ig-logo-light"));
    assert!(!html.contains("ig-logo-dark"));
}

#[test]
fn the_dark_logo_url_is_escaped() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.dark_mode_brand_logo("/x\"onerror=\"alert(1)"));
    let html = page(&client, "/admin");
    assert!(html.contains("src=\"/x&quot;onerror=&quot;alert(1)\""), "{html}");
    assert!(!html.contains("onerror=\"alert"));
}
