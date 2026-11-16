//! Admin themes: the look of the panel.
//!
//! A theme decides how the panel looks: the layout around every page, the navigation menu,
//! the top bar, the flash message, the colour tokens and the icons. It does not decide what
//! the panel shows: every theme renders the same screens (the dashboard, the lists, the forms,
//! the profile, the sign-in pages) from the same data, with the same links and forms, so a
//! switch of theme changes the look and nothing else.
//!
//! The panel ships one theme, `ignite`, which is registered and active from the start. Its
//! views are in `views/ignite/themes/ignite`. An app adds a theme with
//! [`register_admin_theme`] and chooses it with [`set_admin_theme`], as Laravel's managers
//! register a named driver with `extend` and choose it by name:
//!
//! ```ignore
//! ignite_panel::register_admin_theme(MyTheme)?;
//! ignite_panel::set_admin_theme("my-theme")?;
//! ```
//!
//! The choice is process-wide and read on every request, so it can change while the app runs,
//! such as from a settings page.
//!
//! # How a page uses the theme
//!
//! Every page extends `ignite::layouts.app` and includes `ignite::partials.nav`,
//! `ignite::partials.topbar` and `ignite::partials.flash`. The panel resolves those four names
//! through the active theme: to its [`layout`](AdminTheme::layout),
//! [`navigation`](AdminTheme::navigation), [`top_bar`](AdminTheme::top_bar) and
//! [`flash`](AdminTheme::flash) views. A page fills these sections of the layout, and the
//! layout places them where its look needs them:
//!
//! - `title`: the page's title, for the `<title>` element.
//! - `ignite_brand_name`: the brand name, which ends the `<title>` element. Without it the
//!   layout prints `Ignite`.
//! - `ignite_favicon`: the favicon links (the panel's `partials.favicon`). Without it the
//!   layout links Ignite's icons, so a view that has none of the panel's values, such as an
//!   app's own page, can extend the layout.
//! - `content`: the screen itself.
//! - `ignite_html`: attributes of the `<html>` element (the light or dark mode).
//! - `ignite_head`: the theme's colour tokens, as a `<style>` element, when it has any.
//! - `ignite_stylesheets`: the app's stylesheet links (the panel's `partials.stylesheets`,
//!   set with [`Ignite::stylesheet`](crate::Ignite::stylesheet)), at the end of the head,
//!   after the theme's CSS. Empty by default.
//! - `ignite_aside`: the navigation, on signed-in pages.
//! - `ignite_column`: classes of the column beside the navigation.
//! - `ignite_bar`: the top bar.
//! - `ignite_flash`: the flash message.
//!
//! Besides each page's own values, every page has the values of the look: `brand` (the
//! brand's `name`, `has_logo`, `logo`, `logo_height`, `has_mark`, `has_favicon`, `favicon` and
//! `stylesheets`, set with [`Ignite::brand_name`](crate::Ignite::brand_name) and the methods beside it),
//! `nav_sections` (the sidebar in sections, each with `has_heading`, `heading` and `items`, as
//! [`Ignite::navigation_groups`](crate::Ignite::navigation_groups) orders them) and `nav` (the
//! same entries in one list). Each entry has `title`, `url`, `active`, `has_add`, `add_url`,
//! `icon` (the theme's icon) and `has_icon_class` and `icon_class` (the resource's own
//! icon, its `NAVIGATION_ICON`).
//!
//! A theme's own views come from [`AdminTheme::view`], so a theme can ship them in its binary,
//! and the same method can replace any view of the panel by name. A replaced page must keep
//! the page's forms and links, so the flows stay the same.
//!
//! # Builds
//!
//! A debug build renders the panel's views at run time, through the active theme. A release
//! build renders the views compiled into the panel, which are the `ignite` theme's, so it
//! refuses to switch to another theme ([`ThemeError::NotCompiled`]) rather than show the
//! wrong look.

use std::fmt;
use std::io;
use std::sync::{Arc, OnceLock, PoisonError, RwLock};

use propel_runtime::Loader;

/// The name of the panel's own theme, which is registered and active from the start.
pub const IGNITE_THEME: &str = "ignite";

/// The `ignite` theme's layout view.
pub const IGNITE_LAYOUT: &str = "themes.ignite.layouts.app";

