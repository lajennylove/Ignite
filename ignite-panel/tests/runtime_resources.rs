//! Runtime resources through the panel: a resource registered while the panel serves shows
//! in the navigation and has a list, a create form, an edit form and a delete action; when it
//! is unregistered it leaves the navigation and its routes answer 404; and a slug that a
//! static resource or the panel uses is refused. Runs against the test database.
//!
//! The registry is shared by the whole test binary. Each test holds the lock of `reset` and
//! uses a slug of its own.

mod common;

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ColumnDef, Database, FieldDef, FieldKindDef, ListQuery,
    Page, Row, RuntimeData, RuntimeError, RuntimeFuture, RuntimeResource,
};
use ignite_panel::authz;
use orbit_orm::{Error as OrbitError, Model, OrbitModel, Value};
use rocket::http::{ContentType, Status};
use serde_json::json;

/// A row of the `runtime_books` table, which backs the runtime resources of these tests.
#[derive(Debug, Clone, OrbitModel)]
#[orbit(table = "runtime_books")]
struct BookRow {
    id: i64,
    #[orbit(fillable)]
    title: String,
    #[orbit(fillable)]
    pages: Option<i64>,
}

impl BookRow {
    fn into_row(self) -> Row {
        let mut row = Row::new();
        row.insert("id".into(), json!(self.id));
        row.insert("title".into(), json!(self.title));
        row.insert("pages".into(), json!(self.pages));
        row
    }
}

/// The values of a validated row, as Orbit binds them.
fn values(input: &Row) -> Vec<(&'static str, Value)> {
    let mut values = Vec::new();
    if let Some(title) = input.get("title").and_then(|v| v.as_str()) {
        values.push(("title", title.to_string().into()));
    }
    if let Some(pages) = input.get("pages") {
        values.push(("pages", pages.as_i64().into()));
    }
    values
}

/// The app's data source: rows of `runtime_books`, as a CMS would back a custom post type.
struct Books;

impl RuntimeData for Books {
    fn list<'a>(&'a self, db: &'a Database, query: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async move {
            let total = BookRow::query().count(db).await?;
            let rows = BookRow::query()
                .order_by("id", orbit_orm::Direction::Asc)
                .limit(query.per_page as u64)
                .offset(query.offset() as u64)
                .get(db)
                .await?;
            Ok(Page {
                rows: rows.into_iter().map(BookRow::into_row).collect(),
                total,
            })
        })
    }

    fn find<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(async move { Ok(BookRow::query().find(db, id).await?.map(BookRow::into_row)) })
    }

    fn create<'a>(&'a self, db: &'a Database, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move {
            let id = BookRow::query().insert(db, &values(&input)).await?;
            self.find(db, id as i64).await?.ok_or(OrbitError::NotFound {
                table: "runtime_books",
            })
        })
    }

    fn update<'a>(&'a self, db: &'a Database, id: i64, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move {
            BookRow::query()
                .where_eq("id", id)
                .update(db, &values(&input))
                .await?;
            self.find(db, id).await?.ok_or(OrbitError::NotFound {
                table: "runtime_books",
            })
        })
    }

    fn delete<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async move { BookRow::query().where_eq("id", id).delete(db).await })
    }
}

/// A books resource at `slug`, with a required title and an optional page count.
fn books(slug: &str) -> RuntimeResource {
    RuntimeResource::new(slug, "Books", "Book", Books)
        .field(FieldDef {
            name: "title".into(),
            label: "Title".into(),
            kind: FieldKindDef::Text,
            required: true,
        })
        .field(FieldDef {
            name: "pages".into(),
            label: "Pages".into(),
            kind: FieldKindDef::Number,
            required: false,
        })
        .column(ColumnDef {
            key: "title".into(),
            label: "Title".into(),
            sortable: false,
            searchable: false,
        })
        .column(ColumnDef {
            key: "pages".into(),
            label: "Pages".into(),
            sortable: false,
            searchable: false,
        })
}

