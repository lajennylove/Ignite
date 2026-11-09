//! The panel's views, compiled into the crate. A binary that uses the panel
//! needs no view files on disk: the templates are part of the binary, as a
//! Laravel package's views are part of the package.

use std::io;

use propel_runtime::Loader;

/// Serves the panel's `.propel` views from the binary. The names are the
/// view names: `index`, `layouts.app`, `partials.nav`, and so on.
pub struct EmbeddedViews;

/// Every view the panel ships, by name. A test checks this list against the
/// `views/ignite` directory, so a new view cannot be left out.
pub(crate) const VIEWS: &[(&str, &str)] = &[
    (
        "dashboard",
        include_str!("../views/ignite/dashboard.propel"),
    ),
    ("form", include_str!("../views/ignite/form.propel")),
    ("index", include_str!("../views/ignite/index.propel")),
    ("login", include_str!("../views/ignite/login.propel")),
    (
        "layouts.app",
        include_str!("../views/ignite/layouts/app.propel"),
    ),
    (
        "partials.flash",
        include_str!("../views/ignite/partials/flash.propel"),
    ),
    (
        "partials.nav",
        include_str!("../views/ignite/partials/nav.propel"),
    ),
];

impl Loader for EmbeddedViews {
    fn source(&self, name: &str) -> io::Result<String> {
        VIEWS
            .iter()
            .find(|(view, _)| *view == name)
            .map(|(_, source)| source.to_string())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, format!("no panel view `{name}`"))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The view name of a file under `dir`, such as `layouts.app` for `layouts/app.propel`.
    fn collect(dir: &Path, prefix: &str, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                collect(&path, &format!("{prefix}{name}."), found);
            } else if let Some(stem) = name.strip_suffix(".propel") {
                found.push(format!("{prefix}{stem}"));
            }
        }
    }

    #[test]
    fn every_view_file_is_embedded() {
        let views = Path::new(env!("CARGO_MANIFEST_DIR")).join("views/ignite");
        let mut on_disk = Vec::new();
        collect(&views, "", &mut on_disk);
        on_disk.sort();

        let mut embedded: Vec<String> = VIEWS.iter().map(|(name, _)| name.to_string()).collect();
        embedded.sort();
        assert_eq!(embedded, on_disk, "add new views to VIEWS");
    }

    #[test]
    fn an_embedded_view_matches_its_file() {
        let on_disk = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("views/ignite/layouts/app.propel"),
        )
        .unwrap();
        assert_eq!(EmbeddedViews.source("layouts.app").unwrap(), on_disk);
    }

    #[test]
    fn an_unknown_view_is_not_found() {
        let error = EmbeddedViews.source("nothing.here").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
