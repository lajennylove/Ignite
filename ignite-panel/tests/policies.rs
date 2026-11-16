//! The acting user and resource policies through the panel: a resource's queries are given the
//! signed-in user, so its list and its records are restricted to them (a record the user's
//! query does not find is 404), and a registered policy decides each action for the user and
//! the record (a refusal is 403, and the buttons it refuses are hidden). A super-admin passes
//! every policy, and a resource without a policy is decided by its abilities alone. Runs
//! against the test database.
//!
//! The registries are shared by the whole test binary. Each test holds the lock of `reset`
//! and removes what it registers.

mod common;

use common::*;
use ignite_core::{
    register_policy, register_runtime, unregister_policy, unregister_runtime, ActingUser,
    ColumnDef, Database, FieldDef, FieldKindDef, ListQuery, Page, Policy, Row, RuntimeData,
    RuntimeFuture, RuntimeResource,
};
use ignite_panel::authz;
use orbit_orm::{Error as OrbitError, Model, OrbitModel, Value};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::json;

/// The slug of the posts resource of these tests.
const SLUG: &str = "policy-posts";

/// A row of the `policy_posts` table: a post with an owner and a status.
#[derive(Debug, Clone, OrbitModel)]
#[orbit(table = "policy_posts")]
struct PostRow {
    id: i64,
    #[orbit(fillable)]
    title: String,
    #[orbit(fillable)]
    owner_id: Option<i64>,
    #[orbit(fillable)]
    status: String,
}

impl PostRow {
    fn into_row(self) -> Row {
        let mut row = Row::new();
        row.insert("id".into(), json!(self.id));
        row.insert("title".into(), json!(self.title));
        row.insert("owner_id".into(), json!(self.owner_id));
        row.insert("status".into(), json!(self.status));
        row
    }
}

/// The values of a validated row, as Orbit binds them.
fn values(input: &Row) -> Vec<(&'static str, Value)> {
    ["title", "status"]
        .into_iter()
        .filter_map(|key| {
            let text = input.get(key)?.as_str()?.to_string();
            Some((key, text.into()))
        })
        .collect()
}

/// The owner a user's query is restricted to: none for a super-admin, who sees every post,
/// and the user's own id for anyone else, as an author sees only their posts.
fn owner_scope(user: &ActingUser) -> Option<i64> {
    (!user.is_super_admin()).then_some(user.id())
}

/// The posts: every row without a user, and the user's own rows with one.
struct Posts;

impl Posts {
    async fn rows(db: &Database, owner: Option<i64>) -> Result<Vec<PostRow>, OrbitError> {
        let mut query = PostRow::query().order_by("id", orbit_orm::Direction::Asc);
        if let Some(owner) = owner {
            query = query.where_eq("owner_id", owner);
        }
        query.get(db).await
    }

    async fn find_owned(
        db: &Database,
        owner: Option<i64>,
        id: i64,
    ) -> Result<Option<Row>, OrbitError> {
        let mut query = PostRow::query().where_eq("id", id);
        if let Some(owner) = owner {
            query = query.where_eq("owner_id", owner);
        }
        Ok(query.first(db).await?.map(PostRow::into_row))
    }

    async fn insert(db: &Database, input: &Row, owner: Option<i64>) -> Result<Row, OrbitError> {
        let mut values = values(input);
        values.push(("owner_id", owner.into()));
        let id = PostRow::query().insert(db, &values).await?;
        Posts::find_owned(db, None, id as i64)
            .await?
            .ok_or(OrbitError::NotFound {
                table: "policy_posts",
            })
    }
}

fn page(rows: Vec<PostRow>) -> Page<Row> {
    let total = rows.len() as i64;
    Page {
        rows: rows.into_iter().map(PostRow::into_row).collect(),
        total,
    }
}

