//! The builder that collects resources and attaches the panel to Rocket.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignite_core::{build_pool, IgniteResource, Pool};
use propel_rocket::register_namespace;
use rocket::catchers;
use rocket::fairing::AdHoc;

use crate::erased::{Adapter, Erased};
use crate::routes;
use crate::session;

/// The directory of the `ignite` view namespace, shipped with this crate.
///
/// Used when neither `views_dir` nor `IGNITE_VIEWS_DIR` names a directory. It
/// exists on the machine that built the crate, not on a deployed server.
const DEFAULT_VIEWS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/views/ignite");

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
        // The views directory is checked when the fairing is built, so a builder
        // call to `views_dir` can still name it.
        let views = match std::env::var(VIEWS_DIR_ENV) {
            Ok(path) => Some(PathBuf::from(path)),
            Err(_) if Path::new(DEFAULT_VIEWS_DIR).is_dir() => {
                Some(PathBuf::from(DEFAULT_VIEWS_DIR))
            }
            Err(_) => None,
        };

        Ignite {
            pool,
            mount: DEFAULT_MOUNT.to_string(),
            resources: Vec::new(),
            secret: session::secret_from_env(),
            views,
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

    /// Reads the panel's views from `path` instead of the default. Use it when the
    /// views are shipped next to a deployed binary. The directory must hold
    /// `layouts/app.propel`, and an override here wins over `IGNITE_VIEWS_DIR`.
    ///
    /// # Panics
    ///
    /// The check runs when the fairing is built, so this method does not panic.
    pub fn views_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.views = Some(path.into());
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
        } = self;
        let views = views.unwrap_or_else(|| {
            panic!(
                "Ignite could not find its views: ship the views/ignite directory, or set {VIEWS_DIR_ENV}, or call views_dir"
            )
        });
        register_views(&views);
        AdHoc::on_ignite("Ignite", move |rocket| async move {
            let registry = Registry {
                mount: mount.clone(),
                resources,
                secret,
            };
            rocket
                .manage(pool)
                .manage(registry)
                .register("/", catchers![session::unauthorized])
                .mount(mount, routes::all())
        })
    }
}

/// Registers the panel's views from `dir`. Panics if the directory does not hold them.
fn register_views(dir: &Path) {
    assert!(
        dir.join("layouts/app.propel").is_file(),
        "Ignite could not find its views in {}: ship the views/ignite directory, or set {VIEWS_DIR_ENV}",
        dir.display()
    );
    register_namespace("ignite", dir);
}
