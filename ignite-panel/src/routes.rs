//! The admin routes. Each handler looks up the resource by its slug, runs the
//! database work on a blocking thread, and renders a Propel view. Every route
//! except the sign-in and the sign-out needs an [`AdminUser`].

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use ignite_core::{
    Column, Conn, Direction, FieldErrors, FormInput, LatestRecords, ListQuery, Pool,
};
use propel_rocket::PropelView;
use rocket::data::{Data, ToByteUnit};
use rocket::http::{ContentType, CookieJar, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::response::{self, Redirect, Responder, Response};
use rocket::{get, post, routes, Route, State};
use serde_json::Value;

use crate::erased::Erased;
use crate::ignite::Registry;
use crate::session::{
    check_csrf, csrf_token, end_session, session_id, set_flash, start_session, take_flash,
    AdminUser,
};
use crate::sessions;
use crate::throttle;
use crate::users;
use crate::views::{
    cell_text, compiled, field_views, record_form, render_page, CardView, CellView, ColumnHead,
    DashboardPage, FormPage, HeaderView, IndexPage, LatestRow, LatestTable, Layout, LoginPage,
    ProfilePage, RowView,
};

/// The largest form body the panel reads.
const MAX_FORM_BYTES: u64 = 64 * 1024;

/// Every route the panel mounts.
pub(crate) fn all() -> Vec<Route> {
    routes![
        build_asset,
        dashboard,
        login_form,
        login,
        logout,
        logout_everywhere,
        profile,
        index,
        new_form,
        create,
        edit_form,
        update,
        destroy
    ]
}

/// What a write handler returns: a redirect on success, or a page again.
enum Reply {
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
    pool: &State<Pool>,
) -> Result<PropelView, Status> {
    let flash = take_flash(cookies, &registry.secret);
    let token = csrf_token(cookies, &registry.secret);
    let resources = registry.resources.clone();
    let (widgets, tables) = blocking(pool, move |conn| {
        let mut widgets = Vec::new();
        let mut tables = Vec::new();
        for resource in &resources {
            widgets.extend(resource.widgets(conn).map_err(db_status)?);
            if let Some(declared) = resource.latest_records() {
                let limit = declared.limit.max(1);
                let records = resource.latest(conn, limit).map_err(db_status)?;
                tables.push((declared, resource.columns(), records));
            }
        }
        Ok((widgets, tables))
    })
    .await?;
    let tables = tables
        .into_iter()
        .map(|(declared, columns, records)| latest_table(&declared, columns, &records))
        .collect();

    let layout = Layout::new(registry, Some(&session), "", flash, token.clone());
    let page = DashboardPage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        has_flash: layout.has_flash,
        flash: layout.flash,
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
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        has_flash: layout.has_flash,
        flash: layout.flash,
        title: "Sign in",
        action: format!("{}/login", registry.mount),
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
    pool: &State<Pool>,
) -> Result<Reply, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let email = form.get("email").unwrap_or("").trim().to_string();
    let password = form.get("password").unwrap_or("").to_string();
    let email_key = throttle::email_key(&email);
    let ip_key = throttle::ip_key(client_ip.0);

    // Refuse before checking the password while either count is at its limit.
    let (check_email, check_ip) = (email_key.clone(), ip_key.clone());
    let waiting = blocking(pool, move |conn| {
        let by_email =
            throttle::retry_after(conn, &check_email, throttle::EMAIL_LIMIT).map_err(db_status)?;
        let by_ip =
            throttle::retry_after(conn, &check_ip, throttle::IP_LIMIT).map_err(db_status)?;
        Ok(by_email.into_iter().chain(by_ip).max())
    })
    .await?;
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

    let lookup = email.clone();
    let signed_in = blocking(pool, move |conn| {
        let user = users::find_by_email(conn, &lookup).map_err(db_status)?;
        // Check a password even when there is no such user, so the response time
        // does not show which emails are registered.
        let matched = match &user {
            Some(user) => users::verify_password(&user.password_hash, &password),
            None => {
                users::verify_password(dummy_hash(), &password);
                false
            }
        };
        match user.filter(|_| matched) {
            Some(user) => {
                throttle::clear(conn, &email_key).map_err(db_status)?;
                sessions::create(conn, user.id).map(Some).map_err(db_status)
            }
            None => {
                throttle::record_failure(conn, &email_key).map_err(db_status)?;
                throttle::record_failure(conn, &ip_key).map_err(db_status)?;
                Ok(None)
            }
        }
    })
    .await?;

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

/// The client's IP address, for the login throttle. Unknown when the request has none.
struct ClientIp(Option<IpAddr>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ClientIp {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, ()> {
        Outcome::Success(ClientIp(request.client_ip()))
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
    pool: &State<Pool>,
) -> Result<Redirect, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    if let Some(session_id) = session_id(cookies, &registry.secret) {
        // The row goes first: a copied cookie stops working even if the cookie is still in the jar.
        let _ = blocking(pool, move |conn| {
            sessions::delete(conn, &session_id).map_err(db_status)
        })
        .await;
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
    pool: &State<Pool>,
) -> Result<Redirect, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    blocking(pool, move |conn| {
        sessions::delete_for_user(conn, user.user_id()).map_err(db_status)
    })
    .await?;
    end_session(cookies);
    Ok(Redirect::to(format!("{}/login", registry.mount)))
}

/// The signed-in user's profile, linked from the top bar.
#[get("/profile")]
async fn profile(
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
) -> PropelView {
    let flash = take_flash(cookies, &registry.secret);
    let token = csrf_token(cookies, &registry.secret);
    let layout = Layout::new(registry, Some(&session), "profile", flash, token);
    let page = ProfilePage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        has_flash: layout.has_flash,
        flash: layout.flash,
        title: "Profile",
        email: session.email().to_string(),
    };
    render_page("ignite::profile", &page, compiled::profile::render)
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
    pool: &State<Pool>,
) -> Result<PropelView, Status> {
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
    let per_page = resource.per_page();
    let query = ListQuery {
        page: page_no,
        per_page,
        search: (!search.is_empty()).then(|| search.clone()),
        sort: sort.clone().map(|key| (key, direction)),
    };

    let worker = resource.clone();
    let (records, total) = blocking(pool, move |conn| {
        worker.list(conn, &query).map_err(db_status)
    })
    .await?;

    let mut rows = Vec::with_capacity(records.len());
    for record in &records {
        let id = record_id(record)?;
        rows.push(RowView {
            cells: columns
                .iter()
                .map(|c| CellView {
                    text: cell_text(record.get(c.key)),
                })
                .collect(),
            edit_url: format!("{index_url}/{id}/edit"),
            delete_url: format!("{index_url}/{id}/delete"),
        });
    }

    let heads = columns
        .iter()
        .map(|c| {
            let active = sort.as_deref() == Some(c.key);
            let (indicator, next) = match (active, direction) {
                (true, Direction::Asc) => ("↑", "desc"),
                (true, Direction::Desc) => ("↓", "asc"),
                (false, _) => ("", "asc"),
            };
            ColumnHead {
                label: c.label,
                sortable: c.sortable,
                sort_url: list_url(&index_url, &search, Some((c.key, next)), 1),
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
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        has_flash: layout.has_flash,
        flash: layout.flash,
        title: resource.title(),
        singular: resource.singular(),
        new_url: format!("{index_url}/new"),
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
) -> Result<PropelView, Status> {
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());
    let layout = Layout::new(
        registry,
        Some(&session),
        slug,
        take_flash(cookies, &registry.secret),
        csrf_token(cookies, &registry.secret),
    );
    let page = FormPage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        has_flash: layout.has_flash,
        flash: layout.flash,
        title: resource.title(),
        singular: resource.singular(),
        heading: format!("New {}", resource.singular()),
        action: index_url.clone(),
        submit_label: "Create",
        back_url: index_url,
        fields: field_views(
            resource.fields(),
            &FormInput::default(),
            &FieldErrors::new(),
        ),
    };
    Ok(render_page("ignite::form", &page, compiled::form::render))
}

#[post("/<slug>", data = "<body>")]
async fn create(
    slug: &str,
    body: Data<'_>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    pool: &State<Pool>,
) -> Result<Reply, Status> {
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;

    let worker = resource.clone();
    let db_form = form.clone();
    let outcome = blocking(pool, move |conn| {
        worker.create(conn, &db_form).map_err(db_status)
    })
    .await?;

    match outcome {
        Ok(_) => {
            set_flash(cookies, &registry.secret, "Record created.");
            Ok(Reply::Redirect(Box::new(Redirect::to(index_url))))
        }
        Err(errors) => Ok(form_page(
            registry,
            &session,
            &resource,
            &form,
            &errors,
            format!("New {}", resource.singular()),
            index_url,
            "Create",
            csrf_token(cookies, &registry.secret),
        )),
    }
}

#[get("/<slug>/<id>/edit")]
async fn edit_form(
    slug: &str,
    id: &str,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    pool: &State<Pool>,
) -> Result<PropelView, Status> {
    let id = parse_id(id)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());

    let worker = resource.clone();
    let record = blocking(pool, move |conn| worker.find(conn, id).map_err(db_status))
        .await?
        .ok_or(Status::NotFound)?;
    let form = record_form(resource.fields(), &record);

    let layout = Layout::new(
        registry,
        Some(&session),
        slug,
        take_flash(cookies, &registry.secret),
        csrf_token(cookies, &registry.secret),
    );
    let page = FormPage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        has_flash: layout.has_flash,
        flash: layout.flash,
        title: resource.title(),
        singular: resource.singular(),
        heading: format!("Edit {} #{id}", resource.singular()),
        action: format!("{index_url}/{id}"),
        submit_label: "Save",
        back_url: index_url,
        fields: field_views(resource.fields(), &form, &FieldErrors::new()),
    };
    Ok(render_page("ignite::form", &page, compiled::form::render))
}