/// Resets the shared tables and makes `runtime_books` with one row, "Dune".
fn reset_books() -> std::sync::MutexGuard<'static, ()> {
    let guard = reset();
    let t = test_db();
    t.exec("DROP TABLE IF EXISTS runtime_books");
    t.exec(
        "CREATE TABLE runtime_books (id BIGINT AUTO_INCREMENT PRIMARY KEY, \
         title VARCHAR(255) NOT NULL, pages BIGINT NULL)",
    );
    t.exec("INSERT INTO runtime_books (title, pages) VALUES ('Dune', 412)");
    guard
}

fn count_books() -> i64 {
    test_db().count("SELECT COUNT(*) FROM runtime_books", &[])
}

#[test]
fn a_resource_registered_while_serving_has_navigation_list_forms_and_delete() {
    let _guard = reset_books();
    let client = signed_in(None);
    let home = client.get("/admin").dispatch().into_string().unwrap();
    assert!(
        !home.contains(r#"href="/admin/books""#),
        "not yet registered"
    );

    register_runtime(books("books")).expect("register");

    // Navigation, on the next request.
    let home = client.get("/admin").dispatch().into_string().unwrap();
    assert!(home.contains(r#"href="/admin/books""#), "{home}");
    assert!(home.contains("Books"));

    // The list.
    let response = client.get("/admin/books").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_string().unwrap();
    assert!(body.contains("Dune"), "{body}");
    assert!(body.contains("412"));

    // The create form, an invalid save and a valid one.
    let body = client
        .get("/admin/books/new")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.contains(r#"name="title""#));
    assert!(body.contains(r#"action="/admin/books""#));
    assert!(body.contains("New Book"));

    let response = client
        .post("/admin/books")
        .header(ContentType::Form)
        .body(with_token(&client, "title=&pages=many"))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    let body = response.into_string().unwrap();
    assert!(body.contains("Title is required."));
    assert!(body.contains("Pages must be a whole number."));
    assert_eq!(count_books(), 1);

    let response = client
        .post("/admin/books")
        .header(ContentType::Form)
        .body(with_token(&client, "title=Emma&pages=300"))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin/books"));
    assert_eq!(count_books(), 2);

    // The edit form and its save.
    let body = client
        .get("/admin/books/1/edit")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.contains(r#"value="Dune""#), "{body}");
    assert!(body.contains(r#"value="412""#));
    assert!(body.contains(r#"action="/admin/books/1""#));

    let response = client
        .post("/admin/books/1")
        .header(ContentType::Form)
        .body(with_token(&client, "title=Dune+Messiah&pages=256"))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    let t = test_db();
    assert_eq!(
        t.count(
            "SELECT COUNT(*) FROM runtime_books WHERE id = 1 AND title = 'Dune Messiah' AND pages = 256",
            &[]
        ),
        1
    );

    // The delete action.
    let response = client
        .post("/admin/books/1/delete")
        .header(ContentType::Form)
        .body(with_token(&client, ""))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(count_books(), 1);

    unregister_runtime("books").expect("unregister");
}

#[test]
fn an_unregistered_resource_leaves_the_navigation_and_its_routes_are_404() {
    let _guard = reset_books();
    let client = signed_in(None);
    register_runtime(books("gone-books")).expect("register");
    assert_eq!(
        client.get("/admin/gone-books").dispatch().status(),
        Status::Ok
    );

    unregister_runtime("gone-books").expect("unregister");

    let home = client.get("/admin").dispatch().into_string().unwrap();
    assert!(!home.contains(r#"href="/admin/gone-books""#), "{home}");
    for path in [
        "/admin/gone-books",
        "/admin/gone-books/new",
        "/admin/gone-books/1/edit",
    ] {
        assert_eq!(
            client.get(path).dispatch().status(),
            Status::NotFound,
            "{path}"
        );
    }
    let response = client
        .post("/admin/gone-books/1/delete")
        .header(ContentType::Form)
        .body(with_token(&client, ""))
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);
    assert_eq!(count_books(), 1, "nothing was deleted");
}

#[test]
fn a_slug_of_a_static_resource_or_a_panel_page_is_refused() {
    let _guard = reset_books();
    // Building the panel reserves the slugs of its static resources and of its pages.
    let _client = client(None);

    for slug in ["widgets", "login", "profile"] {
        let error = register_runtime(books(slug)).unwrap_err();
        assert_eq!(error, RuntimeError::SlugTaken(slug.to_string()));
    }
    assert_eq!(
        register_runtime(books("widgets")).unwrap_err().to_string(),
        "the slug `widgets` is already used by another Ignite resource"
    );
}

#[test]
fn a_runtime_resource_gets_its_permissions_and_obeys_them() {
    let _guard = reset_books();
    let admin = signed_in(None);
    register_runtime(books("ruled-books")).expect("register");
    // The next request registers the permission rows.
    admin.get("/admin").dispatch();
    let t = test_db();
    assert_eq!(
        t.count(
            "SELECT COUNT(*) FROM permissions WHERE name LIKE ?",
            &["ruled-books.%".into()],
        ),
        5,
        "viewAny, view, create, update and delete"
    );

    // A user who may only view sees the list, and is refused the create form.
    t.run(async {
        ignite_panel::users::create_user(&t.db, "Reader", "reader@example.test", PASSWORD)
            .await
            .expect("create user");
        authz::create_role(&t.db, "reader").await.expect("role");
        authz::grant_permission(&t.db, "reader", "ruled-books.viewAny")
            .await
            .expect("grant");
        authz::assign_role(&t.db, "reader@example.test", "reader")
            .await
            .expect("assign");
    });
    let reader = signed_in_as(None, "reader@example.test", PASSWORD);
    let home = reader.get("/admin").dispatch().into_string().unwrap();
    assert!(home.contains(r#"href="/admin/ruled-books""#));
    assert!(!home.contains(r#"href="/admin/widgets""#));
    assert_eq!(
        reader.get("/admin/ruled-books").dispatch().status(),
        Status::Ok
    );
    assert_eq!(
        reader.get("/admin/ruled-books/new").dispatch().status(),
        Status::Forbidden
    );

    unregister_runtime("ruled-books").expect("unregister");
}

#[test]
fn the_app_creates_the_permissions_when_it_registers_so_a_grant_works_at_once() {
    let _guard = reset_books();
    // The panel is running, but no request is made after the registration.
    let _client = client(None);
    let t = test_db();

    register_runtime(books("granted-books")).expect("register");
    t.run(ignite_panel::register_runtime_permissions(
        &t.db,
        "granted-books",
    ))
    .expect("register permissions");
    t.run(ignite_panel::register_runtime_permissions(
        &t.db,
        "granted-books",
    ))
    .expect("again changes nothing");

    t.run(async {
        authz::create_role(&t.db, "editor").await.expect("role");
        authz::grant_permission(&t.db, "editor", "granted-books.create")
            .await
            .expect("the permission exists right after registration");
    });
    assert_eq!(
        t.count(
            "SELECT COUNT(*) FROM permissions WHERE name LIKE ?",
            &["granted-books.%".into()],
        ),
        5
    );

    unregister_runtime("granted-books").expect("unregister");
}

#[test]
fn permissions_are_not_created_for_a_slug_that_is_not_registered() {
    let _guard = reset_books();
    let _client = client(None);
    let t = test_db();

    let error = t
        .run(ignite_panel::register_runtime_permissions(
            &t.db,
            "missing-books",
        ))
        .unwrap_err();
    assert!(
        matches!(&error, authz::AuthzError::NotFound(what) if what.contains("missing-books")),
        "{error}"
    );
    assert_eq!(
        t.count(
            "SELECT COUNT(*) FROM permissions WHERE name LIKE ?",
            &["missing-books.%".into()],
        ),
        0
    );
}
