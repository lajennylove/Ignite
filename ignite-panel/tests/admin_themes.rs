//! Admin themes other than `ignite`: a theme with its own layout, a theme of colour tokens, a
//! theme's icons and stylesheets, both themes in one build, and each user's own admin theme.
//! Runs against the `ignite_test` database. Run it with `--release` too: a release build
//! renders a theme other than `ignite` through Propel's runtime renderer.
//!
//! The themes registered here stay registered for the rest of the binary, so the snapshot test
//! of the `ignite` theme is in `tests/theme.rs`, where no other theme is registered.

mod common;

use common::snapshots::{assert_snapshot, page, pages};
use common::*;
use ignite_panel::theme::{
    active_admin_theme, icon_classes, register_admin_theme, set_admin_theme, AdminTheme,
    ColorToken, IgniteTheme, ThemeError,
};
use ignite_panel::{resolve_admin_theme, users};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;

fn client_signed_out() -> Client {
    client(None)
}

/// The profile page without its admin theme card, which shows only when more than one theme
/// is registered, so it can be compared with the `ignite` theme's snapshot.
fn without_admin_theme_card(html: &str) -> String {
    let card = "\n\n    <section class=\"ig-card\">\n      <h2 class=\"mb-5\">Admin theme</h2>";
    let start = html.find(card).expect("the admin theme card");
    let end = html[start + card.len()..]
        .find("</section>")
        .expect("the end of the card")
        + start
        + card.len()
        + "</section>".len();
    format!("{}{}", &html[..start], &html[end..])
}

/// A second look for the same screens: its own layout, with a marker, the theme's colour
/// tokens and the panel's sections, and its own icon for the dashboard link.
struct Plain;

const PLAIN_LAYOUT: &str = "<!doctype html>
<html data-admin-theme=\"plain\">
<head><title>@yield('title')</title><style>{!! $theme_style !!}</style></head>
<body>
<nav>@foreach (group in nav)<a class=\"{{ $group.icon }}\" href=\"{{ $group.url }}\">{{ $group.title }}</a>@endforeach</nav>
@yield('content')
</body>
</html>
";

impl AdminTheme for Plain {
    fn name(&self) -> &str {
        "plain"
    }

    fn layout(&self) -> &str {
        "themes.plain.layout"
    }

    fn colors(&self) -> Vec<ColorToken> {
        vec![ColorToken::new("primary", "#2271b1", "#72aee6")]
    }

    fn icon(&self, name: &str) -> Option<String> {
        match name {
            "dashboard" => Some("dashicons dashicons-dashboard"),
            "post" => Some("dashicons dashicons-admin-post"),
            "media" => Some("dashicons dashicons-admin-media"),
            _ => None,
        }
        .map(str::to_string)
    }

    fn resource_icon(&self) -> String {
        "dashicons dashicons-admin-generic".to_string()
    }

    fn view(&self, name: &str) -> Option<String> {
        (name == "themes.plain.layout").then(|| PLAIN_LAYOUT.to_string())
    }
}

/// Puts the `ignite` theme back when a test ends, even when it fails, so the snapshot test
/// sees the default look.
struct RestoreTheme;

impl Drop for RestoreTheme {
    fn drop(&mut self) {
        set_admin_theme("ignite").expect("the built-in theme is always registered");
    }
}

#[test]
fn switching_the_theme_changes_the_look_of_the_same_list_page() {
    let _guard = reset();
    let client = signed_in(None);
    let before = page(&client, "/admin/widgets");
    assert!(!before.contains("data-admin-theme=\"plain\""));

    // A second registration of the name is refused. The test binary may have registered it.
    let _ = register_admin_theme(Plain);
    assert!(matches!(
        register_admin_theme(Plain),
        Err(ThemeError::AlreadyRegistered(name)) if name == "plain"
    ));
    let _restore = RestoreTheme;
    set_admin_theme("plain").expect("switch to the plain theme");
    assert_eq!(active_admin_theme(), "plain");

    let after = page(&client, "/admin/widgets");
    assert!(
        after.contains("<html data-admin-theme=\"plain\">"),
        "the plain layout: {after}"
    );
    assert!(!after.contains("ig-with-sidebar"), "not the ignite layout");
    // The same screen: the list's rows, search and pager are the resource's, as before.
    assert!(after.contains("Apple") && after.contains("Banana"));
    assert!(after.contains("Page 1 of 2"));
    // The theme's colour tokens and icons reach its layout.
    assert!(
        after.contains(":root{--color-primary:#2271b1;}")
            && after.contains(":root[data-theme=\"dark\"]{--color-primary:#72aee6;}"),
        "colour tokens: {after}"
    );
    assert!(after.contains("class=\"dashicons dashicons-dashboard\" href=\"/admin\""));
    // Widgets names no icon: the theme's generic resource icon.
    assert!(after.contains("class=\"dashicons dashicons-admin-generic\" href=\"/admin/widgets\""));

    // Back to the default look: the same HTML as before.
    set_admin_theme("ignite").unwrap();
    assert_eq!(page(&client, "/admin/widgets"), before);
}

