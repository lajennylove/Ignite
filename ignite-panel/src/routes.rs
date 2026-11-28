//! The admin routes. Each handler looks up the resource by its slug, runs the
//! database work through the app's Orbit database, and renders a Propel view. Every route
//! except the sign-in and the sign-out needs an [`AdminUser`].

use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use ignite_core::{
    ActingUser, ColumnDef, Database, Direction, FieldDef, FieldErrors, FormContext, FormInput,
    LatestRecords, ListQuery, Operation, RedirectAfter, ValidationError,
};
use orbit_orm::Error as OrbitError;
use propel_rocket::PropelView;
use rocket::data::{Data, ToByteUnit};
use rocket::form::Form;
use rocket::fs::TempFile;
use rocket::http::{ContentType, CookieJar, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::response::content::RawHtml;
use rocket::response::{self, Redirect, Responder, Response};
use rocket::tokio::io::AsyncReadExt;
use rocket::{get, post, routes, FromForm, Route, State};
use serde_json::Value;

use ignite_core::structure::structured_input;

use crate::authz::Action;

use crate::erased::{self, Erased};
use crate::ignite::{DashboardWidget, Registry};
use crate::profile::{self, ProfileFields, ProfilePicture};
use crate::proxy::{self, TrustedProxies};
use crate::session::{
    check_csrf, csrf_token, end_session, recaller, remember, session_id, set_flash, start_session,
    take_flash, AdminUser,
};
use crate::sessions;
use crate::throttle;
use crate::users;
use crate::views::{
    admin_theme_options, cell_text, compiled, content_width_class, editor_for, field_views,
    field_views_with, form_field_views, has_tabs, layout_columns, layout_grid_class, live_values,
    record_form, render_page, structure_views, CardView, CellView, ColumnHead, DashboardPage,
    FormChoices, FormPage, HeaderView, HiddenView, IndexPage, LatestRow, LatestTable, Layout,
    LoginPage, ProfilePage, RowActionView, RowView, StructureContext, TabView, WidgetView,
};

/// The largest form body the panel reads.
const MAX_FORM_BYTES: u64 = 64 * 1024;

/// The health check, which Laravel ships as `/up`. It needs no sign-in, and the panel
/// mounts it at the application root, not under the panel's prefix.
pub(crate) fn health() -> Vec<Route> {
    routes![up]
}

/// Answers 200 while the app is running, with a minimal page as Laravel's `/up` does.
#[get("/up")]
fn up() -> RawHtml<&'static str> {
    RawHtml(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>Application up</title>\n</head>\n<body>\nApplication up\n</body>\n</html>\n",
    )
}

/// Every route the panel mounts under its prefix.
pub(crate) fn all() -> Vec<Route> {
    let mut all = routes![
        build_asset,
        dashboard,
        login_form,
        login,
        logout,
        logout_everywhere,
        profile_form,
        save_profile,
        save_picture,
        change_password,
        upload_avatar,
        remove_avatar,
        avatar,
        set_theme,
        save_admin_theme,
        index,
        new_form,
        new_form_live,
        create,
        edit_form,
        edit_form_live,
        update,
        destroy,
        run_row_action
    ];
    all.extend(crate::password_reset::all());
    all
}

/// What a write handler returns: a redirect on success, or a page again.
pub(crate) enum Reply {
    Redirect(Box<Redirect>),
    Page(Status, PropelView),
    /// A page refused because of too many attempts, with the seconds to wait.
    Throttled(i64, PropelView),
}

impl<'r> Responder<'r, 'static> for Reply {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'static> {
        match self {
            Reply::Redirect(redirect) => (*redirect).respond_to(request),
            Reply::Page(status, view) => (status, view).respond_to(request),
            Reply::Throttled(seconds, view) => {
                let refused = (Status::TooManyRequests, view).respond_to(request)?;
                Response::build_from(refused)
                    .raw_header("Retry-After", seconds.to_string())
                    .ok()
            }
        }
    }
}

/// The dashboard: the stat cards that every resource provides.
#[get("/")]
async fn dashboard(
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<PropelView, Status> {
    let flash = take_flash(cookies, &registry.secret);
    let token = csrf_token(cookies, &registry.secret);
    dashboard_page(registry, &session, db.inner(), flash, token, None).await
}

/// A form widget whose save the resource refused: the widget, by its place among the
/// dashboard's widgets, and the values and messages it shows again.
struct RefusedWidget<'a> {
    index: usize,
    form: &'a FormInput,
    errors: &'a FieldErrors,
}

/// The dashboard page, with the form widget `refused` showing its typed values and its
/// validation messages, if any.
async fn dashboard_page(
    registry: &Registry,
    session: &AdminUser,
    db: &Database,
    flash: Option<String>,
    token: String,
    refused: Option<RefusedWidget<'_>>,
) -> Result<PropelView, Status> {
    // The dashboard shows a resource's cards and table only when the user may view its records.
    let resources: Vec<Arc<dyn Erased>> = registry
        .all()
        .into_iter()
        .filter(|r| session.may(r.slug(), Action::ViewAny))
        .collect();
    // The panel's own widgets first, as Filament's panel `widgets()`, each for the users its
    // `can_view` allows, then each resource's cards.
    let user = session.acting_user();
    let mut stats = Vec::new();
    for widget in registry.widgets.iter().filter(|w| w.can_view(user)) {
        stats.extend(widget.stats(db, user).await.map_err(db_status)?);
    }
    let mut tables = Vec::new();
    for resource in &resources {
        let database = db;
        stats.extend(resource.stats(database, user).await.map_err(db_status)?);
        if let Some(declared) = resource.latest_records() {
            let limit = declared.limit.max(1);
            let records = resource.latest(database, limit).await.map_err(db_status)?;
            let lookups = lookup_choices(
                database,
                session.acting_user(),
                resource.as_ref(),
                declared.columns,
            )
            .await
            .map_err(db_status)?;
            tables.push((declared, resource.columns(), records, lookups));
        }
    }
    let tables = tables
        .into_iter()
        .map(|(declared, columns, records, lookups)| {
            latest_table(&declared, columns, &records, &lookups)
        })
        .collect();

    let widgets = dashboard_widgets(registry, session, db, &token, refused.as_ref()).await?;
    let layout = Layout::new(registry, Some(session), "", flash, token.clone());
    let page = DashboardPage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        has_avatar: layout.has_avatar,
        avatar_url: layout.avatar_url,
        theme: layout.theme,
        theme_url: layout.theme_url,
        theme_switcher: layout.theme_switcher,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        nav_sections: layout.nav_sections,
        nav_collapsible: layout.nav_collapsible,
        has_flash: layout.has_flash,
        flash: layout.flash,
        theme_style: layout.theme_style,
        admin_theme: layout.admin_theme,
        brand: layout.brand,
        title: "Dashboard",
        cards: stats
            .iter()
            .enumerate()
            .map(|(index, stat)| CardView::new(index, stat))
            .collect(),
        has_widgets: !widgets.is_empty(),
        widgets,
        tables,
    };
    Ok(render_page(
        "ignite::dashboard",
        &page,
        compiled::dashboard::render,
    ))
}

/// The list and form widgets of the dashboard the signed-in user sees, in the order they were
/// added: a list widget its `can_view` allows, and a form widget whose resource exists and lets
/// the user create a record. The form widget `refused` shows its typed values and messages.
async fn dashboard_widgets(
    registry: &Registry,
    session: &AdminUser,
    db: &Database,
    token: &str,
    refused: Option<&RefusedWidget<'_>>,
) -> Result<Vec<WidgetView>, Status> {
    let user = session.acting_user();
    let mut views = Vec::new();
    for (index, widget) in registry.dashboard_widgets.iter().enumerate() {
        match widget {
            DashboardWidget::List(widget) => {
                if !widget.is_visible_to(user) {
                    continue;
                }
                let rows = widget.load(db, user).await.map_err(db_status)?;
                views.push(WidgetView::list(index, widget, &rows));
            }
            DashboardWidget::Form(widget) => {
                let slug = widget.get_resource();
                let Some(resource) = registry.find(slug) else {
                    continue;
                };
                if !session.may(slug, Action::Create) {
                    continue;
                }
                let names = widget.get_fields();
                let defs: Vec<FieldDef> = if names.is_empty() {
                    resource.fields().to_vec()
                } else {
                    names
                        .iter()
                        .filter_map(|name| resource.fields().iter().find(|f| &f.name == name))
                        .cloned()
                        .collect()
                };
                let shown: Vec<&str> = defs.iter().map(|f| f.name.as_str()).collect();
                let lookups = lookup_choices(db, user, resource.as_ref(), &shown)
                    .await
                    .map_err(db_status)?;
                let choices = FormChoices {
                    lookups,
                    media: HashMap::new(),
                };
                let empty = (FormInput::default(), FieldErrors::new());
                let (values, errors) = match refused {
                    Some(refused) if refused.index == index => (refused.form, refused.errors),
                    _ => (&empty.0, &empty.1),
                };
                views.push(WidgetView {
                    is_form: true,
                    action: format!("{}/{}", registry.mount, resource.slug()),
                    csrf_token: token.to_string(),
                    back_url: registry.mount.clone(),
                    widget_key: index.to_string(),
                    fields: field_views_with(&defs, values, errors, &choices),
                    hidden: widget
                        .get_hidden()
                        .iter()
                        .map(|(name, value)| HiddenView {
                            name: name.clone(),
                            value: value.clone(),
                        })
                        .collect(),
                    submit_label: widget.get_submit_label().to_string(),
                    ..WidgetView::empty(index, widget.get_title())
                });
            }
        }
    }
    Ok(views)
}

/// The place among the dashboard's widgets of the form widget of the resource `slug` whose
/// save was refused: the one the form names in `_widget`, else the resource's first form
/// widget. `None` when the dashboard has no form widget of the resource.
fn refused_widget(registry: &Registry, slug: &str, form: &FormInput) -> Option<usize> {
    let of_resource = |index: &usize| {
        matches!(
            registry.dashboard_widgets.get(*index),
            Some(DashboardWidget::Form(widget)) if widget.get_resource() == slug
        )
    };
    form.get("_widget")
        .and_then(|key| key.parse::<usize>().ok())
        .filter(of_resource)
        .or_else(|| (0..registry.dashboard_widgets.len()).find(of_resource))
}

/// Serves the panel's built stylesheet and script, under `{mount}/build`. The names are
/// hashed, so a changed file has a new name and an old copy is never stale.
// A lower priority than the resource routes, so a resource named "build" still works.
#[get("/build/<path..>", rank = 10)]
fn build_asset(path: PathBuf) -> Option<(ContentType, Vec<u8>)> {
    let name = path.to_string_lossy().replace('\\', "/");
    let bytes = crate::assets::file(&name)?;
    let content_type = path
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(ContentType::from_extension)
        .unwrap_or(ContentType::Binary);
    Some((content_type, bytes))
}