impl RuntimeData for Posts {
    fn list<'a>(&'a self, db: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async move { Ok(page(Posts::rows(db, None).await?)) })
    }

    fn find<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(Posts::find_owned(db, None, id))
    }

    fn create<'a>(&'a self, db: &'a Database, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move { Posts::insert(db, &input, None).await })
    }

    fn update<'a>(&'a self, db: &'a Database, id: i64, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move {
            PostRow::query()
                .where_eq("id", id)
                .update(db, &values(&input))
                .await?;
            Posts::find_owned(db, None, id)
                .await?
                .ok_or(OrbitError::NotFound {
                    table: "policy_posts",
                })
        })
    }

    fn delete<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async move { PostRow::query().where_eq("id", id).delete(db).await })
    }

    fn options<'a>(
        &'a self,
        _: &'a Database,
        _: &'a str,
    ) -> RuntimeFuture<'a, Vec<(String, String)>> {
        Box::pin(async {
            Ok(vec![
                ("draft".into(), "Draft".into()),
                ("published".into(), "Published".into()),
            ])
        })
    }

    fn list_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        _: &'a ListQuery,
    ) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async move { Ok(page(Posts::rows(db, owner_scope(user)).await?)) })
    }

    fn find_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        id: i64,
    ) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(Posts::find_owned(db, owner_scope(user), id))
    }

    fn create_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        input: Row,
    ) -> RuntimeFuture<'a, Row> {
        Box::pin(async move { Posts::insert(db, &input, Some(user.id())).await })
    }

    // Only a super-admin may choose "Published", as only an editor may publish.
    fn options_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        field: &'a str,
    ) -> RuntimeFuture<'a, Vec<(String, String)>> {
        Box::pin(async move {
            let mut choices = self.options(db, field).await?;
            if !user.is_super_admin() {
                choices.retain(|(value, _)| value != "published");
            }
            Ok(choices)
        })
    }
}

/// The posts policy: "Blocked" may not see the list, "Nocreate" may not create, and a
/// published post may be neither changed nor deleted.
struct PostPolicy;

fn is_published(record: &Row) -> bool {
    record.get("status").and_then(|v| v.as_str()) == Some("published")
}

impl Policy for PostPolicy {
    fn view_any<'a>(&'a self, _: &'a Database, user: &'a ActingUser) -> RuntimeFuture<'a, bool> {
        let allowed = user.name() != "Blocked";
        Box::pin(async move { Ok(allowed) })
    }

    fn create<'a>(&'a self, _: &'a Database, user: &'a ActingUser) -> RuntimeFuture<'a, bool> {
        let allowed = user.name() != "Nocreate";
        Box::pin(async move { Ok(allowed) })
    }

    fn update<'a>(
        &'a self,
        _: &'a Database,
        _: &'a ActingUser,
        record: &'a Row,
    ) -> RuntimeFuture<'a, bool> {
        let allowed = !is_published(record);
        Box::pin(async move { Ok(allowed) })
    }

    fn delete<'a>(
        &'a self,
        _: &'a Database,
        _: &'a ActingUser,
        record: &'a Row,
    ) -> RuntimeFuture<'a, bool> {
        let allowed = !is_published(record);
        Box::pin(async move { Ok(allowed) })
    }
}

fn posts() -> RuntimeResource {
    RuntimeResource::new(SLUG, "Posts", "Post", Posts)
        .field(FieldDef {
            name: "title".into(),
            label: "Title".into(),
            kind: FieldKindDef::Text,
            required: true,
        })
        .field(FieldDef {
            name: "status".into(),
            label: "Status".into(),
            kind: FieldKindDef::Lookup,
            required: true,
        })
        .column(ColumnDef {
            key: "title".into(),
            label: "Title".into(),
            sortable: false,
            searchable: false,
        })
}

/// The ids of the authors: Ann, Bob, Blocked and Nocreate, each with every ability of the
/// posts resource through the role `authors`.
struct Authors {
    ann: i64,
    bob: i64,
}

