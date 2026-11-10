//! The builder that collects resources and attaches the panel to Rocket.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignite_core::{build_pool, Field, IgniteResource, Pool};
use rocket::catchers;
use rocket::fairing::{self, AdHoc, Fairing, Info, Kind};
use rocket::{Build, Request, Response, Rocket};

use crate::erased::{Adapter, Erased};
use crate::proxy::TrustedProxies;
use crate::routes;
use crate::session;

/// The environment variable that names the panel's view directory at run time: a
/// directory of the `ignite` views, such as `ignite-panel/views/ignite`. It is read
/// only by debug builds, and a `views_dir` call on the builder wins over it.
pub const VIEWS_DIR_ENV: &str = "IGNITE_VIEWS_DIR";

/// The environment variable that lists the proxies whose forwarded headers the panel
/// believes, as IP addresses and CIDR ranges. See [`crate::proxy`].
pub const TRUSTED_PROXIES_ENV: &str = "IGNITE_TRUSTED_PROXIES";

/// The default URL prefix of the admin panel.
const DEFAULT_MOUNT: &str = "/admin";

/// Slugs the panel's own routes use under the mount, so a resource with one of them
/// would be shadowed: its list would never load. `resource` panics on these. The
/// `/<slug>` routes come after the fixed ones, so a fixed route always wins. The
/// health check `/up` is mounted at the application root, not under the mount, so it
/// cannot collide with a resource and is not listed.
const RESERVED_SLUGS: &[&str] = &[
    "login",
    "logout",
    "logout-everywhere",
    "profile",
    "theme",
    "build",
];

/// The panel's state, shared by every route. Rocket manages it as state.
pub(crate) struct Registry {
    /// The URL prefix, without a trailing slash, such as `/admin`.
    pub(crate) mount: String,
    pub(crate) resources: Vec<Arc<dyn Erased>>,
    /// The fields the app adds to every profile.
    pub(crate) profile_fields: Vec<Field>,
    /// The key that signs the session and flash cookies.
    pub(crate) secret: Vec<u8>,
    /// Paths that the app's CSRF guard does not check (see `Ignite::csrf_exempt`).
    pub(crate) csrf_exempt: Vec<String>,
    /// The proxies whose forwarded headers name the client (see `IGNITE_TRUSTED_PROXIES`).
    pub(crate) trusted_proxies: TrustedProxies,
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
    profile_fields: Vec<Field>,
    secret: Vec<u8>,
    views: Option<PathBuf>,
    csrf_exempt: Vec<String>,
    trusted_proxies: TrustedProxies,
}

impl Ignite {
    /// Loads a `.env` file from the working directory, if there is one, then reads
    /// `DATABASE_URL`, `IGNITE_SECRET_KEY`, `IGNITE_TRUSTED_PROXIES` and `IGNITE_VIEWS_DIR`
    /// and builds the connection pool. Variables already set in the environment win
    /// over the `.env` file, as in Laravel. The `ignite` view namespace is registered
    /// when the fairing is built.
    ///
    /// The panel does not run migrations here. Run `ignite migrate` before the first start.
    ///
    /// # Panics
    ///
    /// Panics if `DATABASE_URL` is not set, if the pool cannot connect, if
    /// `IGNITE_TRUSTED_PROXIES` has an entry that is not an address or a range, or if a
    /// release build has no valid `IGNITE_SECRET_KEY`. These are configuration errors
    /// that should stop the app at startup.
    // `new` reads the environment and can panic, so it is not a plain default.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        crate::load_env();
        let url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set to a mysql:// URL for the Ignite panel");
        let pool = build_pool(&url).expect("Ignite could not connect to DATABASE_URL");
        // Debug builds render the embedded views unless a directory overrides them.
        // The override is read here and checked when the fairing is built, so a
        // builder call to `views_dir` can still replace it.
        let views = std::env::var(VIEWS_DIR_ENV).ok().map(PathBuf::from);
        let trusted_proxies = match std::env::var(TRUSTED_PROXIES_ENV) {
            Ok(list) => TrustedProxies::parse(&list)
                .unwrap_or_else(|error| panic!("{TRUSTED_PROXIES_ENV}: {error}")),
            Err(_) => TrustedProxies::default(),
        };

