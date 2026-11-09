//! The builder that collects resources and attaches the panel to Rocket.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignite_core::{build_pool, IgniteResource, Pool};
use rocket::catchers;
use rocket::fairing::AdHoc;

use crate::erased::{Adapter, Erased};
use crate::routes;
use crate::session;

/// The directory of the `ignite` view namespace, shipped with this crate.
///
/// The environment variable that names the panel's view directory at run time.
pub const VIEWS_DIR_ENV: &str = "IGNITE_VIEWS_DIR";

/// The default URL prefix of the admin panel.
const DEFAULT_MOUNT: &str = "/admin";

/// The panel's state, shared by every route. Rocket manages it as state.
pub(crate) struct Registry {
    /// The URL prefix, without a trailing slash, such as `/admin`.
    pub(crate) mount: String,
    pub(crate) resources: Vec<Arc<dyn Erased>>,
    /// The key that signs the session and flash cookies.
    pub(crate) secret: Vec<u8>,
    /// Paths that the app's CSRF guard does not check (see `Ignite::csrf_exempt`).
    pub(crate) csrf_exempt: Vec<String>,
}

impl Registry {
    /// Finds the resource mounted at `slug`.
    pub(crate) fn find(&self, slug: &str) -> Option<Arc<dyn Erased>> {
        self.resources.iter().find(|r| r.slug() == slug).cloned()
    }
}

/// Builds the admin panel. Add resources with [`resource`](Self::resource),
/// then attach [`fairing`](Self::fairing) to a Rocket instance.
pub struct Ignite {
    pool: Pool,
    mount: String,
    resources: Vec<Arc<dyn Erased>>,
    secret: Vec<u8>,
    views: Option<PathBuf>,
    csrf_exempt: Vec<String>,
}

impl Ignite {
    /// Reads `DATABASE_URL` and `IGNITE_SECRET_KEY` and builds the connection pool.
    /// The `ignite` view namespace is registered when the fairing is built.
    ///
    /// # Panics
    ///
    /// Panics if `DATABASE_URL` is not set, if the pool cannot connect, or if a
    /// release build has no valid `IGNITE_SECRET_KEY`. These are configuration
    /// errors that should stop the app at startup.
    // `new` reads the environment and can panic, so it is not a plain default.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set to a mysql:// URL for the Ignite panel");
        let pool = build_pool(&url).expect("Ignite could not connect to DATABASE_URL");
        // Debug builds render the embedded views unless a directory overrides them.
        // The override is read here and checked when the fairing is built, so a
        // builder call to `views_dir` can still replace it.
        let views = std::env::var(VIEWS_DIR_ENV).ok().map(PathBuf::from);

        Ignite {
            pool,
            mount: DEFAULT_MOUNT.to_string(),
            resources: Vec::new(),
            secret: session::secret_from_env(),
            views,
            csrf_exempt: Vec::new(),
        }
    }

    /// Adds a resource. Its [`SLUG`](IgniteResource::SLUG) sets its URL.
    ///
    /// # Panics
    ///
    /// Panics if another resource already uses the same slug.
    pub fn resource<R: IgniteResource>(mut self) -> Self {
        assert!(
            !self.resources.iter().any(|r| r.slug() == R::SLUG),
            "two Ignite resources use the slug `{}`",
            R::SLUG
        );
        self.resources.push(Arc::new(Adapter::<R>::new()));
        self
    }

    /// Reads the panel's views from `path` instead of the views embedded in the
    /// binary. Use it to edit views without rebuilding. The directory must hold
    /// `layouts/app.propel`, and an override here wins over `IGNITE_VIEWS_DIR`.
    ///
    /// # Panics
    ///
    /// The check runs when the fairing is built, so this method does not panic.
    pub fn views_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.views = Some(path.into());
        self
    }

    /// Paths the app's CSRF guard (`CsrfForm`) does not check, such as a payment
    /// webhook that the provider posts to. A path ends with `/*` to match everything
    /// under it. The panel's own routes are always checked.
    pub fn csrf_exempt(mut self, paths: &[&str]) -> Self {
        self.csrf_exempt = paths.iter().map(|p| p.to_string()).collect();
        self
    }

    /// Changes the URL prefix of the panel. The default is `/admin`.
    pub fn mount(mut self, path: &str) -> Self {
        self.mount = path.trim_end_matches('/').to_string();
        self
    }

    /// Returns the fairing. At ignite, before launch, it manages the pool and
    /// mounts the routes under the URL prefix. Rocket runs fairing hooks at
    /// ignite, not when `attach` is called, at this Rocket revision.
    pub fn fairing(self) -> AdHoc {
        let Ignite {
            pool,
            mount,
            resources,
            secret,
            views,
            csrf_exempt,
        } = self;
        // The panel registers its own assets, so an app adds nothing for its styling.
        crate::assets::register(&mount);
        register_views(views.as_deref());
        AdHoc::on_ignite("Ignite", move |rocket| async move {
            let registry = Registry {
                mount: mount.clone(),
                resources,
                secret,
                csrf_exempt,
            };
            rocket
                .manage(pool)
                .manage(registry)
                .register(
                    "/",
                    catchers![session::unauthorized, session::csrf_mismatch],
                )
                .mount(mount, routes::all())
        })
    }
}

/// Registers the panel's views for the dev renderer: a directory when one is named, or
/// the embedded views. A release build renders compiled views, so it registers nothing.
#[cfg(debug_assertions)]
fn register_views(dir: Option<&Path>) {
    match dir {
        Some(dir) => {
            check_views(dir);
            propel_rocket::register_namespace("ignite", dir);
        }
        None => propel_rocket::register_namespace_loader("ignite", crate::embedded::EmbeddedViews),
    }
}

/// See the debug version.
#[cfg(not(debug_assertions))]
fn register_views(_dir: Option<&Path>) {}

/// Checks that a views directory holds the panel's layout. Panics when it does not.
#[cfg(debug_assertions)]
fn check_views(dir: &Path) {
    assert!(
        dir.join("layouts/app.propel").is_file(),
        "Ignite could not find its views in {}: ship the views/ignite directory, or set {VIEWS_DIR_ENV}",
        dir.display()
    );
}