/// The sign-in page. A user who is already signed in, by the session or by the "Remember me"
/// cookie, goes to the dashboard instead, as Laravel's `guest` middleware
/// (`RedirectIfAuthenticated`) sends them to the home route.
#[get("/login")]
fn login_form(
    user: Outcome<AdminUser, ()>,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
) -> Result<Reply, Status> {
    match user {
        Outcome::Success(_) => {
            return Ok(Reply::Redirect(Box::new(Redirect::to(
                registry.mount.clone(),
            ))))
        }
        // A database outage is not a sign-out, so it answers 500 as the guard does elsewhere.
        Outcome::Error((Status::InternalServerError, ())) => {
            return Err(Status::InternalServerError)
        }
        Outcome::Error(_) | Outcome::Forward(_) => {}
    }
    let token = csrf_token(cookies, &registry.secret);
    Ok(Reply::Page(
        Status::Ok,
        render_page(
            "ignite::login",
            &login_page(registry, "", false, None, &FieldErrors::new(), token),
            compiled::login::render,
        ),
    ))
}

fn login_page(
    registry: &Registry,
    email: &str,
    remember: bool,
    error: Option<&str>,
    field_errors: &FieldErrors,
    csrf_token: String,
) -> LoginPage {
    let email_error = field_errors.get("email");
    let password_error = field_errors.get("password");
    let layout = Layout::new(registry, None, "", None, csrf_token);
    LoginPage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        has_avatar: layout.has_avatar,
        avatar_url: layout.avatar_url,
        theme: layout.theme,
        theme_url: layout.theme_url,
        theme_switcher: layout.theme_switcher,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        nav_sections: layout.nav_sections,
        nav_collapsible: layout.nav_collapsible,
        has_flash: layout.has_flash,
        flash: layout.flash,
        theme_style: layout.theme_style,
        admin_theme: layout.admin_theme,
        brand: layout.brand,
        title: "Sign in",
        action: format!("{}/login", registry.mount),
        forgot_url: format!("{}/password/forgot", registry.mount),
        email: email.to_string(),
        remember,
        has_error: error.is_some(),
        error: error.unwrap_or_default().to_string(),
        has_email_error: email_error.is_some(),
        email_error: email_error.cloned().unwrap_or_default(),
        has_password_error: password_error.is_some(),
        password_error: password_error.cloned().unwrap_or_default(),
    }
}

/// The message of an empty sign-in field, as WordPress's `wp-login.php` words it.
pub(crate) const EMPTY_EMAIL: &str = "ERROR: The email field is empty.";
/// The message of an empty password, as WordPress's `wp-login.php` words it.
pub(crate) const EMPTY_PASSWORD: &str = "ERROR: The password field is empty.";

#[post("/login", data = "<body>")]
async fn login(
    body: Data<'_>,
    client_ip: ClientIp,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Reply, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let email = form.get("email").unwrap_or("").trim().to_string();
    let password = form.get("password").unwrap_or("").to_string();
    // The "Remember me" box, read as Laravel's `$request->boolean('remember')`.
    let remember_me = matches!(form.get("remember"), Some("1" | "true" | "on" | "yes"));
    let throttle_key = throttle::key(&email, client_ip.0);

    // An empty email or password is refused before any check, as Laravel's login request
    // validates both as `required` before it attempts the sign-in (and before its rate limit):
    // each field shows WordPress's message in its error slot, the email keeps what was typed,
    // and the attempt does not count against the throttle.
    let mut empty = FieldErrors::new();
    if email.is_empty() {
        empty.insert("email".to_string(), EMPTY_EMAIL.to_string());
    }
    if password.is_empty() {
        empty.insert("password".to_string(), EMPTY_PASSWORD.to_string());
    }
    if !empty.is_empty() {
        return Ok(Reply::Page(
            Status::UnprocessableEntity,
            render_page(
                "ignite::login",
                &login_page(
                    registry,
                    form.get("email").unwrap_or(""),
                    remember_me,
                    None,
                    &empty,
                    csrf_token(cookies, &registry.secret),
                ),
                compiled::login::render,
            ),
        ));
    }

    // Refuse before checking the password while the key is at its limit.
    let waiting = throttle::retry_after(db.inner(), &throttle_key, throttle::LIMIT)
        .await
        .map_err(db_status)?;
    if let Some(seconds) = waiting {
        let message = format!("Too many sign-in attempts. Try again in {seconds} seconds.");
        let page = render_page(
            "ignite::login",
            &login_page(
                registry,
                &email,
                remember_me,
                Some(&message),
                &FieldErrors::new(),
                csrf_token(cookies, &registry.secret),
            ),
            compiled::login::render,
        );
        return Ok(Reply::Throttled(seconds, page));
    }

    let database = db.inner();
    let user = users::find_by_email(database, &email)
        .await
        .map_err(db_status)?;
    // Check a password even when there is no such user, so the response time
    // does not show which emails are registered.
    let matched = match &user {
        Some(user) => users::verify_password(&user.password_hash, &password),
        None => {
            users::verify_password(dummy_hash(), &password);
            false
        }
    };
    // A disabled user gets the same answer as a wrong password, and the attempt counts
    // against the throttle, as Laravel's Auth::attempt does for an extra condition such
    // as `active`. The message does not reveal that the account exists but is disabled.
    let signed_in = match user.filter(|user| matched && user.disabled_at.is_none()) {
        Some(user) => {
            throttle::clear(database, &throttle_key)
                .await
                .map_err(db_status)?;
            let session_id = sessions::create(database, user.id)
                .await
                .map_err(db_status)?;
            // Ticked, the user also gets the remember cookie, as Laravel's `login($user, true)`.
            let token = if remember_me {
                Some(
                    users::ensure_remember_token(database, user.id)
                        .await
                        .map_err(db_status)?,
                )
            } else {
                None
            };
            Some((session_id, user.id, token))
        }
        None => {
            throttle::record_failure(database, &throttle_key)
                .await
                .map_err(db_status)?;
            None
        }
    };

    match signed_in {
        Some((session_id, user_id, token)) => {
            start_session(cookies, &registry.secret, &session_id);
            if let Some(token) = token {
                remember(cookies, &registry.secret, user_id, &token);
            }
            Ok(Reply::Redirect(Box::new(Redirect::to(
                registry.mount.clone(),
            ))))
        }
        None => Ok(Reply::Page(
            Status::UnprocessableEntity,
            render_page(
                "ignite::login",
                &login_page(
                    registry,
                    &email,
                    remember_me,
                    Some("The email or password is incorrect."),
                    &FieldErrors::new(),
                    csrf_token(cookies, &registry.secret),
                ),
                compiled::login::render,
            ),
        )),
    }
}

/// The client's IP address, for the login throttle: the socket address, or the client
/// named by `X-Forwarded-For` when the socket is a trusted proxy (see [`proxy`]). Unknown
/// when Rocket has no socket address for the request.
///
/// This never calls `Request::client_ip`, which reads `X-Real-IP` from any client.
pub(crate) struct ClientIp(pub(crate) Option<IpAddr>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ClientIp {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, ()> {
        let socket = request.remote().map(|remote| remote.ip());
        let forwarded = request
            .headers()
            .get("X-Forwarded-For")
            .collect::<Vec<_>>()
            .join(",");
        let forwarded = (!forwarded.is_empty()).then_some(forwarded);
        let fallback = TrustedProxies::default();
        let trusted = request
            .rocket()
            .state::<Registry>()
            .map_or(&fallback, |registry| &registry.trusted_proxies);
        Outcome::Success(ClientIp(proxy::client_ip(
            socket,
            forwarded.as_deref(),
            trusted,
        )))
    }
}

/// A hash that matches no password, used to spend the same time on an unknown email.
fn dummy_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| users::hash_password("not-a-real-password").expect("argon2 hashes"))
}

#[post("/logout", data = "<body>")]
async fn logout(
    body: Data<'_>,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Redirect, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let database = db.inner();
    let session_id = session_id(cookies, &registry.secret);
    // The signed-out user: the one of the session, or of a valid remember cookie when the
    // session has ended. Their new remember token ends every "Remember me" sign-in, as
    // Laravel's `logout` does with `cycleRememberToken`.
    let mut user_id = None;
    if let Some(session_id) = &session_id {
        user_id = sessions::user_for(database, session_id)
            .await
            .ok()
            .flatten();
    }
    if user_id.is_none() {
        if let Some((id, token)) = recaller(cookies, &registry.secret) {
            if let Ok(Some(user)) = users::find_by_remember_token(database, id, &token).await {
                user_id = Some(user.id);
            }
        }
    }
    if let Some(user_id) = user_id {
        let _ = users::cycle_remember_token(database, user_id).await;
    }
    if let Some(session_id) = session_id {
        // The row goes first: a copied cookie stops working even if the cookie is still in the jar.
        let _ = sessions::delete(database, &session_id).await;
    }
    end_session(cookies);
    Ok(Redirect::to(format!("{}/login", registry.mount)))
}

/// Ends every session of the signed-in user, on every device.
#[post("/logout-everywhere", data = "<body>")]
async fn logout_everywhere(
    body: Data<'_>,
    user: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Redirect, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    sessions::delete_for_user(db.inner(), user.user_id())
        .await
        .map_err(db_status)?;
    // A new remember token, so a remembered browser does not sign in again (see `logout`).
    users::cycle_remember_token(db.inner(), user.user_id())
        .await
        .map_err(db_status)?;
    end_session(cookies);
    Ok(Redirect::to(format!("{}/login", registry.mount)))
}

/// Saves the signed-in user's colour theme. The page switches at once, so the answer has no body.
#[post("/theme", data = "<body>")]
async fn set_theme(
    body: Data<'_>,
    user: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Status, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let theme = form
        .get("theme")
        .filter(|theme| users::THEMES.contains(theme))
        .ok_or(Status::BadRequest)?
        .to_string();
    let id = user.user_id();
    users::set_theme(db.inner(), id, &theme)
        .await
        .map_err(db_status)?;
    Ok(Status::NoContent)
}

