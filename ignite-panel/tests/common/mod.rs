#![allow(dead_code)]
//! Fixtures shared by the panel tests: a `Widgets` resource, the `ignite_test`
//! database, and the client helpers. The tests share one table, so they run one at a time.

use std::future::Future;
use std::sync::{Mutex, MutexGuard};

use ignite_core::{
    escape_like, Column, Direction, Field, FieldErrors, FieldKind, FormInput, IgniteResource,
    LatestRecords, ListQuery, Page, StatWidget,
};
use ignite_panel::authz::{self, SUPER_ADMIN};
use ignite_panel::{rollback, run_migrations, users, Ignite, RecordingMailer};
use orbit_orm::__sqlx as sqlx;
use orbit_orm::{
    raw, Database, Direction as OrbitDirection, Error as OrbitError, Model, Operator, OrbitModel,
    Value,
};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use rocket::tokio::runtime::Runtime;
use serde::Serialize;

pub static LOCK: Mutex<()> = Mutex::new(());

/// The password of the test user created by `reset`.
pub const PASSWORD: &str = "correct horse battery";
/// The email of the test user created by `reset`.
pub const EMAIL: &str = "admin@example.test";

/// A row of the `widgets` table, which the tests share.
#[derive(Debug, Clone, OrbitModel, Serialize)]
#[orbit(table = "widgets")]
pub struct WidgetRow {
    id: i32,
    #[orbit(fillable)]
    name: String,
    #[orbit(fillable)]
    price: i32,
    #[orbit(fillable)]
    note: Option<String>,
}

/// The values a widget form writes.
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

    async fn list(db: &Database, query: &ListQuery) -> Result<Page<WidgetRow>, OrbitError> {
        let pattern = query
            .search
            .as_ref()
            .map(|s| format!("%{}%", escape_like(s)));
        let mut rows = WidgetRow::query();
        let mut count = WidgetRow::query();
        if let Some(pattern) = &pattern {
            rows = rows.where_op("name", Operator::Like, pattern.as_str());
            count = count.where_op("name", Operator::Like, pattern.as_str());
        }
        let asc = !matches!(query.sort, Some((_, Direction::Desc)));
        let (column, direction) = match (query.sort.as_ref().map(|(k, _)| k.as_str()), asc) {
            (Some("name"), true) => ("name", OrbitDirection::Asc),
            (Some("name"), false) => ("name", OrbitDirection::Desc),
            (Some("price"), true) => ("price", OrbitDirection::Asc),
            (Some("price"), false) => ("price", OrbitDirection::Desc),
            _ => ("id", OrbitDirection::Asc),
        };
        rows = rows.order_by(column, direction);
        let total = count.count(db).await?;
        let rows = rows
            .limit(query.per_page as u64)
            .offset(query.offset() as u64)
            .get(db)
            .await?;
        Ok(Page { rows, total })
    }

    async fn find(db: &Database, id: i64) -> Result<Option<WidgetRow>, OrbitError> {
        WidgetRow::query().find(db, id).await
    }

    async fn create(db: &Database, input: WidgetInput) -> Result<WidgetRow, OrbitError> {
        let id = WidgetRow::query()
            .insert(
                db,
                &[
                    ("name", input.name.as_str().into()),
                    ("price", input.price.into()),
                    ("note", input.note.as_deref().into()),
                ],
            )
            .await?;
        Self::find(db, id as i64)
            .await?
            .ok_or(OrbitError::NotFound { table: "widgets" })
    }

    async fn update(db: &Database, id: i64, input: WidgetInput) -> Result<WidgetRow, OrbitError> {
        // A `None` note is left as it is, as the derive does for an optional field on update.
        let mut values: Vec<(&str, Value)> = vec![
            ("name", input.name.as_str().into()),
            ("price", input.price.into()),
        ];
        if let Some(note) = &input.note {
            values.push(("note", note.as_str().into()));
        }
        let changed = WidgetRow::query()
            .where_eq("id", id)
            .update(db, &values)
            .await?;
        if changed == 0 {
            return Err(OrbitError::NotFound { table: "widgets" });
        }
        Self::find(db, id)
            .await?
            .ok_or(OrbitError::NotFound { table: "widgets" })
    }

    fn latest_records() -> Option<LatestRecords> {
        Some(LatestRecords {
            title: "Latest widgets",
            columns: &["name", "price"],
            limit: 2,
        })
    }

    async fn latest(db: &Database, limit: i64) -> Result<Vec<WidgetRow>, OrbitError> {
        WidgetRow::query()
            .order_by("id", OrbitDirection::Desc)
            .limit(limit as u64)
            .get(db)
            .await
    }

    async fn widgets(db: &Database) -> Result<Vec<StatWidget>, OrbitError> {
        let value = WidgetRow::query().count(db).await?;
        Ok(vec![StatWidget {
            label: "Widgets",
            value,
        }])
    }

    async fn delete(db: &Database, id: i64) -> Result<u64, OrbitError> {
        WidgetRow::query().where_eq("id", id).delete(db).await
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

/// A test database connection: a runtime of its own and an Orbit database on it. Every call
/// runs on that runtime, so the pool and its connections stay with the runtime that made them.
pub struct TestDb {
    runtime: Runtime,
    pub db: Database,
}

impl TestDb {
    /// Runs `future` to completion on the runtime of this connection.
    pub fn run<F: Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }

    /// Runs SQL that may hold several statements, such as a migration's `up.sql`.
    pub fn exec(&self, sql: impl AsRef<str>) {
        let sql = sql.as_ref();
        self.run(async {
            sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                .execute(self.db.pool())
                .await
                .expect("run the SQL")
        });
    }

    /// Runs one statement that may fail, and returns the error instead of panicking.
    pub fn try_exec(&self, sql: &str) -> Result<(), OrbitError> {
        self.run(async { raw::execute(&self.db, sql, &[]).await.map(|_| ()) })
    }

    /// Runs one statement with `binds`, and returns the rows affected.
    pub fn exec_with(&self, sql: &str, binds: &[Value]) -> u64 {
        self.run(async {
            raw::execute(&self.db, sql, binds)
                .await
                .expect("run the statement")
        })
    }

    /// Reads one integer from a statement, such as a `COUNT`.
    pub fn count(&self, sql: &str, binds: &[Value]) -> i64 {
        self.run(async {
            raw::fetch_scalar::<i64>(&self.db, sql, binds)
                .await
                .expect("count")
        })
    }
}