/// Resets the shared tables, registers the posts resource and its policy, makes the authors,
/// and seeds four posts: 1 the admin's draft, 2 Ann's draft, 3 Ann's published post and 4
/// Bob's draft. Returns the lock, a client signed in as the admin (a super-admin), and the
/// authors.
fn reset_posts() -> (std::sync::MutexGuard<'static, ()>, Client, Authors) {
    let guard = reset();
    let admin = signed_in(None);
    let t = test_db();
    t.exec("DROP TABLE IF EXISTS policy_posts");
    t.exec(
        "CREATE TABLE policy_posts (id BIGINT AUTO_INCREMENT PRIMARY KEY, \
         title VARCHAR(255) NOT NULL, owner_id BIGINT NULL, status VARCHAR(20) NOT NULL)",
    );
    let _ = unregister_runtime(SLUG);
    register_runtime(posts()).expect("register");
    register_policy(SLUG, PostPolicy);
    let authors = t.run(async {
        ignite_panel::register_runtime_permissions(&t.db, SLUG)
            .await
            .expect("permissions");
        authz::create_role(&t.db, "authors").await.expect("role");
        for action in ["viewAny", "create", "update", "delete"] {
            authz::grant_permission(&t.db, "authors", &format!("{SLUG}.{action}"))
                .await
                .expect("grant");
        }
        let mut ids = Vec::new();
        for name in ["Ann", "Bob", "Blocked", "Nocreate"] {
            let email = format!("{}@example.test", name.to_lowercase());
            let user = ignite_panel::users::create_user(&t.db, name, &email, PASSWORD)
                .await
                .expect("create user");
            authz::assign_role(&t.db, &email, "authors")
                .await
                .expect("assign");
            ids.push(user.id);
        }
        Authors {
            ann: ids[0],
            bob: ids[1],
        }
    });
    let admin_id = t.count("SELECT id FROM users WHERE email = ?", &[EMAIL.into()]);
    for (title, owner, status) in [
        ("Admin draft", admin_id, "draft"),
        ("Ann draft", authors.ann, "draft"),
        ("Ann published", authors.ann, "published"),
        ("Bob draft", authors.bob, "draft"),
    ] {
        t.exec_with(
            "INSERT INTO policy_posts (title, owner_id, status) VALUES (?, ?, ?)",
            &[title.into(), owner.into(), status.into()],
        );
    }
    (guard, admin, authors)
}

/// Removes the posts resource and its policy.
fn tear_down() {
    unregister_runtime(SLUG).expect("unregister");
    assert!(unregister_policy(SLUG));
}

fn sign_in(name: &str) -> Client {
    signed_in_as(
        None,
        &format!("{}@example.test", name.to_lowercase()),
        PASSWORD,
    )
}

fn post(client: &Client, path: &str, body: &str) -> Status {
    client
        .post(path)
        .header(ContentType::Form)
        .body(with_token(client, body))
        .dispatch()
        .status()
}

fn title_of(id: i64) -> Option<String> {
    let t = test_db();
    t.run(PostRow::query().find(&t.db, id))
        .expect("read")
        .map(|row| row.title)
}

#[test]
fn an_author_lists_only_their_own_records_and_the_others_are_404() {
    let (_guard, _admin, _authors) = reset_posts();
    let ann = sign_in("Ann");

    let response = ann.get(format!("/admin/{SLUG}")).dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_string().unwrap();
    assert!(body.contains("Ann draft"), "{body}");
    assert!(body.contains("Ann published"));
    assert!(
        !body.contains("Bob draft"),
        "another author's post is not listed"
    );
    assert!(!body.contains("Admin draft"));

    // Bob's post is not in Ann's query: its edit form, its save and its delete are 404.
    assert_eq!(
        ann.get(format!("/admin/{SLUG}/4/edit")).dispatch().status(),
        Status::NotFound
    );
    assert_eq!(
        post(
            &ann,
            &format!("/admin/{SLUG}/4"),
            "title=Taken&status=draft"
        ),
        Status::NotFound
    );
    assert_eq!(
        post(&ann, &format!("/admin/{SLUG}/4/delete"), ""),
        Status::NotFound
    );
    assert_eq!(title_of(4).as_deref(), Some("Bob draft"), "nothing changed");

    tear_down();
}