/// Saves the signed-in user's admin theme, chosen on the profile page: the name of a
/// registered theme, or empty for the site default. Only this user's requests change, and the
/// light or dark mode stays as it is. A name no theme is registered with is refused (400), as
/// the light or dark toggle refuses an unknown mode.
#[post("/profile/admin-theme", data = "<body>")]
async fn save_admin_theme(
    body: Data<'_>,
    user: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Redirect, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let choice = match form.get("admin_theme").map(str::trim) {
        None | Some("") => None,
        Some(name) if crate::theme::is_registered_admin_theme(name) => Some(name),
        Some(_) => return Err(Status::BadRequest),
    };
    users::set_admin_theme(db.inner(), user.user_id(), choice)
        .await
        .map_err(db_status)?;
    set_flash(cookies, &registry.secret, "Admin theme saved.");
    Ok(Redirect::to(format!("{}/profile", registry.mount)))
}

/// Renders the profile page: the profile form, the password form and the picture. The
/// forms show `values` and their errors, so a rejected form keeps what was typed.
fn render_profile(
    registry: &Registry,
    session: &AdminUser,
    cookies: &CookieJar<'_>,
    values: &FormInput,
    errors: &FieldErrors,
    password_errors: &FieldErrors,
    choices: &FormChoices,
) -> PropelView {
    let flash = take_flash(cookies, &registry.secret);
    let token = csrf_token(cookies, &registry.secret);
    let layout = Layout::new(registry, Some(session), "profile", flash, token);
    // The line about the light and dark switch is the page theme's to word, or to leave out.
    let note = crate::theme::entry_or_default(&layout.admin_theme.name)
        .theme
        .theme_switcher_note();
    let fields: Vec<FieldDef> = profile::profile_fields(registry)
        .into_iter()
        .map(FieldDef::from)
        .collect();
    // The fields the app keeps come after the name, the email and the stored fields, with the
    // same markup.
    let mut profile_fields = field_views(&fields, values, errors);
    for group in &registry.profile_extensions {
        profile_fields.extend(field_views_with(
            group.get_fields(),
            values,
            errors,
            choices,
        ));
    }
    let picture_fields = match &registry.profile_picture {
        Some(picture) => field_views_with(picture.fields().get_fields(), values, errors, choices),
        None => Vec::new(),
    };
    let password_fields: Vec<FieldDef> = profile::PASSWORD_FIELDS
        .into_iter()
        .map(FieldDef::from)
        .collect();
    let page = ProfilePage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        has_avatar: layout.has_avatar,
        avatar_url: layout.avatar_url,
        theme: layout.theme,
        theme_url: layout.theme_url,
        theme_switcher: layout.theme_switcher,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        nav_sections: layout.nav_sections,
        nav_collapsible: layout.nav_collapsible,
        has_flash: layout.has_flash,
        flash: layout.flash,
        theme_style: layout.theme_style,
        admin_theme: layout.admin_theme,
        brand: layout.brand,
        title: "Profile",
        profile_action: format!("{}/profile", registry.mount),
        profile_fields,
        password_action: format!("{}/profile/password", registry.mount),
        password_fields: field_views(&password_fields, &FormInput::default(), password_errors),
        avatar_action: format!("{}/profile/avatar", registry.mount),
        remove_avatar_action: format!("{}/profile/avatar/remove", registry.mount),
        has_admin_themes: crate::theme::registered_admin_themes().len() > 1,
        admin_theme_action: format!("{}/profile/admin-theme", registry.mount),
        admin_themes: admin_theme_options(session.admin_theme_choice()),
        has_theme_switcher_note: note.is_some(),
        theme_switcher_note: note.unwrap_or_default(),
        has_picture_field: registry.profile_picture.is_some(),
        picture_action: format!("{}/profile/picture", registry.mount),
        picture_fields,
    };
    render_page("ignite::profile", &page, compiled::profile::render)
}

/// The signed-in user's row, as the profile needs it.
async fn load_user(db: &State<Database>, id: i64) -> Result<users::User, Status> {
    users::find_by_id(db.inner(), id)
        .await
        .map_err(db_status)?
        .ok_or(Status::NotFound)
}

/// The groups of profile fields whose values the app keeps: each [`ProfileFields`], then the
/// picture field.
fn kept_profile_fields(registry: &Registry) -> impl Iterator<Item = &ProfileFields> {
    registry.profile_extensions.iter().chain(
        registry
            .profile_picture
            .as_ref()
            .map(ProfilePicture::fields),
    )
}

/// The values the profile's forms start with: the name, the email and the stored fields, then
/// the values the app loads for its own fields.
async fn profile_values(
    registry: &Registry,
    db: &Database,
    session: &AdminUser,
    row: &users::User,
) -> Result<HashMap<String, String>, Status> {
    let mut values = profile::form_values(
        &profile::profile_fields(registry),
        &row.name,
        &row.email,
        row.profile.as_deref(),
    );
    for group in kept_profile_fields(registry) {
        let loaded = group
            .load_values(db, session.acting_user())
            .await
            .map_err(db_status)?;
        values.extend(profile::row_values(group.get_fields(), &loaded));
    }
    Ok(values)
}

/// The choices of the app's profile fields that load their options, and the media library's
/// items for the ids its media fields hold in `form`.
async fn profile_choices(
    registry: &Registry,
    db: &Database,
    session: &AdminUser,
    form: &FormInput,
) -> Result<FormChoices, Status> {
    let user = session.acting_user();
    let mut choices = FormChoices::default();
    let mut kept: Vec<FieldDef> = Vec::new();
    for group in kept_profile_fields(registry) {
        for field in group.get_fields() {
            if field.kind.loads_options() {
                let options = group
                    .options_of(db, user, &field.name)
                    .await
                    .map_err(db_status)?;
                choices.lookups.insert(field.name.clone(), options);
            }
        }
        kept.extend(group.get_fields().iter().cloned());
    }
    if let Some(library) = ignite_core::media_library() {
        let ids = erased::media_ids(&kept, form);
        if !ids.is_empty() {
            let items = library.find(db, &ids).await.map_err(db_status)?;
            choices
                .media
                .extend(items.into_iter().map(|item| (item.id, item)));
        }
    }
    Ok(choices)
}

/// Checks the app's fields of `group` in `form`, as a runtime resource's fields are checked
/// (a lookup's value against its choices, a media id against the media library). Gives the
/// validated values when these fields pass, and adds their messages to `errors` otherwise.
async fn check_kept_fields(
    group: &ProfileFields,
    db: &Database,
    session: &AdminUser,
    form: &FormInput,
    errors: &mut FieldErrors,
) -> Result<Option<ignite_core::Row>, Status> {
    let user = session.acting_user();
    let validated = ignite_core::validate_fields_for_update(group.get_fields(), form);
    let mut found = match &validated {
        Ok(_) => FieldErrors::new(),
        Err(messages) => messages.clone(),
    };
    found = erased::lookup_errors(group.get_fields(), form, found, |field, _| async move {
        group.options_of(db, user, &field).await
    })
    .await
    .map_err(db_status)?;
    found = erased::media_errors(db, group.get_fields(), form, found)
        .await
        .map_err(db_status)?;
    if found.is_empty() {
        return Ok(validated.ok());
    }
    for (field, message) in found {
        errors.entry(field).or_insert(message);
    }
    Ok(None)
}