#[post("/<slug>/<id>", data = "<body>")]
async fn update(
    slug: &str,
    id: &str,
    body: Data<'_>,
    session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    pool: &State<Pool>,
) -> Result<Reply, Status> {
    let id = parse_id(id)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;

    let worker = resource.clone();
    let db_form = form.clone();
    let outcome = blocking(pool, move |conn| {
        worker.update(conn, id, &db_form).map_err(db_status)
    })
    .await?;

    match outcome {
        Ok(_) => {
            set_flash(cookies, &registry.secret, "Changes saved.");
            Ok(Reply::Redirect(Box::new(Redirect::to(index_url))))
        }
        Err(errors) => Ok(form_page(
            registry,
            &session,
            &resource,
            &form,
            &errors,
            format!("Edit {} #{id}", resource.singular()),
            format!("{index_url}/{id}"),
            "Save",
            csrf_token(cookies, &registry.secret),
        )),
    }
}

#[post("/<slug>/<id>/delete", data = "<body>")]
async fn destroy(
    slug: &str,
    id: &str,
    body: Data<'_>,
    _session: AdminUser,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    pool: &State<Pool>,
) -> Result<Redirect, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let id = parse_id(id)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());

    let worker = resource.clone();
    let deleted = blocking(pool, move |conn| worker.delete(conn, id).map_err(db_status)).await?;
    if deleted == 0 {
        return Err(Status::NotFound);
    }
    set_flash(cookies, &registry.secret, "Record deleted.");
    Ok(Redirect::to(index_url))
}

