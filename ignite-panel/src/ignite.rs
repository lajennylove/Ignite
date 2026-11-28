//! The builder that collects resources and attaches the panel to Rocket.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ignite_core::{
    Database, Field, FormWidget, IgniteResource, ListWidget, RegisteredResource, RuntimeResource,
    StatsOverviewWidget,
};
use orbit_orm::{PoolOptions, Transaction};
use rocket::catchers;
use rocket::fairing::{self, AdHoc, Fairing, Info, Kind};
use rocket::{Build, Request, Response, Rocket};

use crate::brand::{self, Brand};
use crate::erased::{Adapter, Erased, RuntimeAdapter};
use crate::mail::{Mailer, NotConfigured};
use crate::navigation::{NavigationGroup, NavigationItem};
use crate::not_found::NotFound;
use crate::profile::{ProfileFields, ProfilePicture};
use crate::proxy::TrustedProxies;
use crate::routes;
use crate::session;
use crate::users::{self, UserCallbackFuture, UserCallbacks};

/// The environment variable that names the panel's view directory at run time: a
/// directory of the `ignite` views, such as `ignite-panel/views/ignite`. It is read
/// only by debug builds, and a `views_dir` call on the builder wins over it.
pub const VIEWS_DIR_ENV: &str = "IGNITE_VIEWS_DIR";

/// The environment variable that lists the proxies whose forwarded headers the panel
/// believes, as IP addresses and CIDR ranges. See [`crate::proxy`].
pub const TRUSTED_PROXIES_ENV: &str = "IGNITE_TRUSTED_PROXIES";

/// The environment variable that holds the app's own URL, such as `https://app.example.test`.
/// Password reset links are built from it. See [`crate::password_reset`].
pub const APP_URL_ENV: &str = "IGNITE_APP_URL";

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
    "password",
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
    /// Sends the panel's email, such as a password reset link. See [`Ignite::mailer`].
    pub(crate) mailer: Arc<dyn Mailer>,
    /// The app's own URL without a trailing slash, for the links in email (see `IGNITE_APP_URL`).
    pub(crate) app_url: Option<String>,
    /// Whether a changed form asks before the user leaves it (see
    /// [`Ignite::unsaved_changes_alerts`]).
    pub(crate) unsaved_changes_alerts: bool,
    /// The slugs of the runtime resources whose permission rows this panel has registered.
    pub(crate) runtime_permissions: Mutex<HashSet<String>>,
    /// The brand the app set: the name, the logo and the favicon (see [`Ignite::brand_name`]).
    pub(crate) brand: Brand,
    /// The navigation groups in the order the sidebar shows them (see
    /// [`Ignite::navigation_groups`]).
    pub(crate) navigation_groups: Vec<NavigationGroup>,
    /// The navigation entries the panel declares itself, such as a parent that is only a
    /// label (see [`Ignite::navigation_items`]).
    pub(crate) navigation_items: Vec<NavigationItem>,
    /// Whether a group that does not say collapses (see
    /// [`Ignite::collapsible_navigation_groups`]).
    pub(crate) collapsible_navigation_groups: bool,
    /// The dashboard widgets of the panel's own (see [`Ignite::widget`]).
    pub(crate) widgets: Vec<Arc<dyn StatsOverviewWidget>>,
    /// The list and form widgets of the dashboard, in the order they were added (see
    /// [`Ignite::list_widget`] and [`Ignite::form_widget`]).
    pub(crate) dashboard_widgets: Vec<DashboardWidget>,
    /// The panel's 404 page (see [`Ignite::not_found`]).
    pub(crate) not_found: NotFound,
    /// The profile's fields whose values the app keeps (see [`Ignite::profile_fields`]).
    pub(crate) profile_extensions: Vec<ProfileFields>,
    /// The app's picture field, in place of the upload (see [`Ignite::profile_picture`]).
    pub(crate) profile_picture: Option<ProfilePicture>,
}

/// A dashboard widget that renders its own view: a list or a form.
pub(crate) enum DashboardWidget {
    List(ListWidget),
    Form(FormWidget),
}

impl Registry {
    /// Finds the resource at `slug`: a static resource first, then a runtime resource from
    /// [`ignite_core::register_runtime`]. The registry is read on each call, so a runtime
    /// resource is found from the request after it is registered, and not after it is removed.
    pub(crate) fn find(&self, slug: &str) -> Option<Arc<dyn Erased>> {
        if let Some(found) = self.resources.iter().find(|r| r.slug() == slug) {
            return Some(found.clone());
        }
        ignite_core::runtime_resource(slug)
            .map(|registered| Arc::new(RuntimeAdapter(registered)) as Arc<dyn Erased>)
    }

    /// Every resource, for the navigation and the dashboard: the static resources in the
    /// order they were added, then the runtime resources in the order they were registered.
    pub(crate) fn all(&self) -> Vec<Arc<dyn Erased>> {
        let runtime = ignite_core::runtime_resources()
            .into_iter()
            .map(|registered| Arc::new(RuntimeAdapter(registered)) as Arc<dyn Erased>);
        self.resources.iter().cloned().chain(runtime).collect()
    }