/// The profile page, linked from the top bar.
#[get("/profile")]
async fn profile_form(
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<PropelView, Status> {
    let row = load_user(db, session.user_id()).await?;
    let values = FormInput::new(profile_values(registry, db, &session, &row).await?);
    let choices = profile_choices(registry, db, &session, &values).await?;
    Ok(render_profile(
        registry,
        &session,
        cookies,
        &values,
        &FieldErrors::new(),
        &FieldErrors::new(),
        &choices,
    ))
}

/// Saves the name, the email and the app's profile fields. The app's fields it keeps itself
/// ([`ProfileFields`]) are checked with the others, and saved through its callbacks before the
/// name and the email, so a refusal from the app shows on the form with nothing saved.
#[post("/profile", data = "<body>")]
async fn save_profile(
    body: Data<'_>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Reply, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let fields = profile::profile_fields(registry);
    let row = load_user(db, session.user_id()).await?;
    let database = db.inner();
    let validated = profile::validate(&fields, &form, row.profile.as_deref());
    let mut errors = match &validated {
        Ok(_) => FieldErrors::new(),
        Err(errors) => errors.clone(),
    };
    let mut kept = Vec::new();
    for group in &registry.profile_extensions {
        if let Some(values) =
            check_kept_fields(group, database, &session, &form, &mut errors).await?
        {
            kept.push((group, values));
        }
    }
    // The form again with 422, each message under its field and the typed values kept.
    let (session, form) = (&session, &form);
    let refused = |errors: FieldErrors| async move {
        let choices = profile_choices(registry, database, session, form).await?;
        Ok::<Reply, Status>(Reply::Page(
            Status::UnprocessableEntity,
            render_profile(
                registry,
                session,
                cookies,
                form,
                &errors,
                &FieldErrors::new(),
                &choices,
            ),
        ))
    };
    let saved = match validated {
        Ok(saved) if errors.is_empty() => saved,
        _ => return refused(errors).await,
    };

    let id = session.user_id();
    let taken = users::email_taken(database, &saved.email, id)
        .await
        .map_err(db_status)?;
    let email_taken = || {
        let mut errors = FieldErrors::new();
        errors.insert(
            "email".to_string(),
            "Another user already has this email.".to_string(),
        );
        errors
    };
    if taken {
        return refused(email_taken()).await;
    }
    // The app's own fields, which it may refuse with field messages.
    for (group, values) in kept {
        if let Err(refusal) = group
            .save_values(database, session.acting_user(), values)
            .await
            .map_err(db_status)?
        {
            return refused(refusal.into_errors()).await;
        }
    }
    // Two saves can both pass the check above. The unique index then refuses the
    // second one, and that refusal is the same field error as a taken email.
    match users::update_profile(database, id, &saved.name, &saved.email, &saved.custom).await {
        Ok(_) => {}
        Err(error) if is_email_index_violation(&error) => return refused(email_taken()).await,
        Err(error) => return Err(db_status(error)),
    }

    set_flash(cookies, &registry.secret, "Profile saved.");
    Ok(Reply::Redirect(Box::new(Redirect::to(format!(
        "{}/profile",
        registry.mount
    )))))
}

/// Saves the app's picture field ([`ProfilePicture`]), which replaces the upload. A refused
/// value shows under the field with 422; a saved one says "Picture saved.". Without a picture
/// field, the path is not found.
#[post("/profile/picture", data = "<body>")]
async fn save_picture(
    body: Data<'_>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Reply, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let picture = registry.profile_picture.as_ref().ok_or(Status::NotFound)?;
    let database = db.inner();
    let mut errors = FieldErrors::new();
    let checked =
        check_kept_fields(picture.fields(), database, &session, &form, &mut errors).await?;
    let refusal = match checked {
        Some(values) => picture
            .fields()
            .save_values(database, session.acting_user(), values)
            .await
            .map_err(db_status)?
            .err()
            .map(ValidationError::into_errors),
        None => Some(errors),
    };
    if let Some(errors) = refusal {
        // The other forms show their stored values, and the picture field what was typed.
        let row = load_user(db, session.user_id()).await?;
        let mut values = profile_values(registry, database, &session, &row).await?;
        let name = &picture.get_field().name;
        match form.get(name) {
            Some(typed) => values.insert(name.clone(), typed.to_string()),
            None => values.remove(name),
        };
        let values = FormInput::new(values);
        let choices = profile_choices(registry, database, &session, &values).await?;
        return Ok(Reply::Page(
            Status::UnprocessableEntity,
            render_profile(
                registry,
                &session,
                cookies,
                &values,
                &errors,
                &FieldErrors::new(),
                &choices,
            ),
        ));
    }
    set_flash(cookies, &registry.secret, "Picture saved.");
    Ok(Reply::Redirect(Box::new(Redirect::to(format!(
        "{}/profile",
        registry.mount
    )))))
}

/// Changes the password. The current password must match, and every other session of
/// the user ends, so an old sign-in cannot keep working. A wrong current password counts
/// against its own key (the email and address, as sign-in, with the same limit), so a live
/// session cannot guess the password without limit. A successful change clears only that
/// key, never the sign-in count.
#[post("/profile/password", data = "<body>")]
async fn change_password(
    body: Data<'_>,
    session: AdminUser,
    client_ip: ClientIp,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Reply, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let row = load_user(db, session.user_id()).await?;
    let key = throttle::password_key(&row.email, client_ip.0);

    // Refuse before the current password is checked while the key is at its limit.
    let waiting = throttle::retry_after(db.inner(), &key, throttle::LIMIT)
        .await
        .map_err(db_status)?;
    if let Some(seconds) = waiting {
        let values = FormInput::new(profile_values(registry, db, &session, &row).await?);
        let choices = profile_choices(registry, db, &session, &values).await?;
        let mut errors = FieldErrors::new();
        errors.insert(
            "current_password".to_string(),
            format!("Too many attempts. Try again in {seconds} seconds."),
        );
        let page = render_profile(
            registry,
            &session,
            cookies,
            &values,
            &FieldErrors::new(),
            &errors,
            &choices,
        );
        return Ok(Reply::Throttled(seconds, page));
    }

    let current = form.get("current_password").unwrap_or("");
    let new = form.get("new_password").unwrap_or("").to_string();
    let confirm = form.get("confirm_password").unwrap_or("");
    let current_ok = users::verify_password(&row.password_hash, current);
    if !current_ok {
        throttle::record_failure(db.inner(), &key)
            .await
            .map_err(db_status)?;
    }
    let errors = profile::password_errors(current_ok, &new, confirm);
    if !errors.is_empty() {
        let values = FormInput::new(profile_values(registry, db, &session, &row).await?);
        let choices = profile_choices(registry, db, &session, &values).await?;
        return Ok(Reply::Page(
            Status::UnprocessableEntity,
            render_profile(
                registry,
                &session,
                cookies,
                &values,
                &FieldErrors::new(),
                &errors,
                &choices,
            ),
        ));
    }

    let id = session.user_id();
    let keep = session_id(cookies, &registry.secret).unwrap_or_default();
    let database = db.inner();
    users::update_password(database, id, &new)
        .await
        .map_err(db_status)?;
    sessions::delete_others_for_user(database, id, &keep)
        .await
        .map_err(db_status)?;
    throttle::clear(database, &key).await.map_err(db_status)?;
    set_flash(
        cookies,
        &registry.secret,
        "Password changed. Other devices were signed out.",
    );
    Ok(Reply::Redirect(Box::new(Redirect::to(format!(
        "{}/profile",
        registry.mount
    )))))
}

/// A picture sent from the profile form. The token travels in the same form.
#[derive(FromForm)]
struct AvatarUpload<'r> {
    #[field(name = "_token")]
    token: String,
    avatar: TempFile<'r>,
}

/// Stores a profile picture. Only PNG, JPEG and WebP pictures of up to 1 MB are kept,
/// and the type comes from the file's bytes, not from its name.
#[post("/profile/avatar", data = "<upload>")]
async fn upload_avatar(
    upload: Form<AvatarUpload<'_>>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Redirect, Status> {
    let upload = upload.into_inner();
    check_csrf(cookies, &registry.secret, Some(upload.token.as_str()))?;
    let mut bytes = Vec::new();
    upload
        .avatar
        .open()
        .await
        .map_err(|_| Status::BadRequest)?
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| Status::BadRequest)?;

    let profile_url = format!("{}/profile", registry.mount);
    let Some(mime) =
        profile::picture_type(&bytes).filter(|_| bytes.len() <= profile::MAX_AVATAR_BYTES)
    else {
        set_flash(
            cookies,
            &registry.secret,
            "Choose a PNG, JPEG or WebP picture of up to 1 MB.",
        );
        return Ok(Redirect::to(profile_url));
    };
    let id = session.user_id();
    users::set_avatar(db.inner(), id, mime, &bytes)
        .await
        .map_err(db_status)?;
    set_flash(cookies, &registry.secret, "Picture saved.");
    Ok(Redirect::to(profile_url))
}

/// Removes the profile picture. The initials show again.
#[post("/profile/avatar/remove", data = "<body>")]
async fn remove_avatar(
    body: Data<'_>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Redirect, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let id = session.user_id();
    users::remove_avatar(db.inner(), id)
        .await
        .map_err(db_status)?;
    set_flash(cookies, &registry.secret, "Picture removed.");
    Ok(Redirect::to(format!("{}/profile", registry.mount)))
}

/// The signed-in user's picture, read from its file in the storage directory and served with
/// the type on the user (`users.avatar_mime`). A missing file or type, or a stored path that
/// leaves the storage directory, is 404.
#[get("/profile/avatar")]
async fn avatar(
    session: AdminUser,
    db: &State<Database>,
) -> Result<(ContentType, Vec<u8>), Status> {
    let id = session.user_id();
    let (mime, bytes) = users::avatar(db.inner(), id)
        .await
        .map_err(db_status)?
        .ok_or(Status::NotFound)?;
    let content_type = ContentType::parse_flexible(&mime).unwrap_or(ContentType::Binary);
    Ok((content_type, bytes))
}

// Rocket takes each query parameter as an argument, so the list has many.
#[allow(clippy::too_many_arguments)]
#[get("/<slug>?<q>&<tab>&<sort>&<dir>&<page>")]
async fn index(
    slug: &str,
    q: Option<String>,
    tab: Option<String>,
    sort: Option<String>,
    dir: Option<String>,
    page: Option<String>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<PropelView, Status> {
    authorize(registry, &session, slug, Action::ViewAny)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let columns = resource.columns();
    let index_url = format!("{}/{}", registry.mount, resource.slug());
    let flash = take_flash(cookies, &registry.secret);
    let token = csrf_token(cookies, &registry.secret);

    let search = q.unwrap_or_default().trim().to_string();
    // Only a sortable column can be sorted by; anything else is ignored.
    let sort = sort.filter(|key| columns.iter().any(|c| c.sortable && c.key == key.as_str()));
    let direction = if dir.as_deref() == Some("desc") {
        Direction::Desc
    } else {
        Direction::Asc
    };
    let page_no = page.and_then(|p| p.parse::<i64>().ok()).unwrap_or(1).max(1);
    // A resource that declares 0 rows per page would divide by zero below, so it shows 1.
    let per_page = resource.per_page().max(1);
    // The tab, as Filament's `?tab=`: the requested one, or the default. A resource without
    // tabs ignores the parameter, so its list and its links are as before.
    let tabs = resource
        .tabs_for(db.inner(), session.acting_user())
        .await
        .map_err(db_status)?;
    let tabs = tabs.as_slice();
    let requested_tab = tab
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty() && !tabs.is_empty());
    let active = ignite_core::active_tab(tabs, requested_tab, resource.default_active_tab());
    let tab_param = requested_tab.or(active.map(|t| t.key.as_str()));
    let query = ListQuery {
        page: page_no,
        per_page,
        search: (!search.is_empty()).then(|| search.clone()),
        sort: sort.clone().map(|key| (key, direction)),
        filter: active.map(|t| t.filter.clone()).unwrap_or_default(),
    };

    let user = session.acting_user();
    let (records, total) = resource
        .list(db.inner(), user, &query)
        .await
        .map_err(db_status)?;
    // The default tab, which always shows: the declared default when it is a tab, or the
    // first, as `active_tab` picks it without a `tab` parameter.
    let default_tab = ignite_core::active_tab(tabs, None, resource.default_active_tab());
    let tab_views = tab_views(
        resource.as_ref(),
        tabs,
        db.inner(),
        user,
        &query,
        active.map(|t| t.key.as_str()),
        default_tab.map(|t| t.key.as_str()),
        total,
        &index_url,
    )
    .await
    .map_err(db_status)?;
    let badges = resource.column_badges();
    // The columns the table shows: a badge shown inside another cell has no column of its
    // own, and a badge column not shown on the active tab is left out, heading and all.
    let active_key = active.map(|t| t.key.as_str());
    let shown: Vec<&ColumnDef> = columns
        .iter()
        .filter(|c| badges.inside_of(&c.key).is_none() && badges.shows_on(&c.key, active_key))
        .collect();
    // The badges each shown column's cell holds after its text, on the active tab.
    let inline: Vec<Vec<&str>> = shown
        .iter()
        .map(|c| {
            badges
                .inside(&c.key)
                .into_iter()
                .filter(|badge| badges.shows_on(badge, active_key))
                .collect()
        })
        .collect();
    let keys: Vec<&str> = columns
        .iter()
        .map(|c| c.key.as_str())
        .chain(inline.iter().flatten().copied())
        .collect();
    let lookups = lookup_choices(db.inner(), user, resource.as_ref(), &keys)
        .await
        .map_err(db_status)?;

    // The Edit and Delete buttons of each record need the ability and the policy's answer for
    // that record, as Filament shows a record's actions only when its policy allows them.
    let can_update = session.may(slug, Action::Update);
    let can_delete = session.may(slug, Action::Delete);
    let mut rows = Vec::with_capacity(records.len());
    for record in &records {
        let key = record_key(resource.as_ref(), record)?;
        let record_can_update =
            can_update && record_allowed(db, slug, &session, Action::Update, record).await?;
        let record_can_delete =
            can_delete && record_allowed(db, slug, &session, Action::Delete, record).await?;
        let default_delete_hidden = default_delete_hidden(resource.as_ref(), record);
        let read_only = record_read_only(resource.as_ref(), record, &session);
        let actions = row_action_views(
            resource.as_ref(),
            record,
            &key,
            &index_url,
            record_can_update,
            record_can_delete,
        );
        rows.push(RowView {
            cells: shown
                .iter()
                .zip(&inline)
                .map(|(c, inline)| {
                    // A badge shows the cell's value; an empty cell has no badge, as in
                    // Filament. Its colour is picked from the stored value.
                    let badge = |key: &str| {
                        let text = choice_text(record.get(key), lookups.get(key));
                        let color = badges.color(key, &cell_text(record.get(key)));
                        (text, color)
                    };
                    let cell = match badge(&c.key) {
                        (text, Some(color)) if !text.is_empty() => CellView::badge(text, color),
                        (text, _) => CellView::plain(text),
                    };
                    cell.with_badges(inline.iter().filter_map(|key| match badge(key) {
                        (text, Some(color)) if !text.is_empty() => Some((text, color)),
                        _ => None,
                    }))
                })
                .collect(),
            edit_url: format!("{index_url}/{key}/edit"),
            edit_label: if read_only { "View" } else { "Edit" },
            delete_url: format!("{index_url}/{key}/delete"),
            can_update: record_can_update,
            can_delete: record_can_delete && !default_delete_hidden,
            has_actions: !actions.is_empty(),
            actions,
        });
    }

    let heads = shown
        .iter()
        .map(|c| {
            let active = sort.as_deref() == Some(c.key.as_str());
            let (indicator, next) = match (active, direction) {
                (true, Direction::Asc) => ("↑", "desc"),
                (true, Direction::Desc) => ("↓", "asc"),
                (false, _) => ("", "asc"),
            };
            ColumnHead {
                label: c.label.clone(),
                sortable: c.sortable,
                sort_url: list_url(
                    &index_url,
                    &search,
                    tab_param,
                    Some((c.key.as_str(), next)),
                    1,
                ),
                indicator,
            }
        })
        .collect();

    let last_page = ((total + per_page - 1) / per_page).max(1);
    let dir_name = match direction {
        Direction::Asc => "asc",
        Direction::Desc => "desc",
    };
    let sort_pair = sort.as_deref().map(|key| (key, dir_name));
    let layout = Layout::new(registry, Some(&session), slug, flash, token.clone());
    let page_view = IndexPage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        has_avatar: layout.has_avatar,
        avatar_url: layout.avatar_url,
        theme: layout.theme,
        theme_url: layout.theme_url,
        theme_switcher: layout.theme_switcher,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        nav_sections: layout.nav_sections,
        nav_collapsible: layout.nav_collapsible,
        has_flash: layout.has_flash,
        flash: layout.flash,
        theme_style: layout.theme_style,
        admin_theme: layout.admin_theme,
        brand: layout.brand,
        title: resource.title().to_string(),
        singular: resource.singular().to_string(),
        new_url: format!("{index_url}/new"),
        can_create: session.may(slug, Action::Create),
        can_update,
        can_delete,
        index_url: index_url.clone(),
        search: search.clone(),
        columns: heads,
        colspan: shown.len() + 1,
        rows,
        total,
        page: page_no,
        last_page,
        has_prev: page_no > 1,
        prev_url: list_url(&index_url, &search, tab_param, sort_pair, page_no - 1),
        has_next: page_no < last_page,
        next_url: list_url(&index_url, &search, tab_param, sort_pair, page_no + 1),
        has_tabs: !tab_views.is_empty(),
        tabs: tab_views,
        has_tab_param: tab_param.is_some(),
        tab_param: tab_param.unwrap_or_default().to_string(),
        has_list_styles: !tabs.is_empty() || !badges.is_empty(),
        return_url: list_url(&index_url, &search, tab_param, sort_pair, page_no),
    };
    Ok(render_page(
        "ignite::index",
        &page_view,
        compiled::index::render,
    ))
}