/// Renders a form again with the submitted values and the errors, and a 422
/// status so the browser and tests can tell it from a fresh form.
#[allow(clippy::too_many_arguments)]
fn form_page(
    registry: &Registry,
    user: &AdminUser,
    resource: &Arc<dyn Erased>,
    form: &FormInput,
    errors: &FieldErrors,
    heading: String,
    action: String,
    submit_label: &'static str,
    csrf_token: String,
) -> Reply {
    let layout = Layout::new(registry, Some(user), resource.slug(), None, csrf_token);
    let page = FormPage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        has_flash: layout.has_flash,
        flash: layout.flash,
        title: resource.title(),
        singular: resource.singular(),
        heading,
        action,
        submit_label,
        back_url: format!("{}/{}", registry.mount, resource.slug()),
        fields: field_views(resource.fields(), form, errors),
    };
    Reply::Page(
        Status::UnprocessableEntity,
        render_page("ignite::form", &page, compiled::form::render),
    )
}

/// Runs `work` on a blocking thread with a connection from the pool.
async fn blocking<T, F>(pool: &State<Pool>, work: F) -> Result<T, Status>
where
    T: Send + 'static,
    F: FnOnce(&mut Conn) -> Result<T, Status> + Send + 'static,
{
    let pool = pool.inner().clone();
    rocket::tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().map_err(|e| {
            rocket::error!("ignite: no database connection: {e}");
            Status::InternalServerError
        })?;
        work(&mut conn)
    })
    .await
    .unwrap_or(Err(Status::InternalServerError))
}

/// Maps a database error to a status. A missing row is a 404; anything else is
/// logged and answered with a 500.
fn db_status(error: diesel::result::Error) -> Status {
    match error {
        diesel::result::Error::NotFound => Status::NotFound,
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
async fn read_form(data: Data<'_>) -> Result<FormInput, Status> {
    let body = data
        .open(MAX_FORM_BYTES.bytes())
        .into_string()
        .await
        .map_err(|_| Status::BadRequest)?;
    if !body.is_complete() {
        return Err(Status::PayloadTooLarge);
    }
    let values: HashMap<String, String> = form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect();
    Ok(FormInput::new(values))
}

/// Builds a dashboard table from a resource's declaration and its newest records.
/// A declared key that is not a column shows its key as the heading.
fn latest_table(declared: &LatestRecords, columns: &[Column], records: &[Value]) -> LatestTable {
    let headers = declared
        .columns
        .iter()
        .map(|key| HeaderView {
            label: columns
                .iter()
                .find(|c| c.key == *key)
                .map(|c| c.label)
                .unwrap_or(key),
        })
        .collect();
    let rows = records
        .iter()
        .map(|record| LatestRow {
            cells: declared
                .columns
                .iter()
                .map(|key| CellView {
                    text: cell_text(record.get(*key)),
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
