//! Integration tests for the resource routes, against the `ignite_test` database.
//! The tests share one table, so they run one at a time.

use std::sync::{Mutex, MutexGuard};

use diesel::dsl::sql;
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::result::{Error as DieselError, QueryResult};
use diesel::sql_query;
use diesel::sql_types::BigInt;
use ignite_core::{
    escape_like, Column, Direction, Field, FieldErrors, FieldKind, FormInput, IgniteResource,
    ListQuery, Page,
};
use ignite_panel::Ignite;
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde::Serialize;

static LOCK: Mutex<()> = Mutex::new(());

diesel::table! {
    widgets (id) {
        id -> Integer,
        name -> Text,
        price -> Integer,
        note -> Nullable<Text>,
    }
}

#[derive(Queryable, Selectable, Serialize)]
#[diesel(table_name = widgets)]
struct WidgetRow {
    id: i32,
    name: String,
    price: i32,
    note: Option<String>,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = widgets)]
struct WidgetInput {
    name: String,
    price: i32,
    note: Option<String>,
}

const NAME: Field = Field {
    name: "name",
    label: "Name",
    kind: FieldKind::Text,
    required: true,
};
const PRICE: Field = Field {
    name: "price",
    label: "Price",
    kind: FieldKind::Number,
    required: true,
};
const NOTE: Field = Field {
    name: "note",
    label: "Note",
    kind: FieldKind::Textarea,
    required: false,
};

struct Widgets;

impl IgniteResource for Widgets {
    type Record = WidgetRow;
    type Input = WidgetInput;

    const SLUG: &'static str = "widgets";
    const TITLE: &'static str = "Widgets";
    const PER_PAGE: i64 = 2;

    fn fields() -> &'static [Field] {
        &[NAME, PRICE, NOTE]
    }

    fn columns() -> &'static [Column] {
        &[
            Column {
                key: "name",
                label: "Name",
                sortable: true,
                searchable: true,
            },
            Column {
                key: "price",
                label: "Price",
                sortable: true,
                searchable: false,
            },
        ]
    }

    fn list(conn: &mut MysqlConnection, query: &ListQuery) -> QueryResult<Page<WidgetRow>> {
        let pattern = query
            .search
            .as_ref()
            .map(|s| format!("%{}%", escape_like(s)));
        let mut rows = widgets::table.into_boxed();
        let mut count = widgets::table.into_boxed();
        if let Some(pattern) = &pattern {
            rows = rows.filter(widgets::name.like(pattern.clone()));
            count = count.filter(widgets::name.like(pattern.clone()));
        }
        let asc = !matches!(query.sort, Some((_, Direction::Desc)));
        rows = match (query.sort.as_ref().map(|(k, _)| k.as_str()), asc) {
            (Some("name"), true) => rows.order(widgets::name.asc()),
            (Some("name"), false) => rows.order(widgets::name.desc()),
            (Some("price"), true) => rows.order(widgets::price.asc()),
            (Some("price"), false) => rows.order(widgets::price.desc()),
            _ => rows.order(widgets::id.asc()),
        };
        let total: i64 = count.count().get_result(conn)?;
        let rows = rows
            .limit(query.per_page)
            .offset(query.offset())
            .select(WidgetRow::as_select())
            .load(conn)?;
        Ok(Page { rows, total })
    }

    fn find(conn: &mut MysqlConnection, id: i64) -> QueryResult<Option<WidgetRow>> {
        widgets::table
            .find(id as i32)
            .select(WidgetRow::as_select())
            .first(conn)
            .optional()
    }

    fn create(conn: &mut MysqlConnection, input: WidgetInput) -> QueryResult<WidgetRow> {
        diesel::insert_into(widgets::table)
            .values(&input)
            .execute(conn)?;
        let id: i64 = diesel::select(sql::<BigInt>("LAST_INSERT_ID()")).get_result(conn)?;
        Self::find(conn, id)?.ok_or(DieselError::NotFound)
    }

    fn update(conn: &mut MysqlConnection, id: i64, input: WidgetInput) -> QueryResult<WidgetRow> {
        let changed = diesel::update(widgets::table.find(id as i32))
            .set(&input)
            .execute(conn)?;
        if changed == 0 {
            return Err(DieselError::NotFound);
        }
        Self::find(conn, id)?.ok_or(DieselError::NotFound)
    }

    fn delete(conn: &mut MysqlConnection, id: i64) -> QueryResult<usize> {
        diesel::delete(widgets::table.find(id as i32)).execute(conn)
    }

    fn validate(form: &FormInput) -> Result<WidgetInput, FieldErrors> {
        let mut errors = FieldErrors::new();
        let name = NAME.check_required(form, &mut errors).map(str::to_string);
        let price = match PRICE.check_required(form, &mut errors) {
            Some(text) => match text.parse::<i32>() {
                Ok(price) => Some(price),
                Err(_) => {
                    errors.insert("price", "Price must be a whole number.".to_string());
                    None
                }
            },
            None => None,
        };
        match (name, price) {
            (Some(name), Some(price)) if errors.is_empty() => Ok(WidgetInput {
                name,
                price,
                note: form.get("note").map(str::to_string),
            }),
            _ => Err(errors),
        }
    }
}

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "mysql://lajennylove@localhost/ignite_test".into())
}