/// The `ignite` theme's navigation view: the sidebar menu.
pub const IGNITE_NAVIGATION: &str = "themes.ignite.partials.nav";

/// The `ignite` theme's top bar view.
pub const IGNITE_TOP_BAR: &str = "themes.ignite.partials.topbar";

/// The `ignite` theme's flash message view.
pub const IGNITE_FLASH: &str = "themes.ignite.partials.flash";

/// A look of the panel. Implement it and register the theme with [`register_admin_theme`].
///
/// Only [`name`](Self::name) and [`layout`](Self::layout) are required. The other parts
/// default to the `ignite` theme's, so a theme replaces only what it changes, as a WordPress
/// child theme does. A theme in the style of wp-admin, for example, gives its own layout (the
/// admin bar above a dark menu and the content), its own navigation and top bar, WordPress's
/// colours as tokens, and Dashicons for the icons.
///
/// View names are names in the panel's `ignite` namespace, such as `themes.ignite.layouts.app`.
/// A theme's views get the same values as the panel's own: see the module documentation for
/// the sections a page fills.
pub trait AdminTheme: Send + Sync + 'static {
    /// The theme's name, which [`set_admin_theme`] chooses it by. Lowercase letters, digits,
    /// `-` and `_`, such as `wp-like`.
    fn name(&self) -> &str;

    /// The view of the layout every page extends: the HTML document around the page.
    fn layout(&self) -> &str;

    /// The view of the navigation menu, which pages put in the `ignite_aside` section.
    fn navigation(&self) -> &str {
        IGNITE_NAVIGATION
    }

    /// The view of the top bar, which pages put in the `ignite_bar` section.
    fn top_bar(&self) -> &str {
        IGNITE_TOP_BAR
    }

    /// The view of the flash message, which pages put in the `ignite_flash` section.
    fn flash(&self) -> &str {
        IGNITE_FLASH
    }

    /// The colour tokens the theme sets, as CSS custom properties named `--color-<name>`, over
    /// the panel's stylesheet. The `ignite` theme sets none: its colours are the stylesheet's.
    fn colors(&self) -> Vec<ColorToken> {
        Vec::new()
    }

    /// The CSS classes of `icon`. The default is the `ignite` theme's Tabler icon.
    fn icon(&self, icon: Icon) -> String {
        match icon {
            Icon::Dashboard => "ti-layout-dashboard",
            Icon::Resource => "ti-table",
        }
        .to_string()
    }

    /// The source of the theme's view `name`, or `None` to read the panel's view of that
    /// name. A theme returns its own layout and partials here; returning a source for one of
    /// the panel's views, such as `index`, replaces it.
    fn view(&self, name: &str) -> Option<String> {
        let _ = name;
        None
    }
}

/// An icon the panel shows outside a theme's own views, so the theme chooses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Icon {
    /// The dashboard's entry in the navigation.
    Dashboard,
    /// A resource's entry in the navigation.
    Resource,
}

/// A colour of a theme: the CSS custom property `--color-<name>`, with its value in the light
/// mode and in the dark mode. The panel's stylesheet uses names such as `primary`, `surface`,
/// `page`, `heading`, `text` and `border`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorToken {
    /// The token's name without `--color-`, such as `primary`.
    pub name: String,
    /// The value in the light mode, such as `#2271b1` or `oklch(65% 0.18 266)`.
    pub light: String,
    /// The value in the dark mode.
    pub dark: String,
}

impl ColorToken {
    /// A token named `name` with its light and dark values.
    pub fn new(name: impl Into<String>, light: impl Into<String>, dark: impl Into<String>) -> Self {
        ColorToken {
            name: name.into(),
            light: light.into(),
            dark: dark.into(),
        }
    }
}

/// The panel's own look, the theme named `ignite`.
#[derive(Debug, Clone, Copy, Default)]
pub struct IgniteTheme;

impl AdminTheme for IgniteTheme {
    fn name(&self) -> &str {
        IGNITE_THEME
    }

    fn layout(&self) -> &str {
        IGNITE_LAYOUT
    }
}

