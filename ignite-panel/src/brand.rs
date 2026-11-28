//! The panel's brand: the name, the logo and the favicon, as Filament's panel sets them with
//! `brandName`, `brandLogo`, `darkModeBrandLogo`, `brandLogoHeight` and `favicon`, and the app's stylesheets, as
//! Filament registers an app's CSS with `FilamentAsset::register`. The app sets them on the
//! [`Ignite`](crate::Ignite) builder; the views read them from every page's `brand` value, so
//! the compiled views of a release build show them too.

use serde::Serialize;

/// The brand name the panel shows when the app sets none.
pub(crate) const DEFAULT_BRAND_NAME: &str = "Ignite";

/// The height of an app's logo when the app sets none: Filament's default, `1.5rem`.
pub(crate) const DEFAULT_LOGO_HEIGHT: &str = "1.5rem";

/// The brand the app set on the builder. A part left `None` keeps Ignite's own.
#[derive(Debug, Clone, Default)]
pub(crate) struct Brand {
    pub(crate) name: Option<String>,
    pub(crate) logo: Option<String>,
    /// The logo shown in dark mode, as Filament's `darkModeBrandLogo`.
    pub(crate) dark_logo: Option<String>,
    pub(crate) logo_height: Option<String>,
    pub(crate) favicon: Option<String>,
    /// The app's stylesheets, in the order the app added them.
    pub(crate) stylesheets: Vec<String>,
}

/// The brand as the views read it, as `brand.name` and so on. Propel reads a path, not a
/// comparison, so each choice the views make is a flag.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct BrandView {
    /// The brand name: the text of the brand when there is no logo, the logo's alternative
    /// text when there is one, and the end of every page title.
    pub(crate) name: String,
    /// Whether the app set a logo. The logo then replaces the name, as in Filament.
    pub(crate) has_logo: bool,
    /// The path or URL of the app's logo.
    pub(crate) logo: String,
    /// Whether the app set a logo for dark mode, as Filament's `darkModeBrandLogo`. The views
    /// then show it only in dark mode (`ig-logo-dark`), and the logo or the name only in light
    /// mode (`ig-logo-light`).
    pub(crate) has_dark_logo: bool,
    /// The path or URL of the app's dark mode logo.
    pub(crate) dark_logo: String,
    /// The CSS height of the app's logos, such as `2rem`.
    pub(crate) logo_height: String,
    /// Whether the views show the Ignite mark beside the name: only when the app set neither a
    /// name nor a logo, so the default brand is the one Ignite always showed.
    pub(crate) has_mark: bool,
    /// Whether the app set a favicon. It then replaces Ignite's favicon links.
    pub(crate) has_favicon: bool,
    /// The path or URL of the app's favicon.
    pub(crate) favicon: String,
    /// The paths or URLs of the app's stylesheets, in order. The layout links each one at the
    /// end of the head, after the panel's own CSS. Empty by default.
    pub(crate) stylesheets: Vec<String>,
}

impl BrandView {
    /// The view of `brand`.
    pub(crate) fn new(brand: &Brand) -> Self {
        BrandView {
            name: brand
                .name
                .clone()
                .unwrap_or_else(|| DEFAULT_BRAND_NAME.to_string()),
            has_logo: brand.logo.is_some(),
            logo: brand.logo.clone().unwrap_or_default(),
            has_dark_logo: brand.dark_logo.is_some(),
            dark_logo: brand.dark_logo.clone().unwrap_or_default(),
            logo_height: brand
                .logo_height
                .clone()
                .unwrap_or_else(|| DEFAULT_LOGO_HEIGHT.to_string()),
            has_mark: brand.name.is_none() && brand.logo.is_none() && brand.dark_logo.is_none(),
            has_favicon: brand.favicon.is_some(),
            favicon: brand.favicon.clone().unwrap_or_default(),
            stylesheets: brand.stylesheets.clone(),
        }
    }
}

/// Whether `height` is a CSS length the layout can print in a `style` attribute: a number and
/// a unit or `%`, such as `2rem`, `48px` or `100%`. Anything else could add other CSS.
pub(crate) fn valid_height(height: &str) -> bool {
    let number_end = height
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(height.len());
    let (number, unit) = height.split_at(number_end);
    !number.is_empty()
        && number.chars().any(|c| c.is_ascii_digit())
        && number.matches('.').count() <= 1
        && (unit == "%" || unit.chars().all(|c| c.is_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_brand_is_ignite_with_its_mark() {
        let view = BrandView::new(&Brand::default());
        assert_eq!(view.name, "Ignite");
        assert!(view.has_mark);
        assert!(!view.has_logo);
        assert!(!view.has_favicon);
        assert!(view.stylesheets.is_empty());
    }

    #[test]
    fn a_name_or_a_logo_replaces_the_mark() {
        let named = BrandView::new(&Brand {
            name: Some("Mission Control".into()),
            ..Brand::default()
        });
        assert_eq!(named.name, "Mission Control");
        assert!(!named.has_mark);
        assert!(!named.has_logo);

        let logo = BrandView::new(&Brand {
            logo: Some("/images/logo.webp".into()),
            ..Brand::default()
        });
        assert!(logo.has_logo);
        assert!(!logo.has_mark);
        assert_eq!(
            logo.name, "Ignite",
            "the name stays the logo's alternative text"
        );
        assert_eq!(logo.logo_height, "1.5rem");
        assert!(!logo.has_dark_logo);
    }

    #[test]
    fn a_dark_mode_logo_is_its_own_choice() {
        let view = BrandView::new(&Brand {
            logo: Some("/images/logo.webp".into()),
            dark_logo: Some("/images/logo-dark.webp".into()),
            ..Brand::default()
        });
        assert!(view.has_logo);
        assert!(view.has_dark_logo);
        assert_eq!(view.dark_logo, "/images/logo-dark.webp");

        // A dark logo alone keeps the name in light mode, without the Ignite mark.
        let dark_only = BrandView::new(&Brand {
            dark_logo: Some("/images/logo-dark.webp".into()),
            ..Brand::default()
        });
        assert!(!dark_only.has_logo);
        assert!(!dark_only.has_mark);
    }

    #[test]
    fn a_logo_height_is_a_css_length() {
        for height in ["2rem", "48px", "100%", "1.5rem", "3em", "0"] {
            assert!(valid_height(height), "{height}");
        }
        for height in [
            "",
            "rem",
            "2rem;color:red",
            "2 rem",
            "1..5rem",
            "2rem\"",
            "-1px",
            ".",
        ] {
            assert!(!valid_height(height), "{height}");
        }
    }
}
