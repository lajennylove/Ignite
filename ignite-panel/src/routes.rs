//! The admin routes. Each handler looks up the resource by its slug, runs the
//! database work through the app's Orbit database, and renders a Propel view. Every route
//! except the sign-in and the sign-out needs an [`AdminUser`].

use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use ignite_core::{
    ActingUser, ColumnDef, Database, Direction, FieldDef, FieldErrors, FormInput, LatestRecords,
    ListQuery, Operation,
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

use crate::authz::Action;

use crate::erased::{self, Erased};
use crate::ignite::Registry;
use crate::profile;
use crate::proxy::{self, TrustedProxies};
use crate::session::{
    check_csrf, csrf_token, end_session, session_id, set_flash, start_session, take_flash,
    AdminUser,
};
use crate::sessions;
use crate::throttle;
use crate::users;
use crate::views::{
    admin_theme_options, cell_text, compiled, editor_for, field_views, form_field_views,
    live_values, record_form, render_page, CardView, CellView, ColumnHead, DashboardPage,
    FormChoices, FormPage, HeaderView, IndexPage, LatestRow, LatestTable, Layout, LoginPage,
    ProfilePage, RowView,
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
        destroy
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
    // The dashboard shows a resource's cards and table only when the user may view its records.
    let resources: Vec<Arc<dyn Erased>> = registry
        .all()
        .into_iter()
        .filter(|r| session.may(r.slug(), Action::ViewAny))
        .collect();
    let mut widgets = Vec::new();
    let mut tables = Vec::new();
    for resource in &resources {
        let database = db.inner();
        widgets.extend(resource.widgets(database).await.map_err(db_status)?);
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

    let layout = Layout::new(registry, Some(&session), "", flash, token.clone());
    let page = DashboardPage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        has_avatar: layout.has_avatar,
        avatar_url: layout.avatar_url,
        theme: layout.theme,
        theme_url: layout.theme_url,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        nav_sections: layout.nav_sections,
        has_flash: layout.has_flash,
        flash: layout.flash,
        theme_style: layout.theme_style,
        admin_theme: layout.admin_theme,
        brand: layout.brand,
        title: "Dashboard",
        cards: widgets
            .into_iter()
            .enumerate()
            .map(|(index, w)| CardView::new(index, w.label, w.value.to_string()))
            .collect(),
        tables,
    };
    Ok(render_page(
        "ignite::dashboard",
        &page,
        compiled::dashboard::render,
    ))
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

#[get("/login")]
fn login_form(cookies: &CookieJar<'_>, registry: &State<Registry>) -> PropelView {
    let token = csrf_token(cookies, &registry.secret);
    render_page(
        "ignite::login",
        &login_page(registry, "", None, token),
        compiled::login::render,
    )
}

fn login_page(
    registry: &Registry,
    email: &str,
    error: Option<&str>,
    csrf_token: String,
) -> LoginPage {
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
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        nav_sections: layout.nav_sections,
        has_flash: layout.has_flash,
        flash: layout.flash,
        theme_style: layout.theme_style,
        admin_theme: layout.admin_theme,
        brand: layout.brand,
        title: "Sign in",
        action: format!("{}/login", registry.mount),
        forgot_url: format!("{}/password/forgot", registry.mount),
        email: email.to_string(),
        has_error: error.is_some(),
        error: error.unwrap_or_default().to_string(),
    }
}

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
    let throttle_key = throttle::key(&email, client_ip.0);

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
                Some(&message),
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
            Some(
                sessions::create(database, user.id)
                    .await
                    .map_err(db_status)?,
            )
        }
        None => {
            throttle::record_failure(database, &throttle_key)
                .await
                .map_err(db_status)?;
            None
        }
    };

    match signed_in {
        Some(session_id) => {
            start_session(cookies, &registry.secret, &session_id);
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
                    Some("The email or password is incorrect."),
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
    if let Some(session_id) = session_id(cookies, &registry.secret) {
        // The row goes first: a copied cookie stops working even if the cookie is still in the jar.
        let _ = sessions::delete(db.inner(), &session_id).await;
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
) -> PropelView {
    let flash = take_flash(cookies, &registry.secret);
    let token = csrf_token(cookies, &registry.secret);
    let layout = Layout::new(registry, Some(session), "profile", flash, token);
    let fields: Vec<FieldDef> = profile::profile_fields(registry)
        .into_iter()
        .map(FieldDef::from)
        .collect();
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
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        nav_sections: layout.nav_sections,
        has_flash: layout.has_flash,
        flash: layout.flash,
        theme_style: layout.theme_style,
        admin_theme: layout.admin_theme,
        brand: layout.brand,
        title: "Profile",
        profile_action: format!("{}/profile", registry.mount),
        profile_fields: field_views(&fields, values, errors),
        password_action: format!("{}/profile/password", registry.mount),
        password_fields: field_views(&password_fields, &FormInput::default(), password_errors),
        avatar_action: format!("{}/profile/avatar", registry.mount),
        remove_avatar_action: format!("{}/profile/avatar/remove", registry.mount),
        has_admin_themes: crate::theme::registered_admin_themes().len() > 1,
        admin_theme_action: format!("{}/profile/admin-theme", registry.mount),
        admin_themes: admin_theme_options(session.admin_theme_choice()),
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

/// The profile page, linked from the top bar.
#[get("/profile")]
async fn profile_form(
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    db: &State<Database>,
) -> Result<PropelView, Status> {
    let row = load_user(db, session.user_id()).await?;
    let values = profile::form_values(
        &profile::profile_fields(registry),
        &row.name,
        &row.email,
        row.profile.as_deref(),
    );
    Ok(render_profile(
        registry,
        &session,
        cookies,
        &values,
        &FieldErrors::new(),
        &FieldErrors::new(),
    ))
}

/// Saves the name, the email and the app's profile fields.
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
    let saved = match profile::validate(&fields, &form, row.profile.as_deref()) {
        Ok(saved) => saved,
        Err(errors) => {
            return Ok(Reply::Page(
                Status::UnprocessableEntity,
                render_profile(
                    registry,
                    &session,
                    cookies,
                    &form,
                    &errors,
                    &FieldErrors::new(),
                ),
            ))
        }
    };

    let id = session.user_id();
    let database = db.inner();
    let taken = users::email_taken(database, &saved.email, id)
        .await
        .map_err(db_status)?;
    // Two saves can both pass the check above. The unique index then refuses the
    // second one, and that refusal is the same field error as a taken email.
    let saved_ok = if taken {
        false
    } else {
        match users::update_profile(database, id, &saved.name, &saved.email, &saved.custom).await {
            Ok(_) => true,
            Err(error) if is_email_index_violation(&error) => false,
            Err(error) => return Err(db_status(error)),
        }
    };
    if !saved_ok {
        let mut errors = FieldErrors::new();
        errors.insert(
            "email".to_string(),
            "Another user already has this email.".to_string(),
        );
        return Ok(Reply::Page(
            Status::UnprocessableEntity,
            render_profile(
                registry,
                &session,
                cookies,
                &form,
                &errors,
                &FieldErrors::new(),
            ),
        ));
    }

    set_flash(cookies, &registry.secret, "Profile saved.");
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
        let values = profile::form_values(
            &profile::profile_fields(registry),
            &row.name,
            &row.email,
            row.profile.as_deref(),
        );
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
        let values = profile::form_values(
            &profile::profile_fields(registry),
            &row.name,
            &row.email,
            row.profile.as_deref(),
        );
        return Ok(Reply::Page(
            Status::UnprocessableEntity,
            render_profile(
                registry,
                &session,
                cookies,
                &values,
                &FieldErrors::new(),
                &errors,
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
#[get("/<slug>?<q>&<sort>&<dir>&<page>")]
async fn index(
    slug: &str,
    q: Option<String>,
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
    let query = ListQuery {
        page: page_no,
        per_page,
        search: (!search.is_empty()).then(|| search.clone()),
        sort: sort.clone().map(|key| (key, direction)),
    };

    let keys: Vec<&str> = columns.iter().map(|c| c.key.as_str()).collect();
    let user = session.acting_user();
    let (records, total) = resource
        .list(db.inner(), user, &query)
        .await
        .map_err(db_status)?;
    let lookups = lookup_choices(db.inner(), user, resource.as_ref(), &keys)
        .await
        .map_err(db_status)?;

    // The Edit and Delete buttons of each record need the ability and the policy's answer for
    // that record, as Filament shows a record's actions only when its policy allows them.
    let can_update = session.may(slug, Action::Update);
    let can_delete = session.may(slug, Action::Delete);
    let mut rows = Vec::with_capacity(records.len());
    for record in &records {
        let id = record_id(record)?;
        rows.push(RowView {
            cells: columns
                .iter()
                .map(|c| CellView {
                    text: choice_text(record.get(&c.key), lookups.get(&c.key)),
                })
                .collect(),
            edit_url: format!("{index_url}/{id}/edit"),
            delete_url: format!("{index_url}/{id}/delete"),
            can_update: can_update
                && record_allowed(db, slug, &session, Action::Update, record).await?,
            can_delete: can_delete
                && record_allowed(db, slug, &session, Action::Delete, record).await?,
        });
    }

    let heads = columns
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
                sort_url: list_url(&index_url, &search, Some((c.key.as_str(), next)), 1),
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
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        nav_sections: layout.nav_sections,
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
        colspan: columns.len() + 1,
        rows,
        total,
        page: page_no,
        last_page,
        has_prev: page_no > 1,
        prev_url: list_url(&index_url, &search, sort_pair, page_no - 1),
        has_next: page_no < last_page,
        next_url: list_url(&index_url, &search, sort_pair, page_no + 1),
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
        FormTarget::create(registry, &resource),
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
    let form = live_values(resource.fields(), None, &form);
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
        FormTarget::create(registry, &resource),
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
    let hidden = hidden_fields(&resource, Operation::Create, None, &session, &form);

    let outcome = resource
        .create(db.inner(), session.acting_user(), &form, &hidden)
        .await
        .map_err(db_status)?;

    match outcome {
        Ok(_) => {
            set_flash(cookies, &registry.secret, "Record created.");
            Ok(Reply::Redirect(Box::new(Redirect::to(index_url))))
        }
        Err(errors) => {
            let form = live_values(resource.fields(), None, &form);
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
                    FormTarget::create(registry, &resource),
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
    let id = parse_id(id)?;
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
    Ok(form_view(
        registry,
        layout,
        &resource,
        FormTarget::edit(registry, &resource, id),
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
    let id = parse_id(id)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let record = authorize_record(db, &session, &resource, id, Action::Update).await?;
    let form = live_values(resource.fields(), Some(&record), &form);
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
        FormTarget::edit(registry, &resource, id),
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
    let id = parse_id(id)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let record = authorize_record(db, &session, &resource, id, Action::Update).await?;
    let hidden = hidden_fields(&resource, Operation::Edit, Some(&record), &session, &form);

    let outcome = resource
        .update(db.inner(), session.acting_user(), id, &form, &hidden)
        .await
        .map_err(db_status)?;

    match outcome {
        Ok(_) => {
            set_flash(cookies, &registry.secret, "Changes saved.");
            Ok(Reply::Redirect(Box::new(Redirect::to(index_url))))
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
                    FormTarget::edit(registry, &resource, id),
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
    let id = parse_id(id)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());

    authorize_record(db, &session, &resource, id, Action::Delete).await?;
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

/// Where a create or edit form posts, and its heading and button.
struct FormTarget {
    heading: String,
    action: String,
    submit_label: &'static str,
    /// Where the form sends its values when a live field changes.
    live_url: String,
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
        }
    }

    /// The form that edits the record `id` of `resource`.
    fn edit(registry: &Registry, resource: &Arc<dyn Erased>, id: i64) -> Self {
        let index_url = format!("{}/{}", registry.mount, resource.slug());
        FormTarget {
            heading: format!("Edit {} #{id}", resource.singular()),
            action: format!("{index_url}/{id}"),
            submit_label: "Save",
            live_url: format!("{index_url}/{id}/edit/live"),
        }
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
    let fields = form_field_views(resource.fields(), conditions, hidden, form, errors, choices);
    let has_live = fields.iter().any(|field| field.live);
    let shown_fields = fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let editor = editor_for(resource.fields());
    let page = FormPage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        has_avatar: layout.has_avatar,
        avatar_url: layout.avatar_url,
        theme: layout.theme,
        theme_url: layout.theme_url,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        nav_sections: layout.nav_sections,
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
        fields,
        has_live,
        live_url: target.live_url,
        shown_fields,
        has_editor: editor.is_some(),
        editor_script: editor.unwrap_or_default(),
    };
    render_page("ignite::form", &page, compiled::form::render)
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
    id: i64,
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

/// Parses the id in a path. Anything that is not a whole number is not a row, so 404.
fn parse_id(id: &str) -> Result<i64, Status> {
    id.parse().map_err(|_| Status::NotFound)
}

fn record_id(record: &Value) -> Result<i64, Status> {
    record.get("id").and_then(Value::as_i64).ok_or_else(|| {
        rocket::error!("ignite: a record has no numeric `id` key; see https://ignite.rocketboost.rs/docs/resources");
        Status::InternalServerError
    })
}

/// The list URL with the search, sort and page kept in the query string.
fn list_url(base: &str, search: &str, sort: Option<(&str, &str)>, page: i64) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    if !search.is_empty() {
        query.append_pair("q", search);
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
/// a field sent blank. A plain key that comes twice keeps its last value.
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
                object.insert(inner.to_string(), Value::from(value.into_owned()));
            }
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

/// The names of a resource's form fields.
fn field_names(resource: &dyn Erased) -> Vec<&str> {
    resource.fields().iter().map(|f| f.name.as_str()).collect()
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

/// What a form shows beside its values: the choices of its lookup fields, read from the
/// database, and the media library's items for the ids its media fields hold in `form`.
async fn form_choices(
    db: &State<Database>,
    session: &AdminUser,
    resource: &Arc<dyn Erased>,
    form: &FormInput,
) -> Result<FormChoices, Status> {
    let lookups = lookup_choices(
        db.inner(),
        session.acting_user(),
        resource.as_ref(),
        &field_names(resource.as_ref()),
    )
    .await
    .map_err(db_status)?;
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
                .map(|key| CellView {
                    text: choice_text(record.get(*key), lookups.get(*key)),
                })
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