/// Why a theme could not be registered or chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ThemeError {
    /// No theme is registered with this name.
    Unknown(String),
    /// A theme with this name is already registered.
    AlreadyRegistered(String),
    /// The theme's name is empty or has a character other than a lowercase letter, a digit,
    /// `-` or `_`.
    InvalidName(String),
    /// A colour token's name or value could break out of the CSS it is printed in.
    InvalidColor {
        /// The theme's name.
        theme: String,
        /// The token's name.
        token: String,
    },
    /// A release build renders only the views compiled into the panel, which are the
    /// `ignite` theme's, so it cannot show this theme.
    NotCompiled(String),
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeError::Unknown(name) => write!(f, "no admin theme is registered as `{name}`"),
            ThemeError::AlreadyRegistered(name) => {
                write!(f, "an admin theme is already registered as `{name}`")
            }
            ThemeError::InvalidName(name) => write!(
                f,
                "`{name}` is not a valid admin theme name: use lowercase letters, digits, `-` and `_`"
            ),
            ThemeError::InvalidColor { theme, token } => write!(
                f,
                "the admin theme `{theme}` has an invalid colour token `{token}`: use letters, digits and `-` in the name, and a CSS colour value"
            ),
            ThemeError::NotCompiled(name) => write!(
                f,
                "a release build renders the compiled `{IGNITE_THEME}` views and cannot show the admin theme `{name}`"
            ),
        }
    }
}

impl std::error::Error for ThemeError {}

/// A registered theme, with the CSS of its colour tokens, built once when it is registered.
pub(crate) struct ThemeEntry {
    pub(crate) theme: Box<dyn AdminTheme>,
    /// The `<style>` body of the theme's colour tokens, or empty when it has none.
    pub(crate) style: String,
}

/// The registered themes, and the active one.
struct Themes {
    registered: Vec<Arc<ThemeEntry>>,
    active: Arc<ThemeEntry>,
}

fn themes() -> &'static RwLock<Themes> {
    static THEMES: OnceLock<RwLock<Themes>> = OnceLock::new();
    THEMES.get_or_init(|| {
        let ignite = Arc::new(ThemeEntry {
            theme: Box::new(IgniteTheme),
            style: String::new(),
        });
        RwLock::new(Themes {
            registered: vec![Arc::clone(&ignite)],
            active: ignite,
        })
    })
}

/// Registers `theme`, so [`set_admin_theme`] can choose it by its name. It does not make the
/// theme active.
///
/// Refuses a theme whose name is taken (the `ignite` theme's included) or is not valid, or
/// whose colour tokens could break out of the CSS they are printed in.
pub fn register_admin_theme(theme: impl AdminTheme) -> Result<(), ThemeError> {
    let name = theme.name().to_string();
    if !valid_name(&name) {
        return Err(ThemeError::InvalidName(name));
    }
    let style = style_of(&name, &theme.colors())?;
    let mut themes = themes().write().unwrap_or_else(PoisonError::into_inner);
    if themes.registered.iter().any(|t| t.theme.name() == name) {
        return Err(ThemeError::AlreadyRegistered(name));
    }
    themes.registered.push(Arc::new(ThemeEntry {
        theme: Box::new(theme),
        style,
    }));
    Ok(())
}

/// Makes the theme registered as `name` the panel's look, from the next request on. The
/// built-in theme is `ignite`.
///
/// Refuses a name no theme is registered with, and keeps the active theme. A release build
/// also refuses every theme but `ignite` (see [`ThemeError::NotCompiled`]).
pub fn set_admin_theme(name: &str) -> Result<(), ThemeError> {
    let mut themes = themes().write().unwrap_or_else(PoisonError::into_inner);
    let found = themes
        .registered
        .iter()
        .find(|t| t.theme.name() == name)
        .cloned()
        .ok_or_else(|| ThemeError::Unknown(name.to_string()))?;
    if cfg!(not(debug_assertions)) && name != IGNITE_THEME {
        return Err(ThemeError::NotCompiled(name.to_string()));
    }
    themes.active = found;
    Ok(())
}

/// The name of the active theme.
pub fn active_admin_theme() -> String {
    active().theme.name().to_string()
}

/// The active theme.
pub(crate) fn active() -> Arc<ThemeEntry> {
    Arc::clone(
        &themes()
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .active,
    )
}

