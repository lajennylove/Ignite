#![allow(dead_code)]
//! Fixtures shared by the panel tests: a `Widgets` resource, the `ignite_test`
//! database, and the client helpers. The tests share one table, so they run one at a time.

use std::sync::{Mutex, MutexGuard};

use diesel::dsl::sql;
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::result::{Error as DieselError, QueryResult};
use diesel::sql_query;
use diesel::sql_types::BigInt;
use ignite_core::{
    escape_like, Column, Direction, Field, FieldErrors, FieldKind, FormInput, IgniteResource,
    LatestRecords, ListQuery, Page, StatWidget,
};
use ignite_panel::{run_migrations, users, Ignite};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde::Serialize;

pub static LOCK: Mutex<()> = Mutex::new(());

/// The password of the test user created by `reset`.
pub const PASSWORD: &str = "correct horse battery";
/// The email of the test user created by `reset`.
pub const EMAIL: &str = "admin@example.test";

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
pub struct WidgetRow {
    id: i32,
    name: String,
    price: i32,
    note: Option<String>,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = widgets)]
pub struct WidgetInput {
    pub name: String,
    pub price: i32,
    pub note: Option<String>,
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

pub struct Widgets;

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

    fn latest_records() -> Option<LatestRecords> {
        Some(LatestRecords {
            title: "Latest widgets",
            columns: &["name", "price"],
            limit: 2,
        })
    }

    fn latest(conn: &mut MysqlConnection, limit: i64) -> QueryResult<Vec<WidgetRow>> {
        widgets::table
            .order(widgets::id.desc())
            .limit(limit)
            .select(WidgetRow::as_select())
            .load(conn)
    }

    fn widgets(conn: &mut MysqlConnection) -> QueryResult<Vec<StatWidget>> {
        let value: i64 = widgets::table.count().get_result(conn)?;
        Ok(vec![StatWidget {
            label: "Widgets",
            value,
        }])
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

pub fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .expect("set DATABASE_URL to the test database, such as mysql://user@localhost/ignite_test")
}

/// Locks the shared tables, resets them, seeds three rows and one user.
pub fn reset() -> MutexGuard<'static, ()> {
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

    run_migrations(&mut conn).expect("migrations");
    sql_query("DELETE FROM users").execute(&mut conn).unwrap();
    sql_query("DELETE FROM sessions")
        .execute(&mut conn)
        .unwrap();
    users::create_user(&mut conn, "Admin", EMAIL, PASSWORD).expect("create test user");
    guard
}

pub fn count_widgets() -> i64 {
    let mut conn = MysqlConnection::establish(&database_url()).unwrap();
    widgets::table.count().get_result(&mut conn).unwrap()
}

/// A client for the panel with the `Widgets` resource, not signed in.
pub fn client(mount: Option<&str>) -> Client {
    let mut ignite = Ignite::new().resource::<Widgets>();
    if let Some(mount) = mount {
        ignite = ignite.mount(mount);
    }
    Client::tracked(rocket::build().attach(ignite.fairing())).expect("valid rocket")
}

/// A client signed in as the test user.
pub fn signed_in(mount: Option<&str>) -> Client {
    let client = client(mount);
    let base = mount.unwrap_or("/admin").trim_end_matches('/').to_string();
    let status = client
        .post(format!("{base}/login"))
        .header(ContentType::Form)
        .body(format!("email={EMAIL}&password={PASSWORD}"))
        .dispatch()
        .status();
    assert_eq!(status, Status::SeeOther, "test sign-in failed");
    client
}