#[test]
fn a_theme_of_colour_tokens_keeps_the_ignite_layout() {
    /// The `ignite` look in other colours: a theme that gives only colour tokens.
    struct Tinted;

    impl AdminTheme for Tinted {
        fn name(&self) -> &str {
            "tinted"
        }

        fn layout(&self) -> &str {
            ignite_panel::theme::IGNITE_LAYOUT
        }

        fn colors(&self) -> Vec<ColorToken> {
            vec![ColorToken::new("primary", "#d63638", "#f86368")]
        }
    }

    let _guard = reset();
    let client = signed_in(None);
    let _ = register_admin_theme(Tinted);
    let _restore = RestoreTheme;
    set_admin_theme("tinted").unwrap();

    let body = page(&client, "/admin/widgets");
    assert!(body.contains(
        "<style>:root{--color-primary:#d63638;}:root[data-theme=\"dark\"]{--color-primary:#f86368;}</style></head>"
    ));
    assert!(body.contains("ig-with-sidebar"), "the ignite layout");
    assert!(body.contains("ti-layout-dashboard"), "the ignite icons");
}

/// Both themes render in this build: the `ignite` theme exactly as before (its snapshots),
/// and a registered theme with its own layout. Run with `--release` to cover a release build,
/// which renders the `ignite` theme from the compiled views and the other theme through
/// Propel's runtime renderer, from the views embedded in the binary.
#[test]
fn both_themes_render_in_this_build() {
    let _guard = reset();
    let _ = register_admin_theme(Plain);
    let _restore = RestoreTheme;

    assert_eq!(active_admin_theme(), "ignite");
    for (name, html) in pages() {
        // Two themes are registered, so the profile offers the choice; the rest of the page is
        // the snapshot's.
        let html = if name == "profile" {
            assert!(
                html.contains("<h2 class=\"mb-5\">Admin theme</h2>"),
                "{html}"
            );
            without_admin_theme_card(&html)
        } else {
            html
        };
        assert_snapshot(name, &html);
    }

    set_admin_theme("plain").expect("any registered theme can be chosen, in every build");
    assert_eq!(active_admin_theme(), "plain");
    let client = signed_in(None);
    for path in [
        "/admin",
        "/admin/widgets",
        "/admin/widgets/new",
        "/admin/widgets/1/edit",
        "/admin/profile",
    ] {
        let html = page(&client, path);
        assert!(
            html.starts_with("<!doctype html>\n<html data-admin-theme=\"plain\">"),
            "{path} in the plain layout: {html}"
        );
        assert!(
            !html.contains("ig-with-sidebar"),
            "{path}: not the ignite layout"
        );
    }
    let guest = client_signed_out();
    let login = page(&guest, "/admin/login");
    assert!(login.contains("data-admin-theme=\"plain\""), "{login}");
    assert!(login.contains("Sign in"));

    set_admin_theme("ignite").unwrap();
    assert_snapshot("index", &page(&client, "/admin/widgets"));
}

/// Each theme maps an icon name to its own classes, and a name it does not know, or no name,
/// to its generic resource icon.
#[test]
fn each_theme_maps_icon_names_to_its_own_icons() {
    assert_eq!(
        icon_classes(&IgniteTheme, Some("dashboard")),
        "ti ti-layout-dashboard"
    );
    assert_eq!(icon_classes(&IgniteTheme, Some("post")), "ti ti-pin");
    assert_eq!(
        icon_classes(&IgniteTheme, Some("settings")),
        "ti ti-settings"
    );
    assert_eq!(icon_classes(&IgniteTheme, Some("book")), "ti ti-table");
    assert_eq!(icon_classes(&IgniteTheme, None), "ti ti-table");

    assert_eq!(
        icon_classes(&Plain, Some("post")),
        "dashicons dashicons-admin-post"
    );
    assert_eq!(
        icon_classes(&Plain, Some("media")),
        "dashicons dashicons-admin-media"
    );
    assert_eq!(
        icon_classes(&Plain, Some("book")),
        "dashicons dashicons-admin-generic",
        "a custom post type's name the theme does not know"
    );
    assert_eq!(
        icon_classes(&Plain, None),
        "dashicons dashicons-admin-generic"
    );
}