    /// Registers the permission rows of the runtime resources' abilities that do not have them
    /// yet (each action's ability and the access ability, see
    /// `RegisteredResource::ability_names`), as the panel registers a static resource's rows
    /// when it starts, so the Roles screen lists a resource's abilities without a restart. The
    /// panel calls it on each signed-in request, before it loads the user's abilities. No lock
    /// is held across the database call. A failure is logged, and the next request tries
    /// again.
    pub(crate) async fn sync_runtime_permissions(&self, db: &Database) {
        let missing: Vec<String> = {
            let done = self
                .runtime_permissions
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut missing: Vec<String> = Vec::new();
            for name in ignite_core::runtime_resources()
                .iter()
                .flat_map(|r| r.ability_names())
            {
                if !done.contains(&name) && !missing.contains(&name) {
                    missing.push(name);
                }
            }
            missing
        };
        if missing.is_empty() {
            return;
        }
        let names = missing.clone();
        match crate::authz::register(db, &names).await {
            Ok(()) => self
                .runtime_permissions
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend(missing),
            Err(error) => {
                rocket::error!("ignite: could not register runtime permissions: {error}")
            }
        }
    }
}

/// Builds the admin panel. Add resources with [`resource`](Self::resource),
/// then attach [`fairing`](Self::fairing) to a Rocket instance.
pub struct Ignite {
    database_url: String,
    mount: String,
    resources: Vec<Arc<dyn Erased>>,
    profile_fields: Vec<Field>,
    secret: Vec<u8>,
    views: Option<PathBuf>,
    csrf_exempt: Vec<String>,
    trusted_proxies: TrustedProxies,
    mailer: Arc<dyn Mailer>,
    app_url: Option<String>,
    unsaved_changes_alerts: bool,
    user_callbacks: UserCallbacks,
    storage: PathBuf,
    brand: Brand,
    navigation_groups: Vec<NavigationGroup>,
    navigation_items: Vec<NavigationItem>,
    collapsible_navigation_groups: bool,
    widgets: Vec<Arc<dyn StatsOverviewWidget>>,
    dashboard_widgets: Vec<DashboardWidget>,
    roles: Vec<crate::Role>,
    abilities: Vec<String>,
    not_found: NotFound,
    profile_extensions: Vec<ProfileFields>,
    profile_picture: Option<ProfilePicture>,
}

impl Ignite {
    /// Loads a `.env` file from the working directory, if there is one, then reads
    /// `DATABASE_URL`, `IGNITE_SECRET_KEY`, `IGNITE_TRUSTED_PROXIES`, `IGNITE_APP_URL` and
    /// `IGNITE_VIEWS_DIR`. Variables already set in the environment win over the `.env` file,
    /// as in Laravel. The `ignite` view namespace is registered when the fairing is built.
    ///
    /// The database connects when Rocket ignites, not here, because connecting is async. A
    /// connection that fails stops the launch with an error, as the panel's own pool did
    /// before the move to Orbit it stopped the process at this call.
    ///
    /// The panel does not run migrations here. Run `ignite migrate` before the first start.
    ///
    /// # Panics
    ///
    /// Panics if `DATABASE_URL` is not set, if
    /// `IGNITE_TRUSTED_PROXIES` has an entry that is not an address or a range, if
    /// `IGNITE_APP_URL` is not an `http://` or `https://` URL, or if a release build has no
    /// valid `IGNITE_SECRET_KEY`. These are configuration errors
    /// that should stop the app at startup.
    // `new` reads the environment and can panic, so it is not a plain default.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        crate::load_env();
        let database_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set to a mysql:// URL for the Ignite panel");
        // Debug builds render the embedded views unless a directory overrides them.
        // The override is read here and checked when the fairing is built, so a
        // builder call to `views_dir` can still replace it.
        let views = std::env::var(VIEWS_DIR_ENV).ok().map(PathBuf::from);
        let trusted_proxies = match std::env::var(TRUSTED_PROXIES_ENV) {
            Ok(list) => TrustedProxies::parse(&list)
                .unwrap_or_else(|error| panic!("{TRUSTED_PROXIES_ENV}: {error}")),
            Err(_) => TrustedProxies::default(),
        };

        let app_url = std::env::var(APP_URL_ENV)
            .ok()
            .map(|url| url.trim().trim_end_matches('/').to_string())
            .filter(|url| !url.is_empty());
        if let Some(url) = &app_url {
            assert!(
                url.starts_with("http://") || url.starts_with("https://"),
                "{APP_URL_ENV} must start with http:// or https://"
            );
        }

        // A runtime resource cannot take the slug of one of the panel's pages.
        if let Err(error) = ignite_core::reserve_slugs(RESERVED_SLUGS) {
            panic!("{error}: it is reserved for the panel's own routes");
        }