/// Locks the shared table, resets it and seeds three rows.
fn reset() -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("DATABASE_URL", database_url());
    let mut conn = MysqlConnection::establish(&database_url()).expect("connect to ignite_test");
    sql_query("DROP TABLE IF EXISTS widgets")
        .execute(&mut conn)
        .unwrap();
    sql_query(
        "CREATE TABLE widgets (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL, price INT NOT NULL, note TEXT NULL)",
    )
    .execute(&mut conn)
    .unwrap();
    sql_query("INSERT INTO widgets (name, price, note) VALUES ('Apple', 3, NULL), ('Banana', 1, 'Yellow'), ('Cherry', 2, NULL)")
        .execute(&mut conn)
        .unwrap();
    guard
}

fn count_widgets() -> i64 {
    let mut conn = MysqlConnection::establish(&database_url()).unwrap();
    widgets::table.count().get_result(&mut conn).unwrap()
}

fn client(mount: Option<&str>) -> Client {
    let mut ignite = Ignite::new().resource::<Widgets>();
    if let Some(mount) = mount {
        ignite = ignite.mount(mount);
    }
    Client::tracked(rocket::build().attach(ignite.fairing())).expect("valid rocket")
}

#[test]
fn list_shows_the_first_row_text() {
    let _guard = reset();
    let client = client(None);
    let body = client
        .get("/admin/widgets")
        .dispatch()
        .into_string()
        .unwrap();

    // The first row of a text query used to come back empty. See vendor/diesel/PATCH.md.
    assert!(body.contains("Apple"), "first row missing: {body}");
    assert!(body.contains("Banana"));
    assert!(!body.contains("Cherry"), "page should hold two rows");
}

#[test]
fn list_is_a_page_with_a_pager() {
    let _guard = reset();
    let client = client(None);
    let response = client.get("/admin/widgets?page=2").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_string().unwrap();

    assert!(body.contains("Cherry"));
    assert!(!body.contains("Apple"));
    assert!(body.contains("Page 2 of 2"));
    assert!(body.contains("3 in total"));
}

#[test]
fn search_matches_names_and_treats_wildcards_literally() {
    let _guard = reset();
    let client = client(None);

    let body = client
        .get("/admin/widgets?q=an")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.contains("Banana"));
    assert!(!body.contains("Apple"));

    let body = client
        .get("/admin/widgets?q=%25")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        body.contains("No records found."),
        "a literal % should match nothing"
    );
}