/// A connection to the test database, on a runtime of its own.
pub fn test_db() -> TestDb {
    let runtime = Runtime::new().expect("a runtime for the test");
    let db = runtime
        .block_on(Database::connect(&database_url()))
        .expect("connect to ignite_test through Orbit");
    TestDb { runtime, db }
}

pub fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .expect("set DATABASE_URL to the test database, such as mysql://user@localhost/ignite_test")
}

/// Puts the test database into the state where exactly `applied` are applied: every panel
/// migration is rolled back, the `applied` ones run their `up.sql`, and Orbit's table records
/// them as one batch. The remaining migrations are left for `migrate` to run. Hold the guard
/// from `reset` while calling this.
pub fn rewind_to(applied: &[&str]) {
    let t = test_db();
    t.run(async { while !rollback(&t.db).await.expect("rollback").is_empty() {} });
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/migrations/");
    for name in applied {
        let up = std::fs::read_to_string(format!("{root}{name}/up.sql")).expect("read up.sql");
        t.exec(&up);
        t.exec_with(
            "INSERT INTO __diesel_schema_migrations (version, batch) VALUES (?, 1)",
            &[name[..14].into()],
        );
    }
}

/// Locks the shared tables, resets them, seeds three rows and one user.
pub fn reset() -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("DATABASE_URL", database_url());
    let t = test_db();
    t.exec("DROP TABLE IF EXISTS widgets");
    t.exec(
        "CREATE TABLE widgets (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL, price INT NOT NULL, note TEXT NULL)",
    );
    t.exec("INSERT INTO widgets (name, price, note) VALUES ('Apple', 3, NULL), ('Banana', 1, 'Yellow'), ('Cherry', 2, NULL)");

    // Roll the panel back to nothing, then apply every migration: one batch, the same each time.
    t.run(async {
        while !rollback(&t.db).await.expect("rollback").is_empty() {}
        run_migrations(&t.db).await.expect("migrations");
    });
    for table in [
        "users",
        "sessions",
        "login_throttles",
        "password_reset_tokens",
        "role_user",
        "role_permission",
        "permissions",
        "roles",
    ] {
        t.exec(format!("DELETE FROM {table}"));
    }
    t.run(async {
        users::create_user(&t.db, "Admin", EMAIL, PASSWORD)
            .await
            .expect("create test user");
        // The test user is a super-admin, so the tests that are not about roles pass every check.
        authz::create_role(&t.db, SUPER_ADMIN)
            .await
            .expect("create super-admin");
        authz::assign_role(&t.db, EMAIL, SUPER_ADMIN)
            .await
            .expect("make the test user a super-admin");
    });
    guard
}

