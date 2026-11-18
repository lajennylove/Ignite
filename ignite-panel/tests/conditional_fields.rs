//! Conditional fields through the panel, as Filament's `visible()`, `hidden()`, `hiddenOn()`
//! and `live()`: a field a condition hides is not rendered, is not required, and is not
//! written, whatever a request sends for it; a live field marks the form so the panel's script
//! sends a change to the server, which renders the fields for the new values. Runs against the
//! test database.

mod common;

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, Column, ColumnDef, Database, Field, FieldConditions,
    FieldDef, FieldErrors, FieldKindDef, FormInput, IgniteResource, ListQuery, Operation, Page,
    Row, RuntimeData, RuntimeFuture, RuntimeResource,
};
use orbit_orm::{Error as OrbitError, Model, OrbitModel, Value};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::json;

/// A row of the `cond_posts` table.
#[derive(Debug, Clone, OrbitModel)]
#[orbit(table = "cond_posts")]
struct PostRow {
    id: i64,
    #[orbit(fillable)]
    title: String,
    #[orbit(fillable)]
    kind: Option<String>,
    #[orbit(fillable)]
    company: Option<String>,
    #[orbit(fillable)]
    secret: Option<String>,
    #[orbit(fillable)]
    internal: Option<String>,
    #[orbit(fillable)]
    slug: Option<String>,
    #[orbit(fillable)]
    locked: bool,
}

impl PostRow {
    fn into_row(self) -> Row {
        let mut row = Row::new();
        row.insert("id".into(), json!(self.id));
        row.insert("title".into(), json!(self.title));
        row.insert("kind".into(), json!(self.kind));
        row.insert("company".into(), json!(self.company));
        row.insert("secret".into(), json!(self.secret));
        row.insert("internal".into(), json!(self.internal));
        row.insert("slug".into(), json!(self.slug));
        row.insert("locked".into(), json!(self.locked));
        row
    }
}

/// The values of a validated row, as Orbit binds them: only the keys the row has.
fn values(input: &Row) -> Vec<(&'static str, Value)> {
    let mut values = Vec::new();
    for key in ["title", "kind", "company", "secret", "internal", "slug"] {
        if let Some(value) = input.get(key) {
            values.push((key, value.as_str().map(str::to_string).into()));
        }
    }
    values
}

struct Posts;

impl RuntimeData for Posts {
    fn list<'a>(&'a self, db: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async move {
            let rows = PostRow::query().get(db).await?;
            Ok(Page {
                total: rows.len() as i64,
                rows: rows.into_iter().map(PostRow::into_row).collect(),
            })
        })
    }

    fn find<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(async move { Ok(PostRow::query().find(db, id).await?.map(PostRow::into_row)) })
    }

    fn create<'a>(&'a self, db: &'a Database, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move {
            let id = PostRow::query().insert(db, &values(&input)).await?;
            self.find(db, id as i64).await?.ok_or(OrbitError::NotFound {
                table: "cond_posts",
            })
        })
    }

    fn update<'a>(&'a self, db: &'a Database, id: i64, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move {
            PostRow::query()
                .where_eq("id", id)
                .update(db, &values(&input))
                .await?;
            self.find(db, id).await?.ok_or(OrbitError::NotFound {
                table: "cond_posts",
            })
        })
    }

    fn delete<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async move { PostRow::query().where_eq("id", id).delete(db).await })
    }
}

fn text(name: &str, kind: FieldKindDef, required: bool) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: name.into(),
        kind,
        required,
    }
}

/// The posts resource at `slug`. `company` shows for a business, `secret` (required) is
/// hidden on a locked record, `internal` is hidden from the test user, and `slug` is hidden
/// on create. `kind` is live.
fn posts(slug: &str) -> RuntimeResource {
    let kinds = vec![
        ("person".to_string(), "Person".to_string()),
        ("business".to_string(), "Business".to_string()),
    ];
    RuntimeResource::new(slug, "Posts", "Post", Posts)
        .field(text("title", FieldKindDef::Text, true))
        .field(text("kind", FieldKindDef::Select(kinds), false))
        .field(text("company", FieldKindDef::Text, false))
        .field(text("secret", FieldKindDef::Text, true))
        .field(text("internal", FieldKindDef::Text, false))
        .field(text("slug", FieldKindDef::Text, false))
        .column(ColumnDef {
            key: "title".into(),
            label: "Title".into(),
            sortable: false,
            searchable: false,
        })
        .live("kind")
        .visible("company", |form| form.get("kind") == Some("business"))
        .hidden("secret", |form| {
            form.record()
                .and_then(|record| record.get("locked"))
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        })
        .hidden("internal", |form| form.user().email() == EMAIL)
        .hidden_on("slug", &[Operation::Create])
}