#[get("/<slug>/new")]
async fn new_form(
    slug: &str,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<PropelView, Status> {
    authorize(registry, &session, slug, Action::Create)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let form = FormInput::default();
    let hidden = hidden_fields(&resource, Operation::Create, None, &session, &form);
    let choices = form_choices(db, &session, &resource, &form).await?;
    let layout = Layout::new(
        registry,
        Some(&session),
        slug,
        take_flash(cookies, &registry.secret),
        csrf_token(cookies, &registry.secret),
    );
    Ok(form_view(
        registry,
        layout,
        &resource,
        FormTarget::create(registry, &resource).with_disabled(disabled_fields(
            &resource,
            Operation::Create,
            None,
            &session,
            &form,
        )),
        &form,
        &FieldErrors::new(),
        &choices,
        &hidden,
    ))
}

/// The create form again, for the values a live field changed: the fields the conditions
/// show for them, with the values sent. The panel's script calls it (see
/// [`FieldConditions::live`](ignite_core::FieldConditions::live)); nothing is saved.
#[post("/<slug>/new/live", data = "<body>")]
async fn new_form_live(
    slug: &str,
    body: Data<'_>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<PropelView, Status> {
    authorize(registry, &session, slug, Action::Create)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let form = live_values(resource.fields(), None, &operated(&resource, &form));
    let hidden = hidden_fields(&resource, Operation::Create, None, &session, &form);
    let choices = form_choices(db, &session, &resource, &form).await?;
    let layout = Layout::new(
        registry,
        Some(&session),
        slug,
        None,
        csrf_token(cookies, &registry.secret),
    );
    Ok(form_view(
        registry,
        layout,
        &resource,
        FormTarget::create(registry, &resource).with_disabled(disabled_fields(
            &resource,
            Operation::Create,
            None,
            &session,
            &form,
        )),
        &form,
        &FieldErrors::new(),
        &choices,
        &hidden,
    ))
}

#[post("/<slug>", data = "<body>")]
async fn create(
    slug: &str,
    body: Data<'_>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Reply, Status> {
    authorize(registry, &session, slug, Action::Create)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let form = structured_input(resource.fields(), &form);
    let hidden = hidden_fields(&resource, Operation::Create, None, &session, &form);
    // A disabled field is not saved, as in Filament: it is left out as a hidden one is.
    let skipped = skipped_fields(&hidden, &resource, Operation::Create, None, &session, &form);

    let from_dashboard = form.get("_back") == Some(registry.mount.as_str());
    let outcome = resource
        .create(db.inner(), session.acting_user(), &form, &skipped)
        .await
        .map_err(db_status)?;

    match outcome {
        Ok(record) => {
            set_flash(cookies, &registry.secret, "Record created.");
            // A dashboard form widget comes back to the dashboard; nothing else is followed.
            // Otherwise the resource's `redirect_after_create`: the new record's edit page, as
            // Filament's default, for a user who may edit it, or the list.
            let target = if from_dashboard {
                registry.mount.clone()
            } else if resource.redirect_after_create() == RedirectAfter::Edit
                && session.may(resource.slug(), Action::Update)
                && record_allowed(db, resource.slug(), &session, Action::Update, &record).await?
            {
                // A data source whose create gives back no key leaves the list as the target.
                match saved_key(resource.as_ref(), &record) {
                    Some(key) => format!("{index_url}/{key}/edit"),
                    None => index_url,
                }
            } else {
                index_url
            };
            Ok(Reply::Redirect(Box::new(Redirect::to(target))))
        }
        Err(errors) => {
            let form = live_values(resource.fields(), None, &form);
            // A dashboard form widget shows its messages on the dashboard, under its fields,
            // with the values typed, as a Filament widget's form shows its validation errors.
            if from_dashboard {
                if let Some(index) = refused_widget(registry, resource.slug(), &form) {
                    let page = dashboard_page(
                        registry,
                        &session,
                        db.inner(),
                        None,
                        csrf_token(cookies, &registry.secret),
                        Some(RefusedWidget {
                            index,
                            form: &form,
                            errors: &errors,
                        }),
                    )
                    .await?;
                    return Ok(Reply::Page(Status::UnprocessableEntity, page));
                }
            }
            let choices = form_choices(db, &session, &resource, &form).await?;
            let layout = Layout::new(
                registry,
                Some(&session),
                slug,
                None,
                csrf_token(cookies, &registry.secret),
            );
            Ok(Reply::Page(
                Status::UnprocessableEntity,
                form_view(
                    registry,
                    layout,
                    &resource,
                    FormTarget::create(registry, &resource).with_disabled(disabled_fields(
                        &resource,
                        Operation::Create,
                        None,
                        &session,
                        &form,
                    )),
                    &form,
                    &errors,
                    &choices,
                    &hidden,
                ),
            ))
        }
    }
}

#[get("/<slug>/<id>/edit")]
async fn edit_form(
    slug: &str,
    id: &str,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<PropelView, Status> {
    authorize(registry, &session, slug, Action::Update)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;

    let record = authorize_record(db, &session, &resource, id, Action::Update).await?;
    let form = record_form(resource.fields(), &record);
    let hidden = hidden_fields(&resource, Operation::Edit, Some(&record), &session, &form);
    let choices = form_choices(db, &session, &resource, &form).await?;
    let layout = Layout::new(
        registry,
        Some(&session),
        slug,
        take_flash(cookies, &registry.secret),
        csrf_token(cookies, &registry.secret),
    );
    let context = FormContext::new(
        Operation::Edit,
        record.as_object(),
        session.acting_user(),
        &form,
    );
    let target = if resource.is_read_only(&context) {
        FormTarget::view(registry, &resource, id, resource.read_only_notice(&context))
    } else {
        FormTarget::edit(registry, &resource, id)
    };
    Ok(form_view(
        registry,
        layout,
        &resource,
        target
            .with_disabled(disabled_fields(
                &resource,
                Operation::Edit,
                Some(&record),
                &session,
                &form,
            ))
            .with_slug_locks(slug_locks(&resource, &record, &session, &form)),
        &form,
        &FieldErrors::new(),
        &choices,
        &hidden,
    ))
}

/// The edit form again, for the values a live field changed, as [`new_form_live`]. A field
/// the change reveals shows its stored value.
#[post("/<slug>/<id>/edit/live", data = "<body>")]
#[allow(clippy::too_many_arguments)]
async fn edit_form_live(
    slug: &str,
    id: &str,
    body: Data<'_>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<PropelView, Status> {
    authorize(registry, &session, slug, Action::Update)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let record = authorize_record(db, &session, &resource, id, Action::Update).await?;
    refuse_read_only(resource.as_ref(), &record, &session)?;
    let form = live_values(
        resource.fields(),
        Some(&record),
        &operated(&resource, &form),
    );
    let hidden = hidden_fields(&resource, Operation::Edit, Some(&record), &session, &form);
    let choices = form_choices(db, &session, &resource, &form).await?;
    let layout = Layout::new(
        registry,
        Some(&session),
        slug,
        None,
        csrf_token(cookies, &registry.secret),
    );
    Ok(form_view(
        registry,
        layout,
        &resource,
        FormTarget::edit(registry, &resource, id)
            .with_disabled(disabled_fields(
                &resource,
                Operation::Edit,
                Some(&record),
                &session,
                &form,
            ))
            .with_slug_locks(slug_locks(&resource, &record, &session, &form)),
        &form,
        &FieldErrors::new(),
        &choices,
        &hidden,
    ))
}

#[post("/<slug>/<id>", data = "<body>")]
async fn update(
    slug: &str,
    id: &str,
    body: Data<'_>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Reply, Status> {
    authorize(registry, &session, slug, Action::Update)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let form = structured_input(resource.fields(), &form);
    let record = authorize_record(db, &session, &resource, id, Action::Update).await?;
    refuse_read_only(resource.as_ref(), &record, &session)?;
    let hidden = hidden_fields(&resource, Operation::Edit, Some(&record), &session, &form);
    let skipped = skipped_fields(
        &hidden,
        &resource,
        Operation::Edit,
        Some(&record),
        &session,
        &form,
    );

    let outcome = resource
        .update(db.inner(), session.acting_user(), id, &form, &skipped)
        .await
        .map_err(db_status)?;

    match outcome {
        Ok(saved) => {
            set_flash(cookies, &registry.secret, "Changes saved.");
            // The resource's `redirect_after_update`: the same edit page, as Filament's default,
            // at the key the save left (a route key the form changed moves it), or the list.
            let target = match resource.redirect_after_update() {
                RedirectAfter::Edit => {
                    let key = match saved_key(resource.as_ref(), &saved) {
                        Some(key) => key,
                        None => record_key(resource.as_ref(), &record)?,
                    };
                    format!("{index_url}/{key}/edit")
                }
                RedirectAfter::List => index_url,
            };
            Ok(Reply::Redirect(Box::new(Redirect::to(target))))
        }
        Err(errors) => {
            let form = live_values(resource.fields(), Some(&record), &form);
            let choices = form_choices(db, &session, &resource, &form).await?;
            let layout = Layout::new(
                registry,
                Some(&session),
                slug,
                None,
                csrf_token(cookies, &registry.secret),
            );
            Ok(Reply::Page(
                Status::UnprocessableEntity,
                form_view(
                    registry,
                    layout,
                    &resource,
                    FormTarget::edit(registry, &resource, id)
                        .with_disabled(disabled_fields(
                            &resource,
                            Operation::Edit,
                            Some(&record),
                            &session,
                            &form,
                        ))
                        .with_slug_locks(slug_locks(&resource, &record, &session, &form)),
                    &form,
                    &errors,
                    &choices,
                    &hidden,
                ),
            ))
        }
    }
}

#[post("/<slug>/<id>/delete", data = "<body>")]
async fn destroy(
    slug: &str,
    id: &str,
    body: Data<'_>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Redirect, Status> {
    authorize(registry, &session, slug, Action::Delete)?;
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());

    let record = authorize_record(db, &session, &resource, id, Action::Delete).await?;
    // A record whose Delete button the resource hides cannot be deleted this way either.
    if default_delete_hidden(resource.as_ref(), &record) {
        return Err(Status::Forbidden);
    }
    let deleted = resource
        .delete(db.inner(), session.acting_user(), id)
        .await
        .map_err(db_status)?;
    if deleted == 0 {
        return Err(Status::NotFound);
    }
    set_flash(cookies, &registry.secret, "Record deleted.");
    Ok(Redirect::to(index_url))
}

/// Runs the row action `name` of the resource at `slug` on the record `id`, as a Filament
/// table action's `action()`: the action's form posts here with the CSRF token. The user needs
/// the action's ability on the resource (update by default, or delete) and the policy's
/// answer for the record, as the Edit and Delete buttons do, and the action must be visible on
/// the record: 403 otherwise. An unknown action, or one that is a link, is 404. The handler's
/// refusal shows as the flash message, and so does the action's success message; the list
/// shows again at the address the form sent back (`_back`), or at its first page.
#[post("/<slug>/<id>/actions/<name>", data = "<body>")]
#[allow(clippy::too_many_arguments)]
async fn run_row_action(
    slug: &str,
    id: &str,
    name: &str,
    body: Data<'_>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<Redirect, Status> {
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let action = resource
        .row_actions()
        .iter()
        .find(|action| action.name() == name && action.is_post())
        .cloned()
        .ok_or(Status::NotFound)?;
    let ability = ability_of(&action);
    authorize(registry, &session, slug, ability)?;
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());

    let record = authorize_record(db, &session, &resource, id, ability).await?;
    let row = record.as_object().cloned().unwrap_or_default();
    if !action.is_visible(&row) {
        return Err(Status::Forbidden);
    }
    // The handler is given the record's numeric id; a record addressed by another key has
    // it in the row, and 0 here when it has no `id`.
    let id = row
        .get("id")
        .and_then(Value::as_i64)
        .or_else(|| id.parse().ok())
        .unwrap_or(0);
    let outcome = action
        .run(db.inner(), session.acting_user(), id, &row)
        .await
        .map_err(db_status)?;
    match outcome {
        Ok(()) => {
            if let Some(message) = action.success_message() {
                set_flash(cookies, &registry.secret, message);
            }
        }
        Err(message) => set_flash(cookies, &registry.secret, &message),
    }
    Ok(Redirect::to(back_url(form.get("_back"), &index_url)))
}