#[test]
fn a_policy_refuses_an_action_on_a_record_with_403_and_hides_its_buttons() {
    let (_guard, _admin, _authors) = reset_posts();
    let ann = sign_in("Ann");

    // The draft shows its Edit and Delete buttons; the published post shows neither.
    let body = ann
        .get(format!("/admin/{SLUG}"))
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        body.contains(&format!(r#"href="/admin/{SLUG}/2/edit""#)),
        "{body}"
    );
    assert!(body.contains(&format!(r#"action="/admin/{SLUG}/2/delete""#)));
    assert!(!body.contains(&format!(r#"href="/admin/{SLUG}/3/edit""#)));
    assert!(!body.contains(&format!(r#"action="/admin/{SLUG}/3/delete""#)));

    // The published post: the edit form, its save and the delete are 403, and nothing changes.
    assert_eq!(
        ann.get(format!("/admin/{SLUG}/3/edit")).dispatch().status(),
        Status::Forbidden
    );
    assert_eq!(
        post(
            &ann,
            &format!("/admin/{SLUG}/3"),
            "title=Changed&status=draft"
        ),
        Status::Forbidden
    );
    assert_eq!(
        post(&ann, &format!("/admin/{SLUG}/3/delete"), ""),
        Status::Forbidden
    );
    assert_eq!(title_of(3).as_deref(), Some("Ann published"));

    // The draft: the edit form, its save and the delete are allowed.
    assert_eq!(
        ann.get(format!("/admin/{SLUG}/2/edit")).dispatch().status(),
        Status::Ok
    );
    assert_eq!(
        post(
            &ann,
            &format!("/admin/{SLUG}/2"),
            "title=Ann+edited&status=draft"
        ),
        Status::SeeOther
    );
    assert_eq!(title_of(2).as_deref(), Some("Ann edited"));
    assert_eq!(
        post(&ann, &format!("/admin/{SLUG}/2/delete"), ""),
        Status::SeeOther
    );
    assert_eq!(title_of(2), None);

    tear_down();
}

#[test]
fn a_created_record_belongs_to_the_acting_user_and_the_choices_are_the_users() {
    let (_guard, _admin, authors) = reset_posts();
    let ann = sign_in("Ann");

    // "Published" is not one of Ann's choices: the form leaves it out, and a save with it is
    // refused as a value that is not among the options.
    let body = ann
        .get(format!("/admin/{SLUG}/new"))
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.contains(r#"value="draft""#), "{body}");
    assert!(!body.contains(r#"value="published""#));
    let response = ann
        .post(format!("/admin/{SLUG}"))
        .header(ContentType::Form)
        .body(with_token(&ann, "title=Mine&status=published"))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("Choose one of the options."));

    assert_eq!(
        post(&ann, &format!("/admin/{SLUG}"), "title=Mine&status=draft"),
        Status::SeeOther
    );
    let t = test_db();
    assert_eq!(
        t.count(
            "SELECT owner_id FROM policy_posts WHERE title = 'Mine'",
            &[]
        ),
        authors.ann,
        "create_for made Ann the owner"
    );

    tear_down();
}

#[test]
fn view_any_and_create_refused_by_the_policy_leave_the_navigation_and_answer_403() {
    let (_guard, _admin, _authors) = reset_posts();

    let blocked = sign_in("Blocked");
    let home = blocked.get("/admin").dispatch().into_string().unwrap();
    assert!(
        !home.contains(&format!(r#"href="/admin/{SLUG}""#)),
        "{home}"
    );
    assert_eq!(
        blocked.get(format!("/admin/{SLUG}")).dispatch().status(),
        Status::Forbidden
    );

    let nocreate = sign_in("Nocreate");
    let home = nocreate.get("/admin").dispatch().into_string().unwrap();
    assert!(home.contains(&format!(r#"href="/admin/{SLUG}""#)), "{home}");
    let list = nocreate.get(format!("/admin/{SLUG}")).dispatch();
    assert_eq!(list.status(), Status::Ok);
    assert!(!list
        .into_string()
        .unwrap()
        .contains(&format!(r#"href="/admin/{SLUG}/new""#)));
    assert_eq!(
        nocreate
            .get(format!("/admin/{SLUG}/new"))
            .dispatch()
            .status(),
        Status::Forbidden
    );
    assert_eq!(
        post(
            &nocreate,
            &format!("/admin/{SLUG}"),
            "title=No&status=draft"
        ),
        Status::Forbidden
    );

    tear_down();
}

#[test]
fn a_super_admin_passes_every_policy_and_sees_every_record() {
    let (_guard, admin, _authors) = reset_posts();

    let body = admin
        .get(format!("/admin/{SLUG}"))
        .dispatch()
        .into_string()
        .unwrap();
    for title in ["Admin draft", "Ann draft", "Ann published", "Bob draft"] {
        assert!(body.contains(title), "{title}: {body}");
    }
    assert!(body.contains(&format!(r#"href="/admin/{SLUG}/3/edit""#)));

    let form = admin.get(format!("/admin/{SLUG}/3/edit")).dispatch();
    assert_eq!(form.status(), Status::Ok);
    assert!(form.into_string().unwrap().contains(r#"value="published""#));
    assert_eq!(
        post(
            &admin,
            &format!("/admin/{SLUG}/3"),
            "title=Edited+by+admin&status=published"
        ),
        Status::SeeOther
    );
    assert_eq!(title_of(3).as_deref(), Some("Edited by admin"));
    assert_eq!(
        post(&admin, &format!("/admin/{SLUG}/3/delete"), ""),
        Status::SeeOther
    );
    assert_eq!(title_of(3), None);

    tear_down();
}

/// The widgets policy: Apple may not be deleted.
struct KeepApple;

impl Policy for KeepApple {
    fn delete<'a>(
        &'a self,
        _: &'a Database,
        _: &'a ActingUser,
        record: &'a Row,
    ) -> RuntimeFuture<'a, bool> {
        let allowed = record.get("name").and_then(|v| v.as_str()) != Some("Apple");
        Box::pin(async move { Ok(allowed) })
    }
}

// A static resource follows its policy too, and without one its abilities alone decide again.
#[test]
fn a_static_resource_follows_its_policy_and_without_one_its_abilities_decide() {
    let _guard = reset();
    let _admin = signed_in(None);
    let t = test_db();
    t.run(async {
        ignite_panel::users::create_user(&t.db, "Wendy", "wendy@example.test", PASSWORD)
            .await
            .expect("create user");
        authz::create_role(&t.db, "pruners").await.expect("role");
        for ability in ["widgets.viewAny", "widgets.delete"] {
            authz::grant_permission(&t.db, "pruners", ability)
                .await
                .expect("grant");
        }
        authz::assign_role(&t.db, "wendy@example.test", "pruners")
            .await
            .expect("assign");
    });
    register_policy("widgets", KeepApple);
    let wendy = signed_in_as(None, "wendy@example.test", PASSWORD);

    let body = wendy
        .get("/admin/widgets")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        !body.contains(r#"action="/admin/widgets/1/delete""#),
        "{body}"
    );
    assert!(body.contains(r#"action="/admin/widgets/2/delete""#));
    assert_eq!(
        post(&wendy, "/admin/widgets/1/delete", ""),
        Status::Forbidden
    );
    assert_eq!(count_widgets(), 3);
    assert_eq!(
        post(&wendy, "/admin/widgets/2/delete", ""),
        Status::SeeOther
    );
    assert_eq!(count_widgets(), 2);

    assert!(unregister_policy("widgets"));
    assert_eq!(
        post(&wendy, "/admin/widgets/1/delete", ""),
        Status::SeeOther
    );
    assert_eq!(count_widgets(), 1);
}

/// A policy whose answers fail: the update for everyone, and the list for Bob.
struct Failing;

impl Policy for Failing {
    fn update<'a>(
        &'a self,
        _: &'a Database,
        _: &'a ActingUser,
        _: &'a Row,
    ) -> RuntimeFuture<'a, bool> {
        Box::pin(async {
            Err(OrbitError::NotFound {
                table: "policy_failure",
            })
        })
    }

    fn view_any<'a>(&'a self, _: &'a Database, user: &'a ActingUser) -> RuntimeFuture<'a, bool> {
        let fails = user.name() == "Bob";
        Box::pin(async move {
            if fails {
                Err(OrbitError::UniqueViolation {
                    index: Some("policy_failure".into()),
                })
            } else {
                Ok(true)
            }
        })
    }
}

// A failed policy is a server error, not a refusal, a missing record or a sign-out.
#[test]
fn a_failed_policy_is_a_server_error() {
    let (_guard, _admin, _authors) = reset_posts();
    register_policy(SLUG, Failing);

    let ann = sign_in("Ann");
    assert_eq!(
        ann.get(format!("/admin/{SLUG}/2/edit")).dispatch().status(),
        Status::InternalServerError
    );
    // The list's view_any is asked on every signed-in request, so Bob's dashboard fails too.
    let bob = sign_in("Bob");
    assert_eq!(
        bob.get("/admin").dispatch().status(),
        Status::InternalServerError
    );

    tear_down();
}

/// A data source that does not depend on the user: it writes only the methods without one.
struct Fixed;

impl RuntimeData for Fixed {
    fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async {
            Ok(Page {
                rows: vec![Row::from_iter([("id".to_string(), json!(1))])],
                total: 1,
            })
        })
    }
    fn find<'a>(&'a self, _: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(async move { Ok(Some(Row::from_iter([("id".to_string(), json!(id))]))) })
    }
    fn create<'a>(&'a self, _: &'a Database, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move { Ok(input) })
    }
    fn update<'a>(&'a self, _: &'a Database, id: i64, mut input: Row) -> RuntimeFuture<'a, Row> {
        input.insert("id".into(), json!(id));
        Box::pin(async move { Ok(input) })
    }
    fn delete<'a>(&'a self, _: &'a Database, id: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async move { Ok(id as u64) })
    }
    fn options<'a>(
        &'a self,
        _: &'a Database,
        field: &'a str,
    ) -> RuntimeFuture<'a, Vec<(String, String)>> {
        Box::pin(async move { Ok(vec![(field.to_string(), "Only".to_string())]) })
    }
}

/// A policy that writes no method.
struct Open;

impl Policy for Open {}

// The `*_for` methods a source does not write call the methods without the user, and a policy
// allows every action it does not write, so a resource that sets neither behaves as in 0.5.0.
#[test]
fn the_defaults_pass_through_without_the_user_and_allow_every_action() {
    let t = test_db();
    let user = ActingUser::new(42, "Ann", "ann@example.test");
    let query = ListQuery {
        page: 1,
        per_page: 20,
        search: None,
        sort: None,
    };
    let row = Row::from_iter([("title".to_string(), json!("Dune"))]);
    t.run(async {
        let data = Fixed;
        let listed = data.list_for(&t.db, &user, &query).await.expect("list");
        assert_eq!((listed.rows.len(), listed.total), (1, 1));
        let found = data.find_for(&t.db, &user, 7).await.expect("find");
        assert_eq!(found.and_then(|r| r.get("id").cloned()), Some(json!(7)));
        let created = data
            .create_for(&t.db, &user, row.clone())
            .await
            .expect("create");
        assert_eq!(created, row);
        let updated = data
            .update_for(&t.db, &user, 9, row.clone())
            .await
            .expect("update");
        assert_eq!(updated.get("id"), Some(&json!(9)));
        assert_eq!(data.delete_for(&t.db, &user, 3).await.expect("delete"), 3);
        assert_eq!(
            data.options_for(&t.db, &user, "status")
                .await
                .expect("options"),
            vec![("status".to_string(), "Only".to_string())]
        );

        let policy = Open;
        assert!(policy.view_any(&t.db, &user).await.expect("view_any"));
        assert!(policy.view(&t.db, &user, &row).await.expect("view"));
        assert!(policy.create(&t.db, &user).await.expect("create"));
        assert!(policy.update(&t.db, &user, &row).await.expect("update"));
        assert!(policy.delete(&t.db, &user, &row).await.expect("delete"));
    });
}