/// A theme with its own stylesheet: the Dashicons font of a WordPress-like theme.
struct Styled;

impl AdminTheme for Styled {
    fn name(&self) -> &str {
        "styled"
    }

    fn layout(&self) -> &str {
        ignite_panel::theme::IGNITE_LAYOUT
    }

    fn stylesheets(&self) -> Vec<String> {
        vec![
            "/mc-includes/css/dashicons.min.css".to_string(),
            "/mc-admin/css/wp.css?v=1&x=2".to_string(),
        ]
    }
}

#[test]
fn a_themes_stylesheets_load_only_while_it_is_active() {
    let _guard = reset();
    let _ = register_admin_theme(Styled);
    let _restore = RestoreTheme;
    let client = signed_in_with(|ignite| ignite.stylesheet("/app.css"));
    let guest = client_with(|ignite| ignite.stylesheet("/app.css"));
    let dashicons = r#"<link rel="stylesheet" href="/mc-includes/css/dashicons.min.css">"#;
    let app = r#"<link rel="stylesheet" href="/app.css">"#;

    // The ignite theme has none: only the app's stylesheet.
    for html in [page(&client, "/admin"), page(&guest, "/admin/login")] {
        assert!(!html.contains("dashicons"), "{html}");
        assert!(html.contains(app));
    }

    set_admin_theme("styled").unwrap();
    for html in [
        page(&client, "/admin"),
        page(&client, "/admin/widgets"),
        page(&client, "/admin/profile"),
        page(&guest, "/admin/login"),
    ] {
        // The theme's stylesheets, in order and escaped, then the app's, at the end of the head.
        let expected = format!(
            "{dashicons}\n<link rel=\"stylesheet\" href=\"/mc-admin/css/wp.css?v=1&amp;x=2\">\n{app}\n</head>"
        );
        assert!(html.contains(&expected), "{html}");
    }

    set_admin_theme("ignite").unwrap();
    assert!(!page(&client, "/admin").contains("dashicons"));
}

// ---------------------------------------------------------------------------------------------
// Each user's admin theme
// ---------------------------------------------------------------------------------------------

/// The email of the second user of these tests.
const BEA: &str = "bea@example.test";

/// Creates a second user, Bea, beside the test user, and returns a client signed in as her.
fn bea() -> Client {
    let t = test_db();
    t.run(async {
        users::create_user(&t.db, "Bea", BEA, PASSWORD)
            .await
            .expect("create Bea")
    });
    signed_in_as(None, BEA, PASSWORD)
}

/// Chooses the admin theme `name` on the profile page of `client` ("" for the site default),
/// and returns the status.
fn choose(client: &Client, name: &str) -> Status {
    client.get("/admin/profile").dispatch();
    client
        .post("/admin/profile/admin-theme")
        .header(ContentType::Form)
        .body(with_token(client, &format!("admin_theme={name}")))
        .dispatch()
        .status()
}

/// The stored `(theme, admin_theme)` of the user with `email`.
fn stored(email: &str) -> (String, Option<String>) {
    let t = test_db();
    t.run(async {
        let user = users::find_by_email(&t.db, email)
            .await
            .expect("read the user")
            .expect("the user exists");
        (user.theme, user.admin_theme)
    })
}

fn in_plain(html: &str) -> bool {
    html.contains("<html data-admin-theme=\"plain\">")
}

fn in_ignite(html: &str) -> bool {
    html.contains("ig-with-sidebar") && !in_plain(html)
}