/// Whether the resource hides the built-in Delete button of `record`
/// (`hide_default_delete_when`).
fn default_delete_hidden(resource: &dyn Erased, record: &Value) -> bool {
    match (resource.hide_default_delete_when(), record.as_object()) {
        (Some(filter), Some(row)) => filter.matches(row),
        _ => false,
    }
}

/// The panel's ability for a row action's [`ActionAbility`](ignite_core::ActionAbility).
fn ability_of(action: &ignite_core::RowAction) -> Action {
    match action.ability() {
        ignite_core::ActionAbility::Update => Action::Update,
        ignite_core::ActionAbility::Delete => Action::Delete,
    }
}

/// Where a row action sends the user back: `back` when it is the list at `index_url` (with a
/// query string or none), else the list itself, so the form cannot redirect elsewhere.
fn back_url(back: Option<&str>, index_url: &str) -> String {
    match back {
        Some(back)
            if back
                .strip_prefix(index_url)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('?'))
                && !back.contains(['\\', '\r', '\n']) =>
        {
            back.to_string()
        }
        _ => index_url.to_string(),
    }
}

/// The row actions of `resource` shown on `record`, whose id is `id`: those whose rule holds
/// for it, and for a POST, those whose ability the user holds on the record (`can_update`,
/// `can_delete`).
fn row_action_views(
    resource: &dyn Erased,
    record: &Value,
    key: &str,
    index_url: &str,
    can_update: bool,
    can_delete: bool,
) -> Vec<RowActionView> {
    let empty = serde_json::Map::new();
    let row = record.as_object().unwrap_or(&empty);
    resource
        .row_actions()
        .iter()
        .filter(|action| action.is_visible(row))
        .filter_map(|action| {
            let url = if action.is_post() {
                let allowed = match action.ability() {
                    ignite_core::ActionAbility::Update => can_update,
                    ignite_core::ActionAbility::Delete => can_delete,
                };
                if !allowed {
                    return None;
                }
                format!("{index_url}/{key}/actions/{}", action.name())
            } else {
                action.url_for(row)?
            };
            let color = action.get_color();
            let base = if color == Some(ignite_core::BadgeColor::Danger) {
                "ig-btn ig-btn-danger px-3 py-1"
            } else {
                "ig-btn ig-btn-ghost px-3 py-1"
            };
            let class = match color {
                Some(color) => format!("{base} ig-action-{}", color.as_str()),
                None => base.to_string(),
            };
            Some(RowActionView {
                label: action.get_label(),
                is_post: action.is_post(),
                url,
                has_confirm: action.is_post() && action.confirmation().is_some(),
                confirm: action.confirmation().unwrap_or_default().to_string(),
                class,
            })
        })
        .collect()
}

/// `form` with the values of its structured fields built from their sub-field inputs, and
/// the add, delete or move button it was sent with (`_ignite_op`) applied, for a live form.
fn operated(resource: &Arc<dyn Erased>, form: &FormInput) -> FormInput {
    let form = structured_input(resource.fields(), form);
    match form.get("_ignite_op") {
        Some(operation) => ignite_core::structure::apply_operation(
            resource.fields(),
            resource.repeaters(),
            &form,
            operation,
        ),
        None => form,
    }
}

/// Where a create or edit form posts, and its heading and button.
struct FormTarget {
    heading: String,
    action: String,
    submit_label: &'static str,
    /// Whether the form shows a read-only record, and the line it shows.
    read_only: bool,
    notice: Option<String>,
    /// Where the form sends its values when a live field changes.
    live_url: String,
    /// The slug fields that no longer follow their source (`slug_locked_when`).
    slug_locks: BTreeSet<String>,
    /// The fields shown disabled (`FieldConditions::disabled`).
    disabled: BTreeSet<String>,
}

impl FormTarget {
    /// The form that creates a record of `resource`.
    fn create(registry: &Registry, resource: &Arc<dyn Erased>) -> Self {
        let index_url = format!("{}/{}", registry.mount, resource.slug());
        FormTarget {
            heading: format!("New {}", resource.singular()),
            live_url: format!("{index_url}/new/live"),
            action: index_url,
            submit_label: "Create",
            read_only: false,
            notice: None,
            slug_locks: BTreeSet::new(),
            disabled: BTreeSet::new(),
        }
    }

    /// The form that edits the record `id` of `resource`.
    fn edit(registry: &Registry, resource: &Arc<dyn Erased>, id: &str) -> Self {
        let index_url = format!("{}/{}", registry.mount, resource.slug());
        let segment = path_segment(id);
        FormTarget {
            heading: format!("Edit {} #{id}", resource.singular()),
            action: format!("{index_url}/{segment}"),
            submit_label: "Save",
            read_only: false,
            notice: None,
            live_url: format!("{index_url}/{segment}/edit/live"),
            slug_locks: BTreeSet::new(),
            disabled: BTreeSet::new(),
        }
    }

