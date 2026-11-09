//! The admin routes. Each handler looks up the resource by its slug, runs the
//! database work on a blocking thread, and renders a Propel view.

use std::collections::HashMap;
use std::sync::Arc;

use ignite_core::{Conn, Direction, FieldErrors, FormInput, ListQuery, Pool};
use propel_rocket::PropelView;
use rocket::data::{Data, ToByteUnit};
use rocket::http::Status;
use rocket::request::Request;
use rocket::response::{self, Redirect, Responder};
use rocket::{get, post, routes, Route, State};
use serde_json::Value;

use crate::erased::Erased;
use crate::ignite::Registry;
use crate::views::{
    cell_text, field_views, record_form, CellView, ColumnHead, FormPage, IndexPage, Layout, RowView,
};

/// The largest form body the panel reads.
const MAX_FORM_BYTES: u64 = 64 * 1024;

/// Every route the panel mounts.
pub(crate) fn all() -> Vec<Route> {
    routes![index, new_form, create, edit_form, update, destroy]
}

/// What a write handler returns: a redirect on success, or the form again.
enum Reply {
    Redirect(Box<Redirect>),
    Page(Status, PropelView),
}

impl<'r> Responder<'r, 'static> for Reply {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'static> {
        match self {
            Reply::Redirect(redirect) => (*redirect).respond_to(request),
            Reply::Page(status, view) => (status, view).respond_to(request),
        }
    }
}

#[get("/<slug>?<q>&<sort>&<dir>&<page>")]
async fn index(
    slug: &str,
    q: Option<String>,
    sort: Option<String>,
    dir: Option<String>,
    page: Option<String>,
    registry: &State<Registry>,
    pool: &State<Pool>,
) -> Result<PropelView, Status> {
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let columns = resource.columns();
    let index_url = format!("{}/{}", registry.mount, resource.slug());

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
    let page_view = IndexPage {
        layout: Layout::new(registry),
        title: resource.title(),
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
    Ok(PropelView::render("ignite::index", &page_view))
}

#[get("/<slug>/new")]
async fn new_form(slug: &str, registry: &State<Registry>) -> Result<PropelView, Status> {
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());
    let page = FormPage {
        layout: Layout::new(registry),
        title: resource.title(),
        heading: format!("New {}", resource.title()),
        action: index_url.clone(),
        submit_label: "Create",
        back_url: index_url,
        fields: field_views(
            resource.fields(),
            &FormInput::default(),
            &FieldErrors::new(),
        ),
    };
    Ok(PropelView::render("ignite::form", &page))
}

#[post("/<slug>", data = "<body>")]
async fn create(
    slug: &str,
    body: Data<'_>,
    registry: &State<Registry>,
    pool: &State<Pool>,
) -> Result<Reply, Status> {
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());
    let form = read_form(body).await?;

    let worker = resource.clone();
    let db_form = form.clone();
    let outcome = blocking(pool, move |conn| {
        worker.create(conn, &db_form).map_err(db_status)
    })
    .await?;

    match outcome {
        Ok(_) => Ok(Reply::Redirect(Box::new(Redirect::to(index_url)))),
        Err(errors) => Ok(form_page(
            registry,
            &resource,
            &form,
            &errors,
            format!("New {}", resource.title()),
            index_url,
            "Create",
        )),
    }
}

#[get("/<slug>/<id>/edit")]
async fn edit_form(
    slug: &str,
    id: &str,
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

    let page = FormPage {
        layout: Layout::new(registry),
        title: resource.title(),
        heading: format!("Edit #{id}"),
        action: format!("{index_url}/{id}"),
        submit_label: "Save",
        back_url: index_url,
        fields: field_views(resource.fields(), &form, &FieldErrors::new()),
    };
    Ok(PropelView::render("ignite::form", &page))
}

#[post("/<slug>/<id>", data = "<body>")]
async fn update(
    slug: &str,
    id: &str,
    body: Data<'_>,
    registry: &State<Registry>,
    pool: &State<Pool>,
) -> Result<Reply, Status> {
    let id = parse_id(id)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());
    let form = read_form(body).await?;

    let worker = resource.clone();
    let db_form = form.clone();
    let outcome = blocking(pool, move |conn| {
        worker.update(conn, id, &db_form).map_err(db_status)
    })
    .await?;

    match outcome {
        Ok(_) => Ok(Reply::Redirect(Box::new(Redirect::to(index_url)))),
        Err(errors) => Ok(form_page(
            registry,
            &resource,
            &form,
            &errors,
            format!("Edit #{id}"),
            format!("{index_url}/{id}"),
            "Save",
        )),
    }
}

#[post("/<slug>/<id>/delete")]
async fn destroy(
    slug: &str,
    id: &str,
    registry: &State<Registry>,
    pool: &State<Pool>,
) -> Result<Redirect, Status> {
    let id = parse_id(id)?;
    let resource = registry.find(slug).ok_or(Status::NotFound)?;
    let index_url = format!("{}/{}", registry.mount, resource.slug());

    let worker = resource.clone();
    let deleted = blocking(pool, move |conn| worker.delete(conn, id).map_err(db_status)).await?;
    if deleted == 0 {
        return Err(Status::NotFound);
    }
    Ok(Redirect::to(index_url))
}

/// Renders a form again with the submitted values and the errors, and a 422
/// status so the browser and tests can tell it from a fresh form.
fn form_page(
    registry: &Registry,
    resource: &Arc<dyn Erased>,
    form: &FormInput,
    errors: &FieldErrors,
    heading: String,
    action: String,
    submit_label: &'static str,
) -> Reply {
    let page = FormPage {
        layout: Layout::new(registry),
        title: resource.title(),
        heading,
        action,
        submit_label,
        back_url: format!("{}/{}", registry.mount, resource.slug()),
        fields: field_views(resource.fields(), form, errors),
    };
    Reply::Page(
        Status::UnprocessableEntity,
        PropelView::render("ignite::form", &page),
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
        rocket::error!("ignite: a record has no numeric `id` key; see docs/contract.md");
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
