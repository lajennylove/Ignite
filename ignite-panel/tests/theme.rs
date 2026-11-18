//! The built-in `ignite` theme renders the same HTML as before the theme system. This test
//! binary registers no other theme, so the pages are exactly the `ignite` theme's (the
//! profile, for one, offers no admin theme choice). The other themes are tested in
//! `tests/admin_themes.rs`. Runs against the `ignite_test` database. Run it with `--release`
//! too: a release build renders the `ignite` theme from the compiled views.
//!
//! The snapshots in `tests/fixtures/theme` are the pages as Ignite rendered them before the
//! theme system. After a deliberate change to the `ignite` theme, write them again with
//! `IGNITE_BLESS=1 cargo test -p ignite-panel --test theme`, and review the diff.

mod common;

use common::snapshots::{assert_snapshot, pages};
use common::*;
use ignite_panel::theme::{
    active_admin_theme, register_admin_theme, registered_admin_themes, set_admin_theme, AdminTheme,
    ColorToken, ThemeError,
};

/// The active theme is shared by the process, so the tests that read it take the lock the
/// database tests take.
fn theme_lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn the_ignite_theme_renders_the_same_html_as_before() {
    let _guard = reset();
    assert_eq!(
        active_admin_theme(),
        "ignite",
        "the built-in theme is active"
    );
    assert_eq!(registered_admin_themes(), ["ignite"], "no other theme");
    for (name, html) in pages() {
        assert_snapshot(name, &html);
    }
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
