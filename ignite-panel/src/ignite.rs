//! The builder that collects resources and attaches the panel to Rocket.

use std::sync::Arc;

use ignite_core::{build_pool, IgniteResource, Pool};
use propel_rocket::register_namespace;
use rocket::catchers;
use rocket::fairing::AdHoc;

use crate::erased::{Adapter, Erased};
use crate::routes;
use crate::session;

/// The directory of the `ignite` view namespace, shipped with this crate.
const VIEWS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/views/ignite");

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
}

impl Ignite {
    /// Reads `DATABASE_URL` and `IGNITE_SECRET_KEY`, builds the connection pool
    /// and registers the `ignite` view namespace.
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
        register_namespace("ignite", VIEWS_DIR);

        Ignite {
            pool,
            mount: DEFAULT_MOUNT.to_string(),
            resources: Vec::new(),
            secret: session::secret_from_env(),
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
        } = self;
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