/// A theme name: lowercase ASCII letters, digits, `-` and `_`.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// The CSS of `tokens`: the light values on `:root`, the dark values on the dark mode's root.
/// The layout prints it raw, so a name may hold only letters, digits and `-`, and a value
/// only what a CSS colour needs: no `;`, braces, quotes or angle brackets.
fn style_of(theme: &str, tokens: &[ColorToken]) -> Result<String, ThemeError> {
    if tokens.is_empty() {
        return Ok(String::new());
    }
    let name_ok = |name: &str| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    let value_ok = |value: &str| {
        !value.trim().is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || " #%()./,+-_".contains(c))
    };
    let mut light = String::from(":root{");
    let mut dark = String::from(":root[data-theme=\"dark\"]{");
    for token in tokens {
        if !name_ok(&token.name) || !value_ok(&token.light) || !value_ok(&token.dark) {
            return Err(ThemeError::InvalidColor {
                theme: theme.to_string(),
                token: token.name.clone(),
            });
        }
        light.push_str(&format!("--color-{}:{};", token.name, token.light.trim()));
        dark.push_str(&format!("--color-{}:{};", token.name, token.dark.trim()));
    }
    Ok(format!("{light}}}{dark}}}"))
}

/// The view a page asks for, as the active theme names it: the layout and the partials of the
/// look resolve to the theme's views, and every other name is unchanged.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
fn themed_name<'a>(theme: &'a dyn AdminTheme, name: &'a str) -> &'a str {
    match name {
        "layouts.app" => theme.layout(),
        "partials.nav" => theme.navigation(),
        "partials.topbar" => theme.top_bar(),
        "partials.flash" => theme.flash(),
        other => other,
    }
}

/// Serves the panel's `ignite` view namespace through the active theme: the layout and the
/// partials of the look are the theme's, the theme's own views come from the theme, and every
/// other view from `base` (the views compiled into the binary, or a views directory).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub(crate) struct ThemedViews<L> {
    base: L,
}

#[cfg_attr(not(debug_assertions), allow(dead_code))]
impl<L> ThemedViews<L> {
    /// Serves `base`'s views through the active theme.
    pub(crate) fn new(base: L) -> Self {
        ThemedViews { base }
    }
}

impl<L: Loader> Loader for ThemedViews<L> {
    fn source(&self, name: &str) -> io::Result<String> {
        let entry = active();
        let name = themed_name(entry.theme.as_ref(), name);
        match entry.theme.view(name) {
            Some(source) => Ok(source),
            None => self.base.source(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ignite_theme_names_its_own_views() {
        let theme = IgniteTheme;
        assert_eq!(themed_name(&theme, "layouts.app"), IGNITE_LAYOUT);
        assert_eq!(themed_name(&theme, "partials.nav"), IGNITE_NAVIGATION);
        assert_eq!(themed_name(&theme, "partials.topbar"), IGNITE_TOP_BAR);
        assert_eq!(themed_name(&theme, "partials.flash"), IGNITE_FLASH);
        assert_eq!(themed_name(&theme, "index"), "index");
        assert_eq!(themed_name(&theme, "partials.field"), "partials.field");
    }

    #[test]
    fn colour_tokens_become_css_for_both_modes() {
        let css = style_of(
            "t",
            &[
                ColorToken::new("primary", "#2271b1", "#72aee6"),
                ColorToken::new("page", "oklch(100% 0 0)", "rgb(29, 35, 39)"),
            ],
        )
        .unwrap();
        assert_eq!(
            css,
            ":root{--color-primary:#2271b1;--color-page:oklch(100% 0 0);}\
             :root[data-theme=\"dark\"]{--color-primary:#72aee6;--color-page:rgb(29, 35, 39);}"
        );
        assert_eq!(style_of("t", &[]).unwrap(), "");
    }

    #[test]
    fn a_token_that_could_leave_the_style_element_is_refused() {
        for (name, value) in [
            ("primary", "red;}body{display:none"),
            ("primary", "</style><script>"),
            ("primary", "url(\"x\")"),
            ("pri mary", "red"),
            ("primary", " "),
        ] {
            assert!(
                style_of("t", &[ColorToken::new(name, value, "red")]).is_err(),
                "{name}: {value}"
            );
        }
    }

    #[test]
    fn theme_names_are_lowercase_words() {
        assert!(valid_name("ignite"));
        assert!(valid_name("wp-like"));
        assert!(valid_name("theme_2"));
        assert!(!valid_name(""));
        assert!(!valid_name("WP"));
        assert!(!valid_name("a.b"));
        assert!(!valid_name("a/b"));
    }
}