        Ignite {
            database_url,
            mount: DEFAULT_MOUNT.to_string(),
            resources: Vec::new(),
            profile_fields: Vec::new(),
            secret: session::secret_from_env(),
            views,
            csrf_exempt: Vec::new(),
            trusted_proxies,
            mailer: Arc::new(NotConfigured),
            app_url,
            unsaved_changes_alerts: false,
            user_callbacks: UserCallbacks::default(),
            storage: PathBuf::from(users::DEFAULT_STORAGE_DIR),
            brand: Brand::default(),
            navigation_groups: Vec::new(),
            navigation_items: Vec::new(),
            collapsible_navigation_groups: true,
            widgets: Vec::new(),
            dashboard_widgets: Vec::new(),
            roles: Vec::new(),
            abilities: Vec::new(),
            not_found: NotFound::default(),
            profile_extensions: Vec::new(),
            profile_picture: None,
        }
    }

    /// Adds a resource. Its [`SLUG`](IgniteResource::SLUG) sets its URL.
    ///
    /// # Panics
    ///
    /// Panics if another resource, static or runtime, already uses the same slug, or if the
    /// slug is one the panel reserves for its own routes, such as `profile` or `login`.
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
        // A runtime resource registered later cannot take this slug.
        if let Err(error) = ignite_core::reserve_slugs(&[R::SLUG]) {
            panic!("{error}: a runtime resource has it, so the static resource cannot");
        }
        self.resources.push(Arc::new(Adapter::<R>::new()));
        self
    }

    /// Adds a resource defined by data ([`RuntimeResource`]) as part of the panel from its
    /// start, as [`resource`](Self::resource) adds a static one: its permission rows are
    /// registered when the app starts and its slug is reserved. The panel's Users and Roles
    /// screens are added this way (see [`crate::screens`]), so an app can extend them with the
    /// [`RuntimeResource`] builder first. To add and remove resources while the panel serves,
    /// use [`ignite_core::register_runtime`] instead.
    ///
    /// # Panics
    ///
    /// Panics if the slug or a field name is not valid, if another resource already uses the
    /// slug, or if the slug is one the panel reserves for its own routes.
    pub fn runtime_resource(mut self, resource: RuntimeResource) -> Self {
        let slug = resource.slug.clone();
        assert!(
            !RESERVED_SLUGS.contains(&slug.as_str()),
            "the slug `{slug}` is reserved for the panel's own routes; choose another slug"
        );
        assert!(
            !self.resources.iter().any(|r| r.slug() == slug),
            "two Ignite resources use the slug `{slug}`"
        );
        let registered =
            RegisteredResource::new(resource).unwrap_or_else(|error| panic!("{error}"));
        if let Err(error) = ignite_core::reserve_slugs(&[slug.as_str()]) {
            panic!("{error}: a runtime resource has it");
        }
        self.resources
            .push(Arc::new(RuntimeAdapter(Arc::new(registered))));
        self
    }

    /// Adds the panel's Users and Roles screens as they are: [`screens::users_resource`]
    /// (WordPress's Users screens: the list with a search and a view per role, add, edit,
    /// disable, enable and delete) and [`screens::roles_resource`] (the roles and their
    /// abilities). Listed in the navigation as "Users", with its "Add new" link, and "Roles",
    /// for the users whose roles grant `users.viewAny` and `roles.viewAny`. To change a
    /// screen, pass it to [`runtime_resource`](Self::runtime_resource) instead; to replace it,
    /// add a resource of the app's own at its slug.
    ///
    /// [`screens::users_resource`]: crate::screens::users_resource
    /// [`screens::roles_resource`]: crate::screens::roles_resource
    pub fn user_management(self) -> Self {
        self.runtime_resource(crate::screens::users_resource())
            .runtime_resource(crate::screens::roles_resource())
    }

    /// Declares a role and the abilities it starts with, such as
    /// `Role::make("author").label("Author").abilities(["posts.viewAny", "posts.create"])`, as
    /// WordPress's `add_role`. When the app starts, the abilities are registered as permissions
    /// and a role that does not exist yet is created with them; a role that exists keeps the
    /// abilities it has (see [`Role`](crate::Role)). The Users and Roles screens show the
    /// role's label. Each call adds one role.
    pub fn role(mut self, role: crate::Role) -> Self {
        self.roles.push(role);
        self
    }

    /// Declares abilities of the app's own, beside the resources' abilities, such as
    /// `settings.manage`: each is registered as a permission when the app starts, so the Roles
    /// screen offers it and a role can be granted it. Check it with
    /// [`AdminUser::can`](crate::AdminUser::can) or [`AdminUser::authorize`](crate::AdminUser::authorize),
    /// or on a navigation item with [`NavigationItem::ability`](crate::NavigationItem::ability).
    pub fn abilities<I, S>(mut self, abilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.abilities.extend(abilities.into_iter().map(Into::into));
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

    /// Adds fields to the profile form whose values the app loads and saves itself, such as a
    /// user's meta rows, as a Filament app adds components to its `EditProfile` page's form
    /// beside the name and the email. They render after the name, the email and the fields of
    /// [`profile_field`](Self::profile_field), with the same markup, and are saved by "Save
    /// details" with the same messages; see [`ProfileFields`]. Each call adds a group of fields
    /// after the ones added before; each group is loaded and saved through its own callbacks.
    ///
    /// # Panics
    ///
    /// The names are checked when the fairing is built: the fairing panics when a field's name
    /// is not a lowercase identifier, or when two of the profile's fields have the same name.
    pub fn profile_fields(mut self, fields: ProfileFields) -> Self {
        self.profile_extensions.push(fields);
        self
    }

    /// Replaces the profile's picture upload with the app's own field, such as a URL to an
    /// attachment or an image field with the media library's picker, loaded and saved through
    /// the app's callbacks, and gives the URL of the user's picture, as Filament's
    /// `HasAvatar::getFilamentAvatarUrl()`; see [`ProfilePicture`]. Without a call, the panel's
    /// upload stays. A later call replaces an earlier one.
    pub fn profile_picture(mut self, picture: ProfilePicture) -> Self {
        self.profile_picture = Some(picture);
        self
    }

    /// Reads the panel's views from `path` instead of the views embedded in the
    /// binary. Use it to edit views without rebuilding. The directory must hold
    /// `layouts/app.propel` and the `ignite` theme's views (`themes/ignite/layouts/app.propel`
    /// and `themes/ignite/partials/nav.propel`, `topbar.propel` and `flash.propel`), and an
    /// override here wins over `IGNITE_VIEWS_DIR`. The views are served through the active
    /// admin theme (see [`crate::theme`]), so edit the `ignite` theme's look in the
    /// directory's `themes/ignite`.
    ///
    /// # Panics
    ///
    /// The check runs when the fairing is built, so this method does not panic. The fairing
    /// panics when a file above is missing, and names the one that is.
    pub fn views_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.views = Some(path.into());
        self
    }

    /// Keeps the panel's files, such as the users' profile pictures (in its `avatars` folder),
    /// in `path` instead of `storage` in the working directory, as Laravel keeps them in
    /// `storage/`. The folders are created when the first file is written. The database keeps
    /// each file's path relative to this directory, so the directory can move.
    ///
    /// The directory is process-wide, as the user callbacks are, because
    /// [`users::delete_user`] and the command line remove a deleted user's picture with only a
    /// [`Database`](orbit_orm::Database): it takes effect when [`fairing`](Self::fairing) or
    /// [`run_cli`](Self::run_cli) is called, and the most recently configured `Ignite`'s
    /// directory is the one used. [`users::flush_storage_dir`] restores the default. The stock
    /// `ignite` binary uses the default.
    pub fn storage_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.storage = path.into();
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

    /// Sets the mailer the panel sends email with, such as [`EmailitMailer`](crate::EmailitMailer).
    /// Without a call, the panel has [`NotConfigured`], which refuses to send and logs why, so
    /// the password reset does not send anything until the app sets a mailer.
    pub fn mailer(mut self, mailer: impl Mailer + 'static) -> Self {
        self.mailer = Arc::new(mailer);
        self
    }

    /// Registers a callback that runs each time Ignite creates a user: from the panel (an app
    /// resource that calls [`users::create_user`]), from `ignite user create` run through
    /// [`run_cli`](Self::run_cli), or from a direct call to [`users::create_user`]. It is
    /// given the new user's id and the open transaction that inserted the user, so the app's
    /// own rows, such as a profile, are written atomically with the user's row:
    ///
    /// ```ignore
    /// Ignite::new().on_user_created(|user_id, tx| {
    ///     Box::pin(async move {
    ///         raw::execute(&mut *tx, "INSERT INTO profiles (user_id) VALUES (?)", &[user_id.into()])
    ///             .await?;
    ///         Ok(())
    ///     })
    /// })
    /// ```
    ///
    /// Several callbacks can be registered; they run in the order they were registered. When
    /// one returns an error, the later ones do not run, the whole creation is rolled back (no
    /// user row is left), and the error is returned to the caller: the panel answers with its
    /// error page and logs the error, and the command exits with status 1 and prints it.
    ///
    /// The callbacks are process-wide, as Laravel's model events are: they take effect when
    /// [`fairing`](Self::fairing) or [`run_cli`](Self::run_cli) is called, and replace the
    /// ones an earlier builder set, so the most recently configured `Ignite`'s callbacks are
    /// the ones that run. [`users::flush_callbacks`] removes them. The stock `ignite` binary
    /// has no app, so it runs none.
    pub fn on_user_created<F>(mut self, callback: F) -> Self
    where
        F: for<'t> Fn(i64, &'t mut Transaction) -> UserCallbackFuture<'t> + Send + Sync + 'static,
    {
        self.user_callbacks.created.push(Arc::new(callback));
        self
    }

    /// Registers a callback that runs each time Ignite deletes a user, before the user's row
    /// is deleted, as Laravel's `deleting` event and WordPress's `delete_user` action run: from
    /// the panel (an app resource that calls [`users::delete_user`]), from `ignite user delete`
    /// run through [`run_cli`](Self::run_cli), or from a direct call to [`users::delete_user`].
    /// It is given the user's id and the open transaction that deletes the user. The row still
    /// exists, so the callback can read the user and remove or reassign the app's rows that
    /// refer to it, such as rows whose foreign key would refuse the delete.
    ///
    /// Several callbacks run in the order they were registered. When one returns an error,
    /// nothing is deleted and the error is returned to the caller, as for
    /// [`on_user_created`](Self::on_user_created). The callbacks are process-wide: the most
    /// recently configured `Ignite` (its [`fairing`](Self::fairing) or
    /// [`run_cli`](Self::run_cli)) sets the ones that run.
    pub fn on_user_deleting<F>(mut self, callback: F) -> Self
    where
        F: for<'t> Fn(i64, &'t mut Transaction) -> UserCallbackFuture<'t> + Send + Sync + 'static,
    {
        self.user_callbacks.deleting.push(Arc::new(callback));
        self
    }

    /// Registers a callback that runs each time Ignite deletes a user, after the user's row
    /// is deleted and in the same transaction, as Laravel's `deleted` event and WordPress's
    /// `deleted_user` action run. The paths are the ones of
    /// [`on_user_deleting`](Self::on_user_deleting). It is given the id of the deleted user and
    /// the open transaction; the row is gone, and rows whose foreign key cascades are gone too.
    ///
    /// Several callbacks run in the order they were registered, after every deleting callback.
    /// When one returns an error, the whole delete is rolled back (the user is kept) and the
    /// error is returned to the caller. The callbacks are process-wide: the most recently
    /// configured `Ignite` sets the ones that run.
    pub fn on_user_deleted<F>(mut self, callback: F) -> Self
    where
        F: for<'t> Fn(i64, &'t mut Transaction) -> UserCallbackFuture<'t> + Send + Sync + 'static,
    {
        self.user_callbacks.deleted.push(Arc::new(callback));
        self
    }

    /// Runs the `ignite` command line in the process arguments with this app's user
    /// callbacks, and exits with the command's status, as Laravel's `artisan` is the app's own
    /// console. Call it from the app's own binary, so `ignite user create` and `ignite user
    /// delete` run the callbacks the panel runs:
    ///
    /// ```ignore
    /// // src/bin/ignite.rs
    /// fn main() {
    ///     Ignite::new().on_user_created(create_profile).run_cli();
    /// }
    /// ```
    ///
    /// The callbacks are process-wide: this call makes this builder's callbacks the ones that
    /// run in the process, replacing any set before. So does its
    /// [`storage_dir`](Self::storage_dir).
    ///
    /// [`Ignite::new`] loads `.env` and needs `DATABASE_URL`. The commands are the ones of
    /// [`crate::cli`]; the resources and other settings of the builder are not used.
    pub fn run_cli(self) -> ! {
        let args = std::env::args().skip(1);
        std::process::exit(self.call_cli(args))
    }

    /// Runs one `ignite` command with this app's user callbacks, which become the process's
    /// callbacks as with [`run_cli`](Self::run_cli), and returns its exit status
    /// instead of exiting, as Laravel's `Artisan::call` does. `args` are the command's
    /// arguments without the program name, such as `["user", "delete", "ada@example.com"]`.
    /// See [`run_cli`](Self::run_cli).
    pub fn call_cli<I>(self, args: I) -> i32
    where
        I: IntoIterator<Item = String>,
    {
        users::install_callbacks(self.user_callbacks);
        users::install_storage_dir(self.storage);
        crate::cli::run(&args.into_iter().collect::<Vec<_>>())
    }

    /// Asks before the user leaves a create or edit form they have changed and not saved, as
    /// Filament's `unsavedChangesAlerts()`: "Are you sure you want to leave? Without saving,
    /// everything is going to be lost." The panel's script asks when the user follows a link or
    /// closes or reloads the page (the browser shows its own wording for those). A form that
    /// has not changed leaves without asking, and saving it does not ask. A form shown again
    /// because its save failed counts as changed. The default, as in Filament, is off.
    pub fn unsaved_changes_alerts(mut self) -> Self {
        self.unsaved_changes_alerts = true;
        self
    }

    /// Sets the brand name, as Filament's `brandName`: the text at the top of the navigation
    /// and in the sign-in bar, and the end of every page title (`Users · <name>`). The default
    /// is `Ignite`, shown beside the Ignite mark. A name set here shows without the mark; with
    /// a [`brand_logo`](Self::brand_logo), the logo shows instead and the name is its
    /// alternative text.
    pub fn brand_name(mut self, name: impl Into<String>) -> Self {
        self.brand.name = Some(name.into());
        self
    }

    /// Sets the brand logo, as Filament's `brandLogo`: the path or URL of an image, such as
    /// `/images/logo.webp`, shown at the top of the navigation and in the sign-in bar in place
    /// of the brand name, with the name as its alternative text. The app serves the image. The
    /// default is the Ignite mark beside the name.
    pub fn brand_logo(mut self, url: impl Into<String>) -> Self {
        self.brand.logo = Some(url.into());
        self
    }

    /// Sets the logo shown in dark mode, as Filament's `darkModeBrandLogo`: the path or URL of
    /// an image, such as `/images/logo-dark.webp`, for a logo that does not show on the dark
    /// navigation. In dark mode it replaces the [`brand_logo`](Self::brand_logo) (or the brand
    /// name, when there is no logo), at the same height and with the name as its alternative
    /// text; in light mode the default logo shows. A theme whose `dark_mode` is off never shows
    /// it. The default is none: the one logo shows in both modes.
    pub fn dark_mode_brand_logo(mut self, url: impl Into<String>) -> Self {
        self.brand.dark_logo = Some(url.into());
        self
    }

    /// Sets the height of the [`brand_logo`](Self::brand_logo) and the
    /// [`dark_mode_brand_logo`](Self::dark_mode_brand_logo), as Filament's
    /// `brandLogoHeight`: a CSS length, such as `2rem` or `48px`. The default is `1.5rem`, as
    /// in Filament. The width follows the image.
    ///
    /// # Panics
    ///
    /// Panics if `height` is not a number followed by a unit or `%`, because the layout prints
    /// it in a `style` attribute.
    pub fn brand_logo_height(mut self, height: impl Into<String>) -> Self {
        let height = height.into();
        assert!(
            brand::valid_height(&height),
            "the brand logo height `{height}` is not a CSS length such as `2rem` or `48px`"
        );
        self.brand.logo_height = Some(height);
        self
    }

    /// Sets the favicon, as Filament's `favicon`: the path or URL of the icon, such as
    /// `/images/favicon.png`. The layout links it in place of Ignite's favicon and touch icon.
    /// The app serves the file. The default is Ignite's icons.
    pub fn favicon(mut self, url: impl Into<String>) -> Self {
        self.brand.favicon = Some(url.into());
        self
    }

    /// Adds a stylesheet to every page of the panel, as Filament registers an app's CSS with
    /// `FilamentAsset::register` and prints it at the end of the head: the path or URL of the
    /// file, such as `/mc-includes/css/dashicons.css`. The layout links it after the panel's
    /// own CSS, so its rules can change the panel's look, on the signed-in pages and on the
    /// sign-in pages. Each call adds one stylesheet, in order. The app serves the file. The
    /// default is none.
    ///
    /// The URL is printed in the `href` attribute and escaped there, as every value of the
    /// views is. These stylesheets load in every admin theme, after the theme's own; a
    /// stylesheet that only one theme needs, such as its icon font, belongs in that theme's
    /// [`AdminTheme::stylesheets`](crate::AdminTheme::stylesheets).
    ///
    /// ```ignore
    /// Ignite::new()
    ///     .stylesheet("/mc-admin/css/admin.css")
    /// ```
    pub fn stylesheet(mut self, url: impl Into<String>) -> Self {
        self.brand.stylesheets.push(url.into());
        self
    }

    /// Orders the navigation groups, as Filament's panel `navigationGroups`: the sidebar shows
    /// the groups in this order, each under its name as a heading, after the resources that
    /// have no group. A resource joins a group with its
    /// [`NAVIGATION_GROUP`](ignite_core::IgniteResource::NAVIGATION_GROUP) (or a runtime
    /// resource's `navigation_group`). A group no resource uses shows nothing, and a group that
    /// is not listed here comes after the listed ones. Each call adds to the list, as in
    /// Filament.
    ///
    /// A group is a label, or a [`NavigationGroup`] that also says whether the group is
    /// collapsible or starts collapsed, as Filament's panel takes labels or `NavigationGroup`
    /// objects.
    ///
    /// ```ignore
    /// Ignite::new().navigation_groups(["Content", "Shop", "Settings"])
    /// Ignite::new().navigation_groups([NavigationGroup::make("Settings").collapsed(true)])
    /// ```
    pub fn navigation_groups<I, S>(mut self, groups: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<NavigationGroup>,
    {
        self.navigation_groups
            .extend(groups.into_iter().map(Into::into));
        self
    }

    /// Adds navigation entries of the panel's own, as Filament's panel `navigationItems()`:
    /// each [`NavigationItem`] is listed in its group by its sort, beside the resources. An
    /// entry without a URL is a parent that is only a label: resources name it as their
    /// `navigation_parent_item` (by its label), and it opens its first child, as a Filament
    /// cluster does. Each call adds to the list.
    ///
    /// ```ignore
    /// Ignite::new().navigation_items([NavigationItem::make("Settings").group("Settings").sort(1)])
    /// ```
    pub fn navigation_items<I>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = NavigationItem>,
    {
        self.navigation_items.extend(items);
        self
    }

    /// Whether every navigation group is collapsible, as Filament's panel
    /// `collapsibleNavigationGroups()`: its heading collapses and expands its entries, and the
    /// browser remembers which groups the user collapsed. The default is `true`, as in
    /// Filament; `false` renders each group under a plain heading. A [`NavigationGroup`] that
    /// says otherwise wins.
    pub fn collapsible_navigation_groups(mut self, collapsible: bool) -> Self {
        self.collapsible_navigation_groups = collapsible;
        self
    }

    /// Adds a dashboard widget of the panel's own, as Filament's panel `widgets()`: a
    /// [`StatsOverviewWidget`] whose cards ([`Stat`](ignite_core::Stat)s) the dashboard shows
    /// in its grid, before the resources' cards, to the users its
    /// [`can_view`](StatsOverviewWidget::can_view) allows. Each call adds a widget after the
    /// others.
    pub fn widget(mut self, widget: impl StatsOverviewWidget) -> Self {
        self.widgets.push(Arc::new(widget));
        self
    }

    /// Adds a list widget to the dashboard, as a Filament custom widget with its own view: a
    /// title and rows of a label, a value and a link, such as WordPress's "Activity" and
    /// "Recently Published" boxes (see [`ListWidget`]). The list and form widgets show after
    /// the stat cards, in the order they were added, each to the users its
    /// [`can_view`](ListWidget::can_view) allows. The `ignite` theme renders it with
    /// `partials.widgets.list`, which a theme can replace.
    pub fn list_widget(mut self, widget: ListWidget) -> Self {
        self.dashboard_widgets.push(DashboardWidget::List(widget));
        self
    }

    /// Adds a form widget to the dashboard: a short form that creates a record of a resource,
    /// such as WordPress's "Quick Draft" (see [`FormWidget`]). It shows only to a user who may
    /// create a record of the resource, and not at all when no resource has its slug. The
    /// `ignite` theme renders it with `partials.widgets.form`, which a theme can replace.
    pub fn form_widget(mut self, widget: FormWidget) -> Self {
        self.dashboard_widgets.push(DashboardWidget::Form(widget));
        self
    }

    /// Sets the panel's 404 page, as Laravel's `resources/views/errors/404.blade.php`: a request
    /// under the panel's mount path that finds nothing (an unknown path, a resource or a record
    /// that does not exist) answers 404 with this page inside the panel's layout, in the admin
    /// theme of the request: the navigation and the top bar around the title, the message and a
    /// link to the dashboard for a signed-in user, the page without them for a visitor. The app
    /// sets the title, the message, the link's label and its own view with [`NotFound`]; every
    /// value is escaped. Without a call, the page says "The page you are looking for could not
    /// be found." with the link "Go to the dashboard".
    ///
    /// The panel's catcher is registered at the mount path only, so a 404 elsewhere, such as on
    /// the app's public site, keeps the app's own 404 page. An app that registers its own 404
    /// catcher at the panel's mount path collides with the panel's, and Rocket refuses to
    /// launch.
    ///
    /// ```ignore
    /// Ignite::new().not_found(NotFound::make().message("That screen does not exist."))
    /// ```
    pub fn not_found(mut self, page: NotFound) -> Self {
        self.not_found = page;
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
            database_url,
            mount,
            resources,
            profile_fields,
            secret,
            views,
            csrf_exempt,
            trusted_proxies,
            mailer,
            app_url,
            unsaved_changes_alerts,
            user_callbacks,
            storage,
            brand,
            navigation_groups,
            navigation_items,
            collapsible_navigation_groups,
            widgets,
            dashboard_widgets,
            roles,
            abilities: app_abilities,
            not_found,
            profile_extensions,
            profile_picture,
        } = self;
        check_profile_names(
            &profile_fields,
            &profile_extensions,
            profile_picture.as_ref(),
        );
        for role in &roles {
            assert!(
                crate::authz::valid_role_name(role.get_name()),
                "the role `{}` is not a valid role name: use lowercase letters, digits, `-` and `_`",
                role.get_name()
            );
        }
        crate::roles::install_labels(&roles);
        // Users created or deleted from now on run the app's callbacks.
        users::install_callbacks(user_callbacks);
        users::install_storage_dir(storage);
        // The panel registers its own assets, so an app adds nothing for its styling.
        crate::assets::register(&mount);
        register_views(views.as_deref());
        let ignite = AdHoc::try_on_ignite("Ignite", move |rocket| async move {
            let db = match connect(&database_url).await {
                Ok(db) => db,
                Err(error) => {
                    rocket::error!("Ignite could not connect to DATABASE_URL: {error}");
                    return Err(rocket);
                }
            };
            // Each resource's abilities get a permission row, so roles can be granted them.
            let mut abilities = crate::authz::resource_abilities(&resources);
            abilities.extend(app_abilities);
            crate::authz::register_at_start(&db, abilities).await;
            // The app's roles, each created once with its abilities.
            if let Err(error) = crate::authz::seed_roles(&db, &roles).await {
                rocket::error!("ignite: could not create the app's roles: {error}");
            }
            let registry = Registry {
                mount: mount.clone(),
                resources,
                profile_fields,
                secret,
                csrf_exempt,
                trusted_proxies,
                mailer,
                app_url,
                unsaved_changes_alerts,
                runtime_permissions: Mutex::new(HashSet::new()),
                brand,
                navigation_groups,
                navigation_items,
                collapsible_navigation_groups,
                widgets,
                dashboard_widgets,
                not_found,
                profile_extensions,
                profile_picture,
            };
            // The panel's 404 page answers only under its mount path.
            let catcher_base = if mount.is_empty() {
                "/".to_string()
            } else {
                mount.clone()
            };
            Ok(rocket
                .manage(db)
                .manage(registry)
                .register(
                    "/",
                    catchers![
                        session::unauthorized,
                        session::csrf_mismatch,
                        session::server_error
                    ],
                )
                .register(catcher_base, catchers![crate::not_found::not_found])
                .mount("/", routes::health())
                .mount(mount, routes::all()))
        });
        PanelFairing { ignite }
    }
}