#[test]
fn sort_by_a_sortable_column_orders_the_page() {
    let _guard = reset();
    let client = client(None);
    let body = client
        .get("/admin/widgets?sort=price&dir=desc")
        .dispatch()
        .into_string()
        .unwrap();

    assert!(body.contains("Apple"));
    assert!(body.contains("Cherry"));
    assert!(
        !body.contains("Banana"),
        "Banana is cheapest, so it is on page 2"
    );
    assert!(body.find("Apple").unwrap() < body.find("Cherry").unwrap());
}

#[test]
fn unknown_sort_keys_are_ignored() {
    let _guard = reset();
    let client = client(None);
    let response = client
        .get("/admin/widgets?sort=password_hash;drop")
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert!(response.into_string().unwrap().contains("Apple"));
}

#[test]
fn new_form_renders_empty_fields() {
    let _guard = reset();
    let client = client(None);
    let body = client
        .get("/admin/widgets/new")
        .dispatch()
        .into_string()
        .unwrap();

    assert!(body.contains(r#"name="name""#));
    assert!(body.contains(r#"action="/admin/widgets""#));
}

#[test]
fn create_saves_a_valid_form_and_redirects() {
    let _guard = reset();
    let client = client(None);
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body("name=Date&price=5&note=Sweet")
        .dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(
        response.headers().get_one("Location"),
        Some("/admin/widgets")
    );
    assert_eq!(count_widgets(), 4);
}

#[test]
fn create_rerenders_the_form_with_errors() {
    let _guard = reset();
    let client = client(None);
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body("name=&price=abc")
        .dispatch();

    assert_eq!(response.status(), Status::UnprocessableEntity);
    let body = response.into_string().unwrap();
    assert!(body.contains("Name is required."));
    assert!(body.contains("Price must be a whole number."));
    assert_eq!(count_widgets(), 3);
}

#[test]
fn edit_form_fills_in_the_stored_values() {
    let _guard = reset();
    let client = client(None);
    let body = client
        .get("/admin/widgets/1/edit")
        .dispatch()
        .into_string()
        .unwrap();

    assert!(body.contains(r#"value="Apple""#));
    assert!(body.contains(r#"value="3""#));
    assert!(body.contains(r#"action="/admin/widgets/1""#));
}

#[test]
fn update_changes_the_row() {
    let _guard = reset();
    let client = client(None);
    let response = client
        .post("/admin/widgets/1")
        .header(ContentType::Form)
        .body("name=Apricot&price=4")
        .dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    let body = client
        .get("/admin/widgets")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.contains("Apricot"));
    assert!(!body.contains("Apple"));
}

#[test]
fn delete_removes_the_row() {
    let _guard = reset();
    let client = client(None);
    let response = client.post("/admin/widgets/2/delete").dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(count_widgets(), 2);
}

#[test]
fn missing_rows_and_resources_are_404() {
    let _guard = reset();
    let client = client(None);

    assert_eq!(
        client.get("/admin/widgets/99/edit").dispatch().status(),
        Status::NotFound
    );
    assert_eq!(
        client
            .post("/admin/widgets/99")
            .header(ContentType::Form)
            .body("name=x&price=1")
            .dispatch()
            .status(),
        Status::NotFound
    );
    assert_eq!(
        client.post("/admin/widgets/99/delete").dispatch().status(),
        Status::NotFound
    );
    assert_eq!(
        client.get("/admin/nope").dispatch().status(),
        Status::NotFound
    );
    assert_eq!(
        client.get("/admin/widgets/abc/edit").dispatch().status(),
        Status::NotFound
    );
}

#[test]
fn the_mount_path_is_configurable() {
    let _guard = reset();
    let client = client(Some("/panel/"));

    assert_eq!(client.get("/panel/widgets").dispatch().status(), Status::Ok);
    assert_eq!(
        client.get("/admin/widgets").dispatch().status(),
        Status::NotFound
    );
}