pub fn count_widgets() -> i64 {
    test_db().count("SELECT COUNT(*) FROM widgets", &[])
}

/// Sets a random `IGNITE_SECRET_KEY` when none is set. A release build refuses to
/// start without one, and a debug build would otherwise pick a random key per instance.
pub fn ensure_secret() {
    if std::env::var_os("IGNITE_SECRET_KEY").is_none() {
        std::env::set_var("IGNITE_SECRET_KEY", random_key());
    }
}

/// A new random key, base64 encoded, as `openssl rand -base64 32` makes one.
pub fn random_key() -> String {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    let mut bytes = [0u8; 32];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    STANDARD.encode(bytes)
}

/// A client for the panel with the `Widgets` resource, not signed in.
pub fn client(mount: Option<&str>) -> Client {
    client_on(mount, rocket::build())
}

/// A client for the panel built on `rocket`, such as one with a TLS configuration.
pub fn client_on(mount: Option<&str>, rocket: rocket::Rocket<rocket::Build>) -> Client {
    ensure_secret();
    Client::tracked(rocket.attach(builder(mount).fairing())).expect("valid rocket")
}

/// A client for the panel whose mail goes to `mailer`. Keep a clone of the recorder to read
/// the messages the panel sent.
pub fn client_with_mailer(mount: Option<&str>, mailer: RecordingMailer) -> Client {
    ensure_secret();
    let ignite = builder(mount).mailer(mailer);
    Client::tracked(rocket::build().attach(ignite.fairing())).expect("valid rocket")
}

/// The panel with the `Widgets` resource and the app's profile field, under `mount`.
fn builder(mount: Option<&str>) -> Ignite {
    // An app's own profile field, declared the way the developer declares it.
    let mut ignite = Ignite::new()
        .resource::<Widgets>()
        .profile_field(ignite_core::Field {
            name: "job_title",
            label: "Job title",
            kind: ignite_core::FieldKind::Text,
            required: false,
        });
    if let Some(mount) = mount {
        ignite = ignite.mount(mount);
    }
    ignite
}

/// The CSRF token of a client's browser session, read from its token cookie.
/// The cookie is `payload.tag`, and the payload is base64url JSON with the token.
pub fn csrf_token(client: &Client) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let cookie = client
        .cookies()
        .get("ignite_csrf")
        .expect("the token cookie is set by a page that has a form")
        .value()
        .to_string();
    let payload = cookie.split('.').next().expect("payload");
    let json = URL_SAFE_NO_PAD.decode(payload).expect("base64 payload");
    let value: serde_json::Value = serde_json::from_slice(&json).expect("json payload");
    value["token"].as_str().expect("token").to_string()
}

/// Adds the CSRF token to a form body, as the panel's own forms do.
pub fn with_token(client: &Client, body: &str) -> String {
    let token = csrf_token(client);
    if body.is_empty() {
        format!("_token={token}")
    } else {
        format!("{body}&_token={token}")
    }
}

/// A client signed in as the test user. It loads the login page first, so it has a token.
pub fn signed_in(mount: Option<&str>) -> Client {
    signed_in_as(mount, EMAIL, PASSWORD)
}

/// A client signed in as the user with `email` and `password`.
pub fn signed_in_as(mount: Option<&str>, email: &str, password: &str) -> Client {
    let client = client(mount);
    let base = mount.unwrap_or("/admin").trim_end_matches('/').to_string();
    client.get(format!("{base}/login")).dispatch();
    let body = with_token(&client, &format!("email={email}&password={password}"));
    let status = client
        .post(format!("{base}/login"))
        .header(ContentType::Form)
        .body(body)
        .dispatch()
        .status();
    assert_eq!(status, Status::SeeOther, "test sign-in failed");
    client
}