    /// The edit form of the record `id`, read-only for the user, as Filament's View page,
    /// with the line `notice`.
    fn view(
        registry: &Registry,
        resource: &Arc<dyn Erased>,
        id: &str,
        notice: Option<String>,
    ) -> Self {
        FormTarget {
            heading: format!("View {} #{id}", resource.singular()),
            read_only: true,
            notice,
            ..FormTarget::edit(registry, resource, id)
        }
    }

    /// The form with the fields `disabled` shown disabled.
    fn with_disabled(mut self, disabled: BTreeSet<String>) -> Self {
        self.disabled = disabled;
        self
    }

    /// The form with the slug fields `locks` no longer following their source.
    fn with_slug_locks(mut self, locks: BTreeSet<String>) -> Self {
        self.slug_locks = locks;
        self
    }
}

/// Renders a create or edit form with `form`'s values and `errors`, without the fields of
/// `hidden`.
#[allow(clippy::too_many_arguments)]
fn form_view(
    registry: &Registry,
    layout: Layout,
    resource: &Arc<dyn Erased>,
    target: FormTarget,
    form: &FormInput,
    errors: &FieldErrors,
    choices: &FormChoices,
    hidden: &BTreeSet<String>,
) -> PropelView {
    let conditions = resource.conditions();
    let mut fields = form_field_views(
        resource.fields(),
        conditions,
        hidden,
        &target.slug_locks,
        form,
        errors,
        choices,
    );
    structure_views(
        &mut fields,
        resource.fields(),
        form,
        &StructureContext {
            conditions,
            repeaters: resource.repeaters(),
            errors,
            choices,
            ops_url: &target.live_url,
        },
    );
    for field in &mut fields {
        field.disabled = target.disabled.contains(&field.name);
    }
    for part in fields.iter_mut().flat_map(|field| field.parts.iter_mut()) {
        part.disabled = target.disabled.contains(&part.name);
    }
    let has_structure = fields.iter().any(|field| field.is_structured);
    // A read-only form changes nothing, so it sends nothing to the live URL.
    let has_live = !target.read_only && (has_structure || fields.iter().any(|field| field.live));
    // A disabled field sends nothing, so the live form shows its stored value.
    let shown_fields = fields
        .iter()
        .filter(|field| !field.disabled)
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let editor = editor_for(resource.fields());
    // The form script asks before a changed form is left, and fills slug fields.
    let has_form_script = registry.unsaved_changes_alerts
        || has_structure
        || has_tabs(resource.form_layout())
        || fields.iter().any(|field| field.has_slug_from);
    // A form with a layout places its fields in the layout's columns and cards; a form
    // without one renders them in one card, as before.
    let form_layout = resource.form_layout();
    let has_layout = !form_layout.is_empty();
    let (fields, columns, layout_class) = if has_layout {
        (
            Vec::new(),
            layout_columns(form_layout, fields),
            layout_grid_class(form_layout),
        )
    } else {
        (fields, Vec::new(), String::new())
    };
    let page = FormPage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        has_avatar: layout.has_avatar,
        avatar_url: layout.avatar_url,
        theme: layout.theme,
        theme_url: layout.theme_url,
        theme_switcher: layout.theme_switcher,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        nav_sections: layout.nav_sections,
        nav_collapsible: layout.nav_collapsible,
        has_flash: layout.has_flash,
        flash: layout.flash,
        theme_style: layout.theme_style,
        admin_theme: layout.admin_theme,
        brand: layout.brand,
        title: resource.title().to_string(),
        singular: resource.singular().to_string(),
        heading: target.heading,
        action: target.action,
        submit_label: target.submit_label,
        back_url: format!("{}/{}", registry.mount, resource.slug()),
        cancel_label: if target.read_only { "Back" } else { "Cancel" },
        read_only: target.read_only,
        has_notice: target.notice.is_some(),
        notice: target.notice.unwrap_or_default(),
        fields,
        has_layout,
        has_form_styles: has_layout || has_structure,
        layout_class,
        layout_columns: columns,
        content_width: content_width_class(form_layout),
        unsaved_changes_alert: registry.unsaved_changes_alerts,
        has_unsaved_changes: registry.unsaved_changes_alerts && !errors.is_empty(),
        has_form_script,
        has_live,
        live_url: target.live_url,
        shown_fields,
        has_editor: editor.is_some(),
        editor_script: editor.unwrap_or_default(),
    };
    render_page("ignite::form", &page, compiled::form::render)
}

/// Whether `record` is read-only for the signed-in user (`read_only_when`), given its edit
/// form's context with the stored values.
fn record_read_only(resource: &dyn Erased, record: &Value, session: &AdminUser) -> bool {
    let form = record_form(resource.fields(), record);
    resource.is_read_only(&FormContext::new(
        Operation::Edit,
        record.as_object(),
        session.acting_user(),
        &form,
    ))
}

/// Refuses with 403 a change to `record` when it is read-only for the signed-in user, as
/// Filament refuses to save a View page.
fn refuse_read_only(
    resource: &dyn Erased,
    record: &Value,
    session: &AdminUser,
) -> Result<(), Status> {
    if record_read_only(resource, record, session) {
        Err(Status::Forbidden)
    } else {
        Ok(())
    }
}

/// The slug fields of `resource` that no longer follow their source on the edit form of
/// `record`, whose fields hold `form` (see `FieldConditions::slug_locked_when`).
fn slug_locks(
    resource: &Arc<dyn Erased>,
    record: &Value,
    session: &AdminUser,
    form: &FormInput,
) -> BTreeSet<String> {
    let conditions = resource.conditions();
    let context = FormContext::new(
        Operation::Edit,
        record.as_object(),
        session.acting_user(),
        form,
    );
    resource
        .fields()
        .iter()
        .filter(|field| conditions.is_slug_locked(&field.name, &context))
        .map(|field| field.name.clone())
        .collect()
}

/// The fields of `resource` that its conditions hide on the form for `operation`, editing
/// `record`, whose fields hold `form`.
fn hidden_fields(
    resource: &Arc<dyn Erased>,
    operation: Operation,
    record: Option<&Value>,
    session: &AdminUser,
    form: &FormInput,
) -> BTreeSet<String> {
    resource.conditions().hidden_fields(
        resource.fields(),
        operation,
        record.and_then(Value::as_object),
        session.acting_user(),
        form,
    )
}

/// The fields of `resource` disabled on the form for `operation`, editing `record`, whose
/// fields hold `form` (see `FieldConditions::disabled`).
fn disabled_fields(
    resource: &Arc<dyn Erased>,
    operation: Operation,
    record: Option<&Value>,
    session: &AdminUser,
    form: &FormInput,
) -> BTreeSet<String> {
    let context = FormContext::new(
        operation,
        record.and_then(Value::as_object),
        session.acting_user(),
        form,
    );
    resource
        .fields()
        .iter()
        .filter(|field| resource.conditions().is_disabled(&field.name, &context))
        .map(|field| field.name.clone())
        .collect()
}

/// The fields a save leaves out: the `hidden` ones and the disabled ones.
fn skipped_fields(
    hidden: &BTreeSet<String>,
    resource: &Arc<dyn Erased>,
    operation: Operation,
    record: Option<&Value>,
    session: &AdminUser,
    form: &FormInput,
) -> BTreeSet<String> {
    let mut skipped = hidden.clone();
    skipped.extend(disabled_fields(resource, operation, record, session, form));
    skipped
}

/// Refuses a request the signed-in user may not make on the resource at `slug`: 404 for an
/// unknown resource, and 403 Forbidden when the user lacks the ability. Each resource
/// route calls it first, as Laravel's policies do.
fn authorize(
    registry: &Registry,
    session: &AdminUser,
    slug: &str,
    action: Action,
) -> Result<(), Status> {
    registry.find(slug).ok_or(Status::NotFound)?;
    if session.may(slug, action) {
        Ok(())
    } else {
        Err(Status::Forbidden)
    }
}

/// Finds the record `id` of `resource` for the signed-in user and refuses an action on it that
/// the resource's policy does not allow, as Filament resolves a record through the resource's
/// query and then asks its policy: 404 when the user's query does not find it, and 403 when the
/// policy refuses. The caller has checked the ability with [`authorize`]. Returns the record.
async fn authorize_record(
    db: &State<Database>,
    session: &AdminUser,
    resource: &Arc<dyn Erased>,
    id: &str,
    action: Action,
) -> Result<Value, Status> {
    let record = resource
        .find(db.inner(), session.acting_user(), id)
        .await
        .map_err(db_status)?
        .ok_or(Status::NotFound)?;
    if record_allowed(db, resource.slug(), session, action, &record).await? {
        Ok(record)
    } else {
        Err(Status::Forbidden)
    }
}

/// Whether the policy of the resource at `slug` lets the signed-in user do `action` on
/// `record`. A failed policy is a server error.
async fn record_allowed(
    db: &State<Database>,
    slug: &str,
    session: &AdminUser,
    action: Action,
    record: &Value,
) -> Result<bool, Status> {
    crate::authz::policy_allows(
        db.inner(),
        slug,
        session.acting_user(),
        action,
        Some(record),
    )
    .await
    .map_err(|error| {
        rocket::error!("ignite: the policy of `{slug}` failed: {error}");
        Status::InternalServerError
    })
}

/// Whether `error` is the unique index of the `users` email, which a second save of the same
/// email hits when two saves both pass the taken check.
fn is_email_index_violation(error: &OrbitError) -> bool {
    matches!(
        error,
        OrbitError::UniqueViolation { index: Some(index) }
            if *index == erased::unique_index_name("users", "email")
    )
}

/// Maps a database error to a status. A missing row is a 404; anything else is
/// logged and answered with a 500.
pub(crate) fn db_status(error: OrbitError) -> Status {
    match error {
        OrbitError::NotFound { .. } => Status::NotFound,
        other => {
            rocket::error!("ignite: database error: {other}");
            Status::InternalServerError
        }
    }
}

/// The key that addresses `record` in the panel's URLs, as one path segment: its numeric
/// `id`, or the value of the resource's `record_route_key_name`, percent-encoded. A record
/// without it is a server error.
fn record_key(resource: &dyn Erased, record: &Value) -> Result<String, Status> {
    let name = resource.record_route_key_name();
    let key = match record.get(name) {
        Some(Value::Number(number)) if name != "id" || number.is_i64() => number.to_string(),
        Some(Value::String(text)) if name != "id" && !text.is_empty() => text.clone(),
        _ => None.ok_or_else(|| {
            if name == "id" {
                rocket::error!("ignite: a record has no numeric `id` key; see https://ignite.rocketboost.rs/docs/resources");
            } else {
                rocket::error!("ignite: a record of `{}` has no `{name}` key, its route key", resource.slug());
            }
            Status::InternalServerError
        })?,
    };
    Ok(path_segment(&key))
}