/// Resets the shared tables and makes `cond_posts` with a locked post (1) and an open one (2).
fn reset_posts(slug: &str) -> (std::sync::MutexGuard<'static, ()>, Client) {
    let guard = reset();
    let t = test_db();
    t.exec("DROP TABLE IF EXISTS cond_posts");
    t.exec(
        "CREATE TABLE cond_posts (id BIGINT AUTO_INCREMENT PRIMARY KEY, \
         title VARCHAR(255) NOT NULL, kind VARCHAR(32) NULL, company VARCHAR(255) NULL, \
         secret VARCHAR(255) NULL, internal VARCHAR(255) NULL, slug VARCHAR(255) NULL, \
         locked BOOLEAN NOT NULL DEFAULT FALSE)",
    );
    t.exec(
        "INSERT INTO cond_posts (title, kind, company, secret, internal, slug, locked) VALUES \
         ('Locked', 'business', 'Acme', 'stored secret', 'stored internal', 'locked', TRUE), \
         ('Open', 'person', 'Old Co', 'open secret', NULL, 'open', FALSE)",
    );
    let _ = unregister_runtime(slug);
    register_runtime(posts(slug)).expect("register");
    (guard, signed_in(None))
}

fn post(client: &Client, path: &str, body: &str) -> (Status, String) {
    let response = client
        .post(path)
        .header(ContentType::Form)
        .body(with_token(client, body))
        .dispatch();
    (
        response.status(),
        response.into_string().unwrap_or_default(),
    )
}

fn stored(id: i64, column: &str) -> Option<String> {
    let t = test_db();
    t.run(async {
        PostRow::query()
            .find(&t.db, id)
            .await
            .expect("find")
            .map(|row| row.into_row())
            .and_then(|row| row.get(column).and_then(|v| v.as_str()).map(str::to_string))
    })
}

