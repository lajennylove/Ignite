//! The light and dark switch per admin theme, as Filament's panel `darkMode(false)` and
//! `themeSwitcher(false)`: a theme without a dark mode hides the top bar's switch and the
//! profile's line about it, and renders light; a theme can word the profile's line itself; the
//! `ignite` theme keeps both. Runs against the `ignite_test` database.

mod common;

use common::*;
use ignite_panel::theme::{register_admin_theme, AdminTheme, IGNITE_LAYOUT};
use ignite_panel::users;

/// A theme in the style of wp-admin: the panel's layout, and no dark mode.
struct NoDark;

impl AdminTheme for NoDark {
    fn name(&self) -> &str {
        "switch-nodark"
    }

    fn layout(&self) -> &str {
        IGNITE_LAYOUT
    }

    fn dark_mode(&self) -> bool {
        false
    }
}

/// A theme with a dark mode and its own wording for the profile's line.
struct Worded;

impl AdminTheme for Worded {
    fn name(&self) -> &str {
        "switch-worded"
    }

    fn layout(&self) -> &str {
        IGNITE_LAYOUT
    }

    fn theme_switcher_note(&self) -> Option<String> {
        Some("Use the <moon> button for dark mode.".to_string())
    }
}

/// A theme that keeps its dark mode but hides the switch, as `themeSwitcher(false)`.
struct Hidden;

impl AdminTheme for Hidden {
    fn name(&self) -> &str {
        "switch-hidden"
    }

    fn layout(&self) -> &str {
        IGNITE_LAYOUT
    }

    fn theme_switcher(&self) -> bool {
        false
    }
}

fn register() {
    let _ = register_admin_theme(NoDark);
    let _ = register_admin_theme(Worded);
    let _ = register_admin_theme(Hidden);
}

/// Sets the test user's admin theme and their light or dark mode.
fn choose(theme: Option<&str>, mode: &str) {
    let t = test_db();
    t.run(async {
        let user = users::find_by_email(&t.db, EMAIL).await.unwrap().unwrap();
        users::set_admin_theme(&t.db, user.id, theme).await.unwrap();
        users::set_theme(&t.db, user.id, mode).await.unwrap();
    });
}

fn body(client: &rocket::local::blocking::Client, path: &str) -> String {
    client.get(path).dispatch().into_string().unwrap()
}

const NOTE: &str = "The light or dark mode is the switch in the top bar, and stays as it is.";

#[test]
fn the_ignite_theme_keeps_its_switch_and_its_line() {
    let _guard = reset();
    register();
    choose(None, "dark");
    let client = signed_in(None);
    let dashboard = body(&client, "/admin");
    assert!(dashboard.contains("data-theme-toggle"));
    assert!(dashboard.contains("data-theme=\"dark\""));
    let profile = body(&client, "/admin/profile");
    assert!(profile.contains("Admin theme"), "more than one theme");
    assert!(profile.contains(NOTE));
}

#[test]
fn a_theme_without_dark_mode_hides_the_switch_and_renders_light() {
    let _guard = reset();
    register();
    choose(Some("switch-nodark"), "dark");
    let client = signed_in(None);
    let dashboard = body(&client, "/admin");
    assert!(!dashboard.contains("data-theme-toggle"), "no switch");
    assert!(dashboard.contains("data-theme=\"light\""), "always light");
    assert!(!dashboard.contains("data-theme=\"dark\""));
    let profile = body(&client, "/admin/profile");
    assert!(profile.contains("Admin theme"), "the theme choice stays");
    assert!(
        !profile.contains(NOTE),
        "no line about a switch it does not have"
    );
    assert!(!profile.contains("data-theme-toggle"));

    // The stored choice is kept for a theme with a dark mode.
    choose(None, "dark");
    let dashboard = body(&client, "/admin");
    assert!(dashboard.contains("data-theme=\"dark\""));
}

#[test]
fn a_theme_can_hide_the_switch_and_keep_dark_mode() {
    let _guard = reset();
    register();
    choose(Some("switch-hidden"), "dark");
    let client = signed_in(None);
    let dashboard = body(&client, "/admin");
    assert!(!dashboard.contains("data-theme-toggle"));
    assert!(dashboard.contains("data-theme=\"dark\""));
    assert!(!body(&client, "/admin/profile").contains(NOTE));
}

#[test]
fn the_line_is_the_themes_wording_escaped() {
    let _guard = reset();
    register();
    choose(Some("switch-worded"), "light");
    let client = signed_in(None);
    let profile = body(&client, "/admin/profile");
    assert!(profile.contains("Use the &lt;moon&gt; button for dark mode."));
    assert!(!profile.contains("<moon>"));
    assert!(!profile.contains(NOTE));
    assert!(body(&client, "/admin").contains("data-theme-toggle"));
}