/// The key of a record a save gave back, as [`record_key`], or `None` when the save gave back
/// no usable key (a data source may give back only the values it wrote).
fn saved_key(resource: &dyn Erased, record: &Value) -> Option<String> {
    match record.get(resource.record_route_key_name()) {
        Some(Value::Number(_)) | Some(Value::String(_)) => record_key(resource, record).ok(),
        _ => None,
    }
}

/// `text` as one URL path segment: every byte but the unreserved ones (`A-Z a-z 0-9 - . _ ~`)
/// percent-encoded, so a key with a slash or a space stays one segment.
fn path_segment(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The list URL with the search, tab, sort and page kept in the query string.
fn list_url(
    base: &str,
    search: &str,
    tab: Option<&str>,
    sort: Option<(&str, &str)>,
    page: i64,
) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    if !search.is_empty() {
        query.append_pair("q", search);
    }
    if let Some(tab) = tab {
        query.append_pair("tab", tab);
    }
    if let Some((key, dir)) = sort {
        query.append_pair("sort", key).append_pair("dir", dir);
    }
    if page > 1 {
        query.append_pair("page", &page.to_string());
    }
    let qs = query.finish();
    if qs.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{qs}")
    }
}

/// Reads an urlencoded form body into a [`FormInput`].
pub(crate) async fn read_form(data: Data<'_>) -> Result<FormInput, Status> {
    let body = data
        .open(MAX_FORM_BYTES.bytes())
        .into_string()
        .await
        .map_err(|_| Status::BadRequest)?;
    if !body.is_complete() {
        return Err(Status::PayloadTooLarge);
    }
    Ok(FormInput::new(form_values(form_urlencoded::parse(
        body.as_bytes(),
    ))))
}

/// The values of a form body, by field name. As PHP and Laravel read a form, the keys
/// `name[]` collect into a list and the keys `name[key]` into an object, each stored as JSON
/// text under `name`: `tags[]=1&tags[]=2` gives `tags` = `["1","2"]`, and
/// `link[url]=...&link[title]=...` gives `link` = `{"title":...,"url":...}`. Blank items are
/// dropped, and a list or object with no value left is the blank text `""`, so it reads as
/// a field sent blank. A plain key that comes twice keeps its last value. A key `name[key]`,
/// and a key of more levels, such as `members[0][name]` (a repeater's sub-field, see
/// [`ignite_core::structure`]), is also kept as it was sent.
pub(crate) fn form_values<'a>(
    pairs: impl Iterator<Item = (std::borrow::Cow<'a, str>, std::borrow::Cow<'a, str>)>,
) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let mut lists: HashMap<String, Vec<Value>> = HashMap::new();
    let mut objects: HashMap<String, serde_json::Map<String, Value>> = HashMap::new();
    for (key, value) in pairs {
        if let Some(base) = key.strip_suffix("[]") {
            let items = lists.entry(base.to_string()).or_default();
            if !value.trim().is_empty() {
                items.push(Value::from(value.into_owned()));
            }
        } else if let Some((base, inner)) = key
            .strip_suffix(']')
            .and_then(|key| key.split_once('['))
            .filter(|(base, inner)| !base.is_empty() && !inner.contains(['[', ']']))
        {
            let object = objects.entry(base.to_string()).or_default();
            if !value.trim().is_empty() {
                object.insert(inner.to_string(), Value::from(value.to_string()));
            }
            // Kept as sent too: a group's sub-field (`seo[title]`) is read by its own name.
            values.insert(key.into_owned(), value.into_owned());
        } else {
            values.insert(key.into_owned(), value.into_owned());
        }
    }
    for (name, items) in lists {
        let text = if items.is_empty() {
            String::new()
        } else {
            Value::Array(items).to_string()
        };
        values.insert(name, text);
    }
    for (name, object) in objects {
        let text = if object.is_empty() {
            String::new()
        } else {
            Value::Object(object).to_string()
        };
        values.insert(name, text);
    }
    values
}

/// The choices of the lookup fields among `names`, by field name, as the panel shows
/// them. A field that is not a lookup is skipped, so a list loads only what it shows.
async fn lookup_choices(
    db: &Database,
    user: &ActingUser,
    resource: &dyn Erased,
    names: &[&str],
) -> Result<HashMap<String, Vec<(String, String)>>, OrbitError> {
    let mut choices = HashMap::new();
    for field in resource.fields() {
        if field.kind.loads_options() && names.contains(&field.name.as_str()) {
            choices.insert(
                field.name.clone(),
                resource.options(db, user, &field.name).await?,
            );
        }
    }
    Ok(choices)
}

/// What a form shows beside its values: the choices of its lookup fields for the values of
/// `form` (a dependent field's choices follow the field it depends on), read from the
/// database, by field name and, for a sub-field of a repeater, group or flexible content, by
/// its dotted path; and the media library's items for the ids its media fields hold in
/// `form`.
async fn form_choices(
    db: &State<Database>,
    session: &AdminUser,
    resource: &Arc<dyn Erased>,
    form: &FormInput,
) -> Result<FormChoices, Status> {
    let user = session.acting_user();
    let state = erased::form_state(resource.fields(), form);
    let mut lookups = HashMap::new();
    for field in resource.fields().iter().filter(|f| f.kind.loads_options()) {
        let choices = resource
            .options_with(db.inner(), user, &field.name, &state)
            .await
            .map_err(db_status)?;
        lookups.insert(field.name.clone(), choices);
    }
    for lookup in erased::nested_lookups(resource.fields(), &state) {
        let choices = resource
            .options_with(db.inner(), user, &lookup.schema, &lookup.container)
            .await
            .map_err(db_status)?;
        lookups.insert(lookup.path, choices);
    }
    let mut media = HashMap::new();
    if let Some(library) = ignite_core::media_library() {
        let ids = erased::media_ids(resource.fields(), form);
        if !ids.is_empty() {
            let items = library.find(db.inner(), &ids).await.map_err(db_status)?;
            media.extend(items.into_iter().map(|item| (item.id, item)));
        }
    }
    Ok(FormChoices { lookups, media })
}

/// The text a cell shows for a stored value. A lookup field shows the label of its
/// choice, and any other value shows as `cell_text` shows it.
fn choice_text(value: Option<&Value>, choices: Option<&Vec<(String, String)>>) -> String {
    let Some(choices) = choices else {
        return cell_text(value);
    };
    let stored = match value {
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::String(text)) => text.clone(),
        _ => return cell_text(value),
    };
    choices
        .iter()
        .find(|(value, _)| *value == stored)
        .map(|(_, label)| label.clone())
        .unwrap_or(stored)
}

/// The tabs of the list page, with the number of rows each one matches. As a Filament tab's
/// `badge()` counts the tab's own query, a count is for all of the tab's rows, whatever the
/// search, unless the resource's counts follow the search
/// ([`tab_counts_follow_search`](ignite_core::IgniteResource::tab_counts_follow_search)). The
/// active tab's count is the list's `total` when it is the same number. A tab whose count is 0
/// is left out when the resource hides empty tabs, except the default tab and the active one.
#[allow(clippy::too_many_arguments)]
async fn tab_views(
    resource: &dyn Erased,
    tabs: &[ignite_core::Tab],
    db: &Database,
    user: &ActingUser,
    query: &ListQuery,
    active: Option<&str>,
    default: Option<&str>,
    total: i64,
    index_url: &str,
) -> Result<Vec<TabView>, OrbitError> {
    let search = query.search.as_deref().unwrap_or_default();
    let dir_name = |direction: &Direction| match direction {
        Direction::Asc => "asc",
        Direction::Desc => "desc",
    };
    let sort = query
        .sort
        .as_ref()
        .map(|(key, direction)| (key.as_str(), dir_name(direction)));
    // The counts' search: the list's, or none.
    let count_search = if resource.tab_counts_follow_search() {
        query.search.clone()
    } else {
        None
    };
    let hide_empty = resource.hide_empty_tabs();
    let mut views = Vec::with_capacity(tabs.len());
    for tab in tabs {
        let is_active = active == Some(tab.key.as_str());
        let count = if is_active && count_search == query.search {
            total
        } else {
            let count_query = ListQuery {
                page: 1,
                per_page: 0,
                search: count_search.clone(),
                sort: None,
                filter: tab.filter.clone(),
            };
            resource.list(db, user, &count_query).await?.1
        };
        if hide_empty && count == 0 && !is_active && default != Some(tab.key.as_str()) {
            continue;
        }
        views.push(TabView {
            label: tab.label.clone(),
            count,
            url: list_url(index_url, search, Some(&tab.key), sort, 1),
            active: is_active,
            class: if is_active {
                "ig-tab ig-tab-active"
            } else {
                "ig-tab"
            },
        });
    }
    Ok(views)
}

/// Builds a dashboard table from a resource's declaration and its newest records.
/// A declared key that is not a column shows its key as the heading.
fn latest_table(
    declared: &LatestRecords,
    columns: &[ColumnDef],
    records: &[Value],
    lookups: &HashMap<String, Vec<(String, String)>>,
) -> LatestTable {
    let headers = declared
        .columns
        .iter()
        .map(|key| HeaderView {
            label: columns
                .iter()
                .find(|c| c.key == *key)
                .map_or_else(|| key.to_string(), |c| c.label.clone()),
        })
        .collect();
    let rows = records
        .iter()
        .map(|record| LatestRow {
            cells: declared
                .columns
                .iter()
                .map(|key| CellView::plain(choice_text(record.get(*key), lookups.get(*key))))
                .collect(),
        })
        .collect();
    LatestTable {
        title: declared.title,
        headers,
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(body: &str) -> HashMap<String, String> {
        form_values(form_urlencoded::parse(body.as_bytes()))
    }

    #[test]
    fn bracket_keys_collect_into_json_lists_and_objects() {
        let values = values(
            "name=Ada&tags[]=&tags[]=1&tags[]=2&link[url]=https%3A%2F%2Fexample.com&link[title]=Go",
        );
        assert_eq!(values["name"], "Ada");
        assert_eq!(values["tags"], r#"["1","2"]"#);
        assert_eq!(
            values["link"],
            r#"{"title":"Go","url":"https://example.com"}"#
        );
    }

    #[test]
    fn a_list_or_object_with_only_blank_values_is_sent_blank() {
        let values = values("tags[]=&link[url]=&link[title]=");
        assert_eq!(values["tags"], "");
        assert_eq!(values["link"], "");
    }
}