        Ignite {
            pool,
            mount: DEFAULT_MOUNT.to_string(),
            resources: Vec::new(),
            profile_fields: Vec::new(),
            secret: session::secret_from_env(),
            views,
            csrf_exempt: Vec::new(),
            trusted_proxies,
        }
    }

    /// Adds a resource. Its [`SLUG`](IgniteResource::SLUG) sets its URL.
    ///
    /// # Panics
    ///
    /// Panics if another resource already uses the same slug, or if the slug is one
    /// the panel reserves for its own routes, such as `profile` or `login`.
    pub fn resource<R: IgniteResource>(mut self) -> Self {
        assert!(
            !RESERVED_SLUGS.contains(&R::SLUG),
            "the slug `{}` is reserved for the panel's own routes; choose another slug",
            R::SLUG
        );
        assert!(
            !self.resources.iter().any(|r| r.slug() == R::SLUG),
            "two Ignite resources use the slug `{}`",
            R::SLUG
        );
        self.resources.push(Arc::new(Adapter::<R>::new()));
        self
    }

    /// Adds a field to every user's profile, after the name and the email. The value is
    /// stored with the user and shown in the profile form. Use the same [`Field`] as a
    /// resource: its `name` is the key the value is stored under.
    ///
    /// # Panics
    ///
    /// Panics if the field's name is `name`, `email`, or another profile field's name.
    pub fn profile_field(mut self, field: Field) -> Self {
        assert!(
            !matches!(field.name, "name" | "email")
                && !self.profile_fields.iter().any(|f| f.name == field.name),
            "the profile already has a field named `{}`",
            field.name
        );
        self.profile_fields.push(field);
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
    /// under it. The panel's own routes are always checked. Each call adds to the
    /// list; a later call does not replace an earlier one.
    pub fn csrf_exempt(mut self, paths: &[&str]) -> Self {
        self.csrf_exempt.extend(paths.iter().map(|p| p.to_string()));
        self
    }

    /// Changes the URL prefix of the panel. The default is `/admin`.
    pub fn mount(mut self, path: &str) -> Self {
        self.mount = path.trim_end_matches('/').to_string();
        self
    }

    /// Returns the fairing. At ignite, before launch, it manages the pool and
    /// mounts the routes under the URL prefix. Rocket runs fairing hooks at
    /// ignite, not when `attach` is called, at this Rocket revision. On each
    /// response it adds `Secure` to the panel's cookies when the request was
    /// made over HTTPS.
    pub fn fairing(self) -> PanelFairing {
        let Ignite {
            pool,
            mount,
            resources,
            profile_fields,
            secret,
            views,
            csrf_exempt,
            trusted_proxies,
        } = self;
        // The panel registers its own assets, so an app adds nothing for its styling.
        crate::assets::register(&mount);
        register_views(views.as_deref());
        let ignite = AdHoc::on_ignite("Ignite", move |rocket| async move {
            let registry = Registry {
                mount: mount.clone(),
                resources,
                profile_fields,
                secret,
                csrf_exempt,
                trusted_proxies,
            };
            rocket
                .manage(pool)
                .manage(registry)
                .register(
                    "/",
                    catchers![
                        session::unauthorized,
                        session::csrf_mismatch,
                        session::server_error
                    ],
                )
                .mount("/", routes::health())
                .mount(mount, routes::all())
        });
        PanelFairing { ignite }
    }
}

/// The fairing [`Ignite::fairing`] returns. Its ignite hook is the panel's setup. Its
/// response hook marks the panel's cookies `Secure` over HTTPS.
pub struct PanelFairing {
    ignite: AdHoc,
}

#[rocket::async_trait]
impl Fairing for PanelFairing {
    fn info(&self) -> Info {
        Info {
            name: "Ignite",
            kind: Kind::Ignite | Kind::Response,
        }
    }

    async fn on_ignite(&self, rocket: Rocket<Build>) -> fairing::Result {
        self.ignite.on_ignite(rocket).await
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        crate::session::mark_secure_cookies(request, response);
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