/// Checks the names of the profile's fields: the name, the email, the app's
/// [`Ignite::profile_field`]s, its [`ProfileFields`] and its picture field must all differ, and
/// the fields the app keeps must have lowercase identifiers, as a runtime resource's fields do.
fn check_profile_names(
    stored: &[Field],
    extensions: &[ProfileFields],
    picture: Option<&ProfilePicture>,
) {
    let mut names: Vec<String> = ["name", "email"]
        .into_iter()
        .chain(stored.iter().map(|f| f.name))
        .map(str::to_string)
        .collect();
    let kept = extensions
        .iter()
        .flat_map(|group| group.get_fields())
        .chain(picture.map(ProfilePicture::get_field));
    for field in kept {
        let name = field.name.as_str();
        let valid = name.len() <= ignite_core::MAX_FIELD_NAME
            && name.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        assert!(
            valid,
            "the profile field `{name}` is not valid: use lowercase letters, digits and `_`, \
             starting with a letter"
        );
        assert!(
            !names.iter().any(|known| known == name),
            "the profile already has a field named `{name}`"
        );
        names.push(name.to_string());
    }
}

/// The pool size: 10 connections, the same number the panel used before the move to Orbit.
const POOL_CONNECTIONS: u32 = 10;

/// Connects the app's database pool with [`POOL_CONNECTIONS`] connections.
async fn connect(url: &str) -> Result<Database, orbit_orm::Error> {
    Database::connect_with(
        url,
        PoolOptions {
            max_connections: POOL_CONNECTIONS,
            ..PoolOptions::default()
        },
    )
    .await
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

/// Registers the panel's views for Propel's runtime renderer, served through the admin theme
/// of the page being rendered (see [`crate::theme`]): a directory when one is named, in a
/// debug build, or the views embedded in the binary.
#[cfg(debug_assertions)]
fn register_views(dir: Option<&Path>) {
    use crate::theme::ThemedViews;

    match dir {
        Some(dir) => {
            check_views(dir);
            propel_rocket::register_namespace_loader(
                "ignite",
                ThemedViews::new(propel_runtime::FileLoader::new(dir)),
            );
        }
        None => propel_rocket::register_namespace_loader(
            "ignite",
            ThemedViews::new(crate::embedded::EmbeddedViews),
        ),
    }
}

/// See the debug version. A release build serves the views embedded in the binary and ignores
/// a views directory, as it always has: the `ignite` theme renders the compiled views, and
/// another theme renders the embedded views through Propel's runtime renderer.
#[cfg(not(debug_assertions))]
fn register_views(_dir: Option<&Path>) {
    propel_rocket::register_namespace_loader(
        "ignite",
        crate::theme::ThemedViews::new(crate::embedded::EmbeddedViews),
    );
}

/// The views of the `ignite` theme that a views directory must hold, besides the panel's
/// `layouts/app.propel`. The panel renders through them (see [`crate::theme`]).
#[cfg(debug_assertions)]
const THEME_VIEWS: &[&str] = &[
    "themes/ignite/layouts/app.propel",
    "themes/ignite/partials/nav.propel",
    "themes/ignite/partials/topbar.propel",
    "themes/ignite/partials/flash.propel",
];

/// Checks that a views directory holds the panel's layout and the `ignite` theme's views.
/// Panics when it does not, so a directory copied before the theme system stops the app at
/// startup instead of failing on every page.
#[cfg(debug_assertions)]
fn check_views(dir: &Path) {
    let layout = dir.join("layouts/app.propel");
    assert!(
        layout.is_file(),
        "Ignite could not find its views in {}: ship the views/ignite directory, or set {VIEWS_DIR_ENV}",
        dir.display()
    );
    let missing: Vec<&str> = THEME_VIEWS
        .iter()
        .copied()
        .filter(|view| !dir.join(view).is_file())
        .collect();
    if missing.is_empty() {
        return;
    }
    // A layout that is not the panel's alias is a customised layout from before the theme
    // system: say where it moved, so the change is not lost silently.
    use propel_runtime::Loader;
    let alias = crate::embedded::EmbeddedViews
        .source("layouts.app")
        .unwrap_or_default();
    let customised = std::fs::read_to_string(&layout).is_ok_and(|source| source != alias);
    let moved = if customised {
        " The panel's layout moved to themes/ignite/layouts/app.propel: move your changes to \
         layouts/app.propel there, and the navigation, top bar and flash partials to \
         themes/ignite/partials."
    } else {
        ""
    };
    panic!(
        "Ignite could not find the ignite theme's views in {} (missing: {}): copy the panel's \
         views/ignite directory again.{moved}",
        dir.display(),
        missing.join(", ")
    );
}
