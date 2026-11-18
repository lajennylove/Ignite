//! The panel's look: the built-in `ignite` theme renders the same HTML as before the theme
//! system, and another theme changes the look of the same screens. Runs against the
//! `ignite_test` database.
//!
//! The snapshots in `tests/fixtures/theme` are the pages as Ignite rendered them before the
//! theme system. After a deliberate change to the `ignite` theme, write them again with
//! `IGNITE_BLESS=1 cargo test -p ignite-panel --test theme`, and review the diff.

mod common;

use std::path::PathBuf;

use common::*;
use ignite_panel::theme::{
    active_admin_theme, icon_classes, register_admin_theme, set_admin_theme, AdminTheme,
    ColorToken, IgniteTheme, ThemeError,
};
use rocket::http::Status;
use rocket::local::blocking::Client;

/// The file of the snapshot `name`.
fn snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/theme")
        .join(format!("{name}.html"))
}

/// The HTML of `path`, with the client's CSRF token replaced by `{csrf}`, because the token
/// is new for each client.
fn page(client: &Client, path: &str) -> String {
    let response = client.get(path).dispatch();
    assert_eq!(response.status(), Status::Ok, "GET {path}");
    let body = response.into_string().unwrap();
    match client.cookies().get("ignite_csrf") {
        Some(_) => body.replace(&csrf_token(client), "{csrf}"),
        None => body,
    }
}

/// Checks `html` against the snapshot `name`, or writes it when `IGNITE_BLESS` is set.
fn assert_snapshot(name: &str, html: &str) {
    let path = snapshot_path(name);
    if std::env::var_os("IGNITE_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, html).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read the snapshot {}: {error}", path.display()));
    assert!(
        html == expected,
        "the page `{name}` differs from its snapshot {}:\n{html}",
        path.display()
    );
}

/// The pages of the panel, signed in and signed out, as `(snapshot, HTML)`.
fn pages() -> Vec<(&'static str, String)> {
    let client = signed_in(None);
    let mut pages = vec![
        ("dashboard", page(&client, "/admin")),
        ("index", page(&client, "/admin/widgets")),
        ("create", page(&client, "/admin/widgets/new")),
        ("edit", page(&client, "/admin/widgets/1/edit")),
        ("profile", page(&client, "/admin/profile")),
    ];
    let guest = client_signed_out();
    pages.push(("login", page(&guest, "/admin/login")));
    pages.push(("forgot", page(&guest, "/admin/password/forgot")));
    pages
}

/// The active theme is shared by the process, so the tests that read it take the lock the
/// database tests take.
fn theme_lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn client_signed_out() -> Client {
    client(None)
}

#[test]
fn the_ignite_theme_renders_the_same_html_as_before() {
    let _guard = reset();
    assert_eq!(
        active_admin_theme(),
        "ignite",
        "the built-in theme is active"
    );
    for (name, html) in pages() {
        assert_snapshot(name, &html);
    }
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

#[test]
fn an_unknown_theme_is_refused_and_the_active_theme_stays() {
    let _guard = theme_lock();
    let error = set_admin_theme("no-such-theme").unwrap_err();
    assert!(matches!(&error, ThemeError::Unknown(name) if name == "no-such-theme"));
    assert_eq!(
        error.to_string(),
        "no admin theme is registered as `no-such-theme`"
    );
    assert_eq!(active_admin_theme(), "ignite");
}

#[test]
fn a_theme_with_an_unsafe_colour_token_is_refused() {
    let _guard = theme_lock();
    struct Broken;
    impl AdminTheme for Broken {
        fn name(&self) -> &str {
            "broken"
        }
        fn layout(&self) -> &str {
            "themes.broken.layout"
        }
        fn colors(&self) -> Vec<ColorToken> {
            vec![ColorToken::new("primary", "red;}</style><script>", "red")]
        }
    }
    assert!(matches!(
        register_admin_theme(Broken),
        Err(ThemeError::InvalidColor { .. })
    ));
    assert!(matches!(
        set_admin_theme("broken"),
        Err(ThemeError::Unknown(_))
    ));
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