#[test]
fn a_users_admin_theme_applies_to_their_requests_only() {
    let _guard = reset();
    let _ = register_admin_theme(Plain);
    let _restore = RestoreTheme;
    let ann = signed_in(None);
    let bea = bea();

    // Two themes are registered: the profile offers the choice, on the site default.
    let profile = page(&ann, "/admin/profile");
    assert!(
        profile.contains(r#"<option value="" selected>Site default (Ignite)</option>"#),
        "{profile}"
    );
    assert!(
        profile.contains(r#"<option value="plain" >plain</option>"#),
        "{profile}"
    );

    let response = {
        ann.get("/admin/profile").dispatch();
        ann.post("/admin/profile/admin-theme")
            .header(ContentType::Form)
            .body(with_token(&ann, "admin_theme=plain"))
            .dispatch()
    };
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(
        response.headers().get_one("Location"),
        Some("/admin/profile")
    );
    assert_eq!(stored(EMAIL).1.as_deref(), Some("plain"));

    // Ann's requests render in her theme.
    for path in [
        "/admin",
        "/admin/widgets",
        "/admin/widgets/new",
        "/admin/profile",
    ] {
        assert!(in_plain(&page(&ann, path)), "Ann's {path}");
    }
    // Bea's, and the signed-out pages, stay on the site default.
    assert_eq!(active_admin_theme(), "ignite");
    // Bea holds no role, so she sees the dashboard and her profile.
    for path in ["/admin", "/admin/profile"] {
        assert!(in_ignite(&page(&bea, path)), "Bea's {path}");
    }
    assert_eq!(stored(BEA).1, None);
    assert!(!in_plain(&page(&client_signed_out(), "/admin/login")));

    // Her profile shows her choice selected; switching back to the site default clears it.
    let profile = page(&ann, "/admin/profile");
    assert!(
        profile.contains(r#"<option value="plain" selected>plain</option>"#),
        "{profile}"
    );
    assert_eq!(choose(&ann, ""), Status::SeeOther);
    assert_eq!(stored(EMAIL).1, None);
    assert!(in_ignite(&page(&ann, "/admin")));
}

#[test]
fn a_user_without_a_choice_gets_the_site_default() {
    let _guard = reset();
    let _ = register_admin_theme(Plain);
    let _restore = RestoreTheme;
    let ann = signed_in(None);
    let bea = bea();
    // Ann chooses the ignite theme; Bea chooses nothing.
    assert_eq!(choose(&ann, "ignite"), Status::SeeOther);

    // The site default changes: Bea follows it, and Ann keeps her choice.
    set_admin_theme("plain").unwrap();
    assert!(
        in_plain(&page(&bea, "/admin")),
        "Bea follows the site default"
    );
    assert!(in_plain(&page(&client_signed_out(), "/admin/login")));
    assert!(in_ignite(&page(&ann, "/admin")), "Ann's choice wins");
    assert_eq!(resolve_admin_theme(None), "plain");
    assert_eq!(resolve_admin_theme(Some("ignite")), "ignite");
}

#[test]
fn an_unknown_stored_name_falls_back_to_the_site_default() {
    let _guard = reset();
    let _ = register_admin_theme(Plain);
    let _restore = RestoreTheme;
    let ann = signed_in(None);
    // A theme the app registered once and no longer does.
    test_db().exec_with(
        "UPDATE users SET admin_theme = 'retired' WHERE email = ?",
        &[EMAIL.into()],
    );

    assert!(in_ignite(&page(&ann, "/admin")), "the site default, ignite");
    let profile = page(&ann, "/admin/profile");
    assert!(
        profile.contains(r#"<option value="" selected>Site default (Ignite)</option>"#),
        "{profile}"
    );
    set_admin_theme("plain").unwrap();
    assert!(in_plain(&page(&ann, "/admin")), "the site default, plain");
    assert_eq!(resolve_admin_theme(Some("retired")), "plain");

    // The profile refuses to store a name no theme has, and keeps the stored one.
    assert_eq!(choose(&ann, "no-such-theme"), Status::BadRequest);
    assert_eq!(stored(EMAIL).1.as_deref(), Some("retired"));
}

#[test]
fn the_light_or_dark_mode_stays_separate_from_the_admin_theme() {
    let _guard = reset();
    let _ = register_admin_theme(Plain);
    let _restore = RestoreTheme;
    let ann = signed_in(None);
    let toggle = |mode: &str| {
        ann.get("/admin").dispatch();
        ann.post("/admin/theme")
            .header(ContentType::Form)
            .body(with_token(&ann, &format!("theme={mode}")))
            .dispatch()
            .status()
    };

    assert_eq!(toggle("dark"), Status::NoContent);
    assert_eq!(choose(&ann, "plain"), Status::SeeOther);
    assert_eq!(
        stored(EMAIL),
        ("dark".to_string(), Some("plain".to_string())),
        "choosing the admin theme keeps the dark mode"
    );

    assert_eq!(toggle("light"), Status::NoContent);
    assert_eq!(
        stored(EMAIL),
        ("light".to_string(), Some("plain".to_string())),
        "the light or dark toggle keeps the admin theme"
    );

    // Back on the ignite theme, the page is in the mode the user chose.
    assert_eq!(toggle("dark"), Status::NoContent);
    assert_eq!(choose(&ann, "ignite"), Status::SeeOther);
    let html = page(&ann, "/admin");
    assert!(
        in_ignite(&html) && html.contains(r#"data-theme="dark""#),
        "{html}"
    );
}