#[test]
fn a_hidden_field_is_not_rendered_and_a_live_field_marks_the_form() {
    let (_guard, client) = reset_posts("cond-posts");

    let body = client
        .get("/admin/cond-posts/new")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.contains(r#"name="title""#), "{body}");
    assert!(
        body.contains(r#"name="secret""#),
        "not locked: no record on create"
    );
    for hidden in ["company", "internal", "slug"] {
        assert!(
            !body.contains(&format!(r#"name="{hidden}""#)),
            "{hidden}: {body}"
        );
        assert!(!body.contains(&format!(r#"data-ignite-field="{hidden}""#)));
    }
    // The script finds the live field and where to send the form.
    assert!(
        body.contains(r#"data-ignite-field="kind" data-ignite-live"#),
        "{body}"
    );
    assert!(!body.contains(r#"data-ignite-field="title" data-ignite-live"#));
    assert!(body.contains(r#"data-ignite-live-url="/admin/cond-posts/new/live""#));
    assert!(
        body.contains(r#"name="_ignite_shown" value="title,kind,secret""#),
        "{body}"
    );

    let body = client
        .get("/admin/cond-posts/1/edit")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.contains(r#"name="company""#), "a business: {body}");
    assert!(body.contains(r#"value="Acme""#));
    assert!(body.contains(r#"name="slug""#), "shown on edit");
    assert!(!body.contains(r#"name="secret""#), "locked");
    assert!(!body.contains("stored secret"));
    assert!(!body.contains(r#"name="internal""#));
    assert!(body.contains(r#"data-ignite-live-url="/admin/cond-posts/1/edit/live""#));

    unregister_runtime("cond-posts").expect("unregister");
}

#[test]
fn a_change_to_a_live_field_renders_the_fields_for_the_new_value() {
    let (_guard, client) = reset_posts("live-posts");

    let (status, body) = post(
        &client,
        "/admin/live-posts/new/live",
        "title=A&kind=business",
    );
    assert_eq!(status, Status::Ok);
    assert!(body.contains(r#"name="company""#), "{body}");
    assert!(body.contains(r#"value="A""#), "keeps what was typed");
    let (_, body) = post(&client, "/admin/live-posts/new/live", "title=A&kind=person");
    assert!(!body.contains(r#"name="company""#), "{body}");

    // On the edit form a field the change reveals shows its stored value.
    let (status, body) = post(
        &client,
        "/admin/live-posts/2/edit/live",
        "_ignite_shown=title,kind,secret,slug&title=Open&kind=business&secret=open+secret&slug=open",
    );
    assert_eq!(status, Status::Ok);
    assert!(body.contains(r#"value="Old Co""#), "{body}");
    // Nothing was saved.
    assert_eq!(stored(2, "kind").as_deref(), Some("person"));

    // The live routes need the token, as every form does.
    let response = client
        .post("/admin/live-posts/new/live")
        .header(ContentType::Form)
        .body("kind=business")
        .dispatch();
    assert_ne!(response.status(), Status::Ok);

    unregister_runtime("live-posts").expect("unregister");
}

#[test]
fn a_hidden_field_is_not_required_and_a_request_cannot_change_it() {
    let (_guard, client) = reset_posts("guarded-posts");

    // The locked post hides `secret`, which is required: the save needs no value for it, and
    // a crafted value for it, for `internal` or for `company` of a person is not written.
    let (status, body) = post(
        &client,
        "/admin/guarded-posts/1",
        "title=Locked+again&kind=person&company=Evil&secret=hacked&internal=hacked&slug=locked",
    );
    assert_eq!(status, Status::SeeOther, "{body}");
    assert_eq!(stored(1, "title").as_deref(), Some("Locked again"));
    assert_eq!(stored(1, "secret").as_deref(), Some("stored secret"));
    assert_eq!(stored(1, "internal").as_deref(), Some("stored internal"));
    assert_eq!(
        stored(1, "company").as_deref(),
        Some("Acme"),
        "hidden for a person"
    );

    // A shown field is still checked and written.
    let (status, _) = post(
        &client,
        "/admin/guarded-posts/2",
        "title=Open&kind=person&secret=",
    );
    assert_eq!(
        status,
        Status::UnprocessableEntity,
        "secret is shown and required"
    );
    let (status, _) = post(
        &client,
        "/admin/guarded-posts/2",
        "title=Open&kind=business&company=New+Co&secret=new+secret",
    );
    assert_eq!(status, Status::SeeOther);
    assert_eq!(stored(2, "company").as_deref(), Some("New Co"));
    assert_eq!(stored(2, "secret").as_deref(), Some("new secret"));

    // On create, a field hidden on create or from the user is not written.
    let (status, _) = post(
        &client,
        "/admin/guarded-posts",
        "title=New&kind=person&company=Evil&secret=s&internal=hacked&slug=hacked",
    );
    assert_eq!(status, Status::SeeOther);
    for column in ["company", "internal", "slug"] {
        assert_eq!(stored(3, column), None, "{column}");
    }

    unregister_runtime("guarded-posts").expect("unregister");
}

#[test]
fn the_panel_script_sends_a_live_change_to_the_form_live_url() {
    let (_guard, client) = reset_posts("script-posts");
    let body = client
        .get("/admin/script-posts/new")
        .dispatch()
        .into_string()
        .unwrap();
    let src = body
        .split(r#"<script type="module" src=""#)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("the panel's script");
    let script = client
        .get(src)
        .dispatch()
        .into_string()
        .expect("the script");
    for needle in [
        "[data-ignite-live]",
        "form[data-ignite-live-url]",
        "igniteLiveUrl",
        "[data-ignite-fields]",
    ] {
        assert!(script.contains(needle), "{needle} in {script}");
    }
    unregister_runtime("script-posts").expect("unregister");
}

/// The widgets of the shared table, whose note shows only when the price is above 2.
struct PricedNotes;

impl IgniteResource for PricedNotes {
    type Record = WidgetRow;
    type Input = WidgetInput;

    const SLUG: &'static str = "priced-notes";
    const TITLE: &'static str = "Priced notes";

    fn fields() -> &'static [Field] {
        Widgets::fields()
    }

    fn columns() -> &'static [Column] {
        Widgets::columns()
    }

    fn conditions() -> FieldConditions {
        FieldConditions::new()
            .live("price")
            .visible("note", |form| {
                form.get("price")
                    .and_then(|price| price.trim().parse::<i32>().ok())
                    .is_some_and(|price| price > 2)
            })
    }

    async fn list(db: &Database, query: &ListQuery) -> Result<Page<WidgetRow>, OrbitError> {
        Widgets::list(db, query).await
    }

    async fn find(db: &Database, id: i64) -> Result<Option<WidgetRow>, OrbitError> {
        Widgets::find(db, id).await
    }

    async fn create(db: &Database, input: WidgetInput) -> Result<WidgetRow, OrbitError> {
        Widgets::create(db, input).await
    }

    async fn update(db: &Database, id: i64, input: WidgetInput) -> Result<WidgetRow, OrbitError> {
        Widgets::update(db, id, input).await
    }

    async fn delete(db: &Database, id: i64) -> Result<u64, OrbitError> {
        Widgets::delete(db, id).await
    }

    fn validate(form: &FormInput) -> Result<WidgetInput, FieldErrors> {
        Widgets::validate(form)
    }
}

#[test]
fn a_static_resource_hides_a_field_by_its_conditions() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.resource::<PricedNotes>());

    // Banana (2) costs 2: its note is hidden, and a request cannot change it.
    let body = client
        .get("/admin/priced-notes/2/edit")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(!body.contains(r#"name="note""#), "{body}");
    assert!(body.contains(r#"data-ignite-field="price" data-ignite-live"#));
    let (status, _) = post(
        &client,
        "/admin/priced-notes/2",
        "name=Banana&price=2&note=Hacked",
    );
    assert_eq!(status, Status::SeeOther);
    let note = |id: i64| {
        let t = test_db();
        t.count(
            "SELECT COUNT(*) FROM widgets WHERE id = ? AND note = 'Yellow'",
            &[id.into()],
        )
    };
    assert_eq!(note(2), 1, "the stored note is kept");

    // At a price of 3 the note shows, and is written.
    let (status, _) = post(
        &client,
        "/admin/priced-notes/2",
        "name=Banana&price=3&note=Ripe",
    );
    assert_eq!(status, Status::SeeOther);
    assert_eq!(note(2), 0);
    assert_eq!(
        test_db().count(
            "SELECT COUNT(*) FROM widgets WHERE id = 2 AND note = 'Ripe'",
            &[]
        ),
        1
    );
}
