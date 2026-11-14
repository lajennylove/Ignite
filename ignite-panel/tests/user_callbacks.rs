//! Tests for the user lifecycle callbacks, `Ignite::on_user_created` and
//! `Ignite::on_user_deleting` / `on_user_deleted`: they run for a user created or deleted from the panel's form,
//! from `users::create_user` and `users::delete_user`, and from the app's own `ignite` command
//! (`Ignite::call_cli`); a callback that fails rolls the change back. They run against the
//! test database.
//!
//! The app's command is run in a child process: the test binary runs itself with only the
//! `cli_child` test, which builds the app's `Ignite` and hands it the arguments, as an app's
//! own `src/bin/ignite.rs` does with `run_cli`.

mod common;

use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::process::{Command, Stdio};

use common::*;
use ignite_core::{
    Column, Field, FieldErrors, FieldKind, FormInput, IgniteResource, ListQuery, Page,
};
use ignite_panel::{users, Ignite};
use orbit_orm::__sqlx as sqlx;
use orbit_orm::{raw, Database, Error as OrbitError, Model, OrbitModel, Transaction};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde::Serialize;

/// The table the test's callbacks write to, one row for each call, in call order.
const LOG_TABLE: &str = "CREATE TABLE IF NOT EXISTS user_callback_log (\
     id INT AUTO_INCREMENT PRIMARY KEY, user_id BIGINT NOT NULL, event VARCHAR(64) NOT NULL)";

/// A user with this in their email is refused by the second created callback.
const REFUSE_CREATE: &str = "refuse-create";
/// A user with this in their email is refused by the deleting callback.
const REFUSE_DELETING: &str = "refuse-deleting";

/// The table that marks the users the deleted callback refuses. The deleted callback cannot
/// read the user's email, because the row is gone by then.
const REFUSE_TABLE: &str =
    "CREATE TABLE IF NOT EXISTS user_callback_refuse_deleted (user_id BIGINT PRIMARY KEY)";

/// The environment variable that makes `cli_child` run the app's command with its arguments,
/// separated by the unit separator.
const CHILD_ARGS: &str = "IGNITE_TEST_CLI_ARGS";

/// A row of the `users` table, as the test's Users resource lists it.
#[derive(Debug, Clone, OrbitModel, Serialize)]
#[orbit(table = "users")]
struct UserRow {
    id: i64,
    #[orbit(fillable)]
    name: String,
    #[orbit(fillable)]
    email: String,
}

/// The values the Users form writes.
struct UserInput {
    name: String,
    email: String,
    password: String,
}

const NAME: Field = Field {
    name: "name",
    label: "Name",
    kind: FieldKind::Text,
    required: true,
};
const EMAIL_FIELD: Field = Field {
    name: "email",
    label: "Email",
    kind: FieldKind::Text,
    required: true,
};
const PASSWORD_FIELD: Field = Field {
    name: "password",
    label: "Password",
    kind: FieldKind::Password,
    required: true,
};

/// An app's Users resource, the way an app writes one: it creates and deletes through the
/// panel's `users` functions, so the callbacks run.
struct Users;

impl IgniteResource for Users {
    type Record = UserRow;
    type Input = UserInput;

    const SLUG: &'static str = "users";
    const TITLE: &'static str = "Users";
    const SINGULAR: &'static str = "User";

    fn fields() -> &'static [Field] {
        &[NAME, EMAIL_FIELD, PASSWORD_FIELD]
    }

    fn columns() -> &'static [Column] {
        &[Column {
            key: "email",
            label: "Email",
            sortable: false,
            searchable: false,
        }]
    }

    async fn list(db: &Database, query: &ListQuery) -> Result<Page<UserRow>, OrbitError> {
        let total = UserRow::query().count(db).await?;
        let rows = UserRow::query()
            .limit(query.per_page as u64)
            .offset(query.offset() as u64)
            .get(db)
            .await?;
        Ok(Page { rows, total })
    }

    async fn find(db: &Database, id: i64) -> Result<Option<UserRow>, OrbitError> {
        UserRow::query().find(db, id).await
    }

    async fn create(db: &Database, input: UserInput) -> Result<UserRow, OrbitError> {
        let user = users::create_user(db, &input.name, &input.email, &input.password).await?;
        Ok(UserRow {
            id: user.id,
            name: user.name,
            email: user.email,
        })
    }

    async fn update(db: &Database, id: i64, input: UserInput) -> Result<UserRow, OrbitError> {
        users::update_profile(db, id, &input.name, &input.email, "{}").await?;
        Self::find(db, id)
            .await?
            .ok_or(OrbitError::NotFound { table: "users" })
    }

    async fn delete(db: &Database, id: i64) -> Result<u64, OrbitError> {
        users::delete_user(db, id).await
    }

    fn validate(form: &FormInput) -> Result<UserInput, FieldErrors> {
        let mut errors = FieldErrors::new();
        let name = NAME.check_required(form, &mut errors).map(str::to_string);
        let email = EMAIL_FIELD
            .check_required(form, &mut errors)
            .map(str::to_string);
        let password = PASSWORD_FIELD
            .check_required(form, &mut errors)
            .map(str::to_string);
        match (name, email, password) {
            (Some(name), Some(email), Some(password)) if errors.is_empty() => Ok(UserInput {
                name,
                email,
                password,
            }),
            _ => Err(errors),
        }
    }
}

/// The boxed future a callback returns.
type CallbackFuture<'t> = Pin<Box<dyn Future<Output = Result<(), OrbitError>> + Send + 't>>;

/// Writes one row of the log, in the callback's transaction.
async fn log(tx: &mut Transaction, user_id: i64, event: &str) -> Result<(), OrbitError> {
    raw::execute(
        &mut *tx,
        "INSERT INTO user_callback_log (user_id, event) VALUES (?, ?)",
        &[user_id.into(), event.into()],
    )
    .await?;
    Ok(())
}

/// The email of the user with `id`, read in the callback's transaction.
async fn email_of(tx: &mut Transaction, id: i64) -> Result<String, OrbitError> {
    users::find_by_id(&mut *tx, id)
        .await?
        .map(|user| user.email)
        .ok_or(OrbitError::NotFound { table: "users" })
}

/// An error the app's callback returns to refuse the change.
fn refused(what: &str) -> OrbitError {
    OrbitError::Database(sqlx::Error::Protocol(format!("the app refused to {what}")))
}

fn first_created(id: i64, tx: &mut Transaction) -> CallbackFuture<'_> {
    Box::pin(async move { log(tx, id, "created:first").await })
}

/// Runs before the delete: the user's row is still there, so its email can be read.
fn deleting(id: i64, tx: &mut Transaction) -> CallbackFuture<'_> {
    Box::pin(async move {
        let email = email_of(tx, id).await?;
        log(tx, id, "deleting").await?;
        if email.contains(REFUSE_DELETING) {
            return Err(refused("start deleting this user"));
        }
        Ok(())
    })
}

/// Runs after the delete: it logs whether the row is gone inside the transaction.
fn deleted(id: i64, tx: &mut Transaction) -> CallbackFuture<'_> {
    Box::pin(async move {
        let gone = users::find_by_id(&mut *tx, id).await?.is_none();
        let event = if gone {
            "deleted:row-gone"
        } else {
            "deleted:row-present"
        };
        log(tx, id, event).await?;
        let marked: Vec<(i64,)> = raw::fetch_all(
            &mut *tx,
            "SELECT user_id FROM user_callback_refuse_deleted WHERE user_id = ?",
            &[id.into()],
        )
        .await?;
        if !marked.is_empty() {
            return Err(refused("finish deleting this user"));
        }
        Ok(())
    })
}

/// The app: the Users resource and its callbacks, registered in order. The second callback
/// is a closure, the way an app usually writes one.
fn app() -> Ignite {
    Ignite::new()
        .resource::<Users>()
        .on_user_created(first_created)
        .on_user_created(|id, tx| {
            Box::pin(async move {
                if email_of(tx, id).await?.contains(REFUSE_CREATE) {
                    return Err(refused("create this user"));
                }
                log(tx, id, "created:second").await
            })
        })
        .on_user_deleted(deleted)
        .on_user_deleting(deleting)
}

/// Resets the database and empties the log. The guard keeps the other tests out.
fn reset_with_log() -> (std::sync::MutexGuard<'static, ()>, TestDb) {
    let guard = reset();
    let conn = test_db();
    conn.exec(LOG_TABLE);
    conn.exec(REFUSE_TABLE);
    conn.exec("DELETE FROM user_callback_log");
    conn.exec("DELETE FROM user_callback_refuse_deleted");
    (guard, conn)
}

/// The events logged for the user with `user_id`, in call order.
fn events(conn: &TestDb, user_id: i64) -> Vec<String> {
    let rows: Vec<(String,)> = conn
        .run(raw::fetch_all(
            &conn.db,
            "SELECT event FROM user_callback_log WHERE user_id = ? ORDER BY id",
            &[user_id.into()],
        ))
        .expect("read the log");
    rows.into_iter().map(|(event,)| event).collect()
}

fn log_rows(conn: &TestDb) -> i64 {
    conn.count("SELECT COUNT(*) FROM user_callback_log", &[])
}

fn find(conn: &TestDb, email: &str) -> Option<users::User> {
    conn.run(users::find_by_email(&conn.db, email))
        .expect("read users")
}

/// A client for the app, signed in as the test user, a super-admin.
fn signed_in_app() -> Client {
    ensure_secret();
    let client = Client::tracked(rocket::build().attach(app().fairing())).expect("valid rocket");
    client.get("/admin/login").dispatch();
    let body = with_token(&client, &format!("email={EMAIL}&password={PASSWORD}"));
    let status = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(body)
        .dispatch()
        .status();
    assert_eq!(status, Status::SeeOther, "test sign-in failed");
    client
}

/// Submits the panel's new-user form.
fn create_from_panel(client: &Client, email: &str) -> Status {
    client.get("/admin/users/new").dispatch();
    let body = with_token(
        client,
        &format!("name=Grace&email={email}&password=a-long-password"),
    );
    client
        .post("/admin/users")
        .header(ContentType::Form)
        .body(body)
        .dispatch()
        .status()
}

/// Presses the panel's delete button on the user with `id`.
fn delete_from_panel(client: &Client, id: i64) -> Status {
    client.get("/admin/users").dispatch();
    client
        .post(format!("/admin/users/{id}/delete"))
        .header(ContentType::Form)
        .body(with_token(client, ""))
        .dispatch()
        .status()
}

/// Runs the app's `ignite` command (`Ignite::call_cli` on [`app`]) in a child process, with
/// `stdin` as its standard input.
fn app_cli(args: &[&str], stdin: &str) -> std::process::Output {
    run_child(
        Command::new(std::env::current_exe().expect("the test binary"))
            .args(["--exact", "cli_child", "--nocapture", "--test-threads=1"])
            .env(CHILD_ARGS, args.join("\u{1f}")),
        stdin,
    )
}

/// Runs the workspace's stock `ignite` binary, which has no app and so no callbacks.
fn stock_cli(args: &[&str], stdin: &str) -> std::process::Output {
    run_child(Command::new(env!("CARGO_BIN_EXE_ignite")).args(args), stdin)
}

/// Runs `command` with the test database, from the temp dir so no `.env` file is read.
fn run_child(command: &mut Command, stdin: &str) -> std::process::Output {
    let mut child = command
        .env("DATABASE_URL", database_url())
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run the command");
    // A command that does not read standard input may exit first, which closes the pipe.
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().expect("wait for the command")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The app's `ignite` binary, run by [`app_cli`] in a child process. In a normal test run
/// the variable is not set, and it does nothing.
#[test]
fn cli_child() {
    let Ok(args) = std::env::var(CHILD_ARGS) else {
        return;
    };
    let args = args.split('\u{1f}').map(str::to_string);
    std::process::exit(app().call_cli(args));
}

#[test]
fn the_panel_form_runs_the_created_callbacks_in_registration_order() {
    let (_guard, conn) = reset_with_log();
    let client = signed_in_app();

    assert_eq!(
        create_from_panel(&client, "grace@example.test"),
        Status::SeeOther
    );

    let grace = find(&conn, "grace@example.test").expect("the user was created");
    assert_eq!(
        events(&conn, grace.id),
        ["created:first", "created:second"],
        "both callbacks ran, in the order they were registered"
    );
}

#[test]
fn a_failing_created_callback_rolls_the_panel_create_back() {
    let (_guard, conn) = reset_with_log();
    let client = signed_in_app();
    let before = log_rows(&conn);

    let email = format!("{REFUSE_CREATE}@example.test");
    assert_eq!(
        create_from_panel(&client, &email),
        Status::InternalServerError,
        "the panel answers with its error page, as for any failed save"
    );

    assert!(find(&conn, &email).is_none(), "no user row is left");
    assert_eq!(
        log_rows(&conn),
        before,
        "the first callback's row is rolled back with the user"
    );
}

#[test]
fn ignite_user_create_through_the_app_runs_the_callbacks() {
    let (_guard, conn) = reset_with_log();

    let output = app_cli(
        &[
            "user",
            "create",
            "--email",
            "ada@example.test",
            "--name",
            "Ada",
            "--password-stdin",
        ],
        "a-long-password\n",
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        text(&output.stderr)
    );

    let ada = find(&conn, "ada@example.test").expect("the user was created");
    assert_eq!(events(&conn, ada.id), ["created:first", "created:second"]);
}

#[test]
fn a_failing_created_callback_makes_ignite_user_create_fail_and_leaves_no_user() {
    let (_guard, conn) = reset_with_log();
    let before = log_rows(&conn);
    let email = format!("{REFUSE_CREATE}@example.test");

    let output = app_cli(
        &["user", "create", "--email", &email, "--password-stdin"],
        "a-long-password\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("Could not create the user") && stderr.contains("the app refused"),
        "the command names the callback's error: {stderr}"
    );
    assert!(find(&conn, &email).is_none(), "no user row is left");
    assert_eq!(log_rows(&conn), before);
}

#[test]
fn the_stock_ignite_binary_runs_no_callbacks() {
    let (_guard, conn) = reset_with_log();

    let output = stock_cli(
        &[
            "user",
            "create",
            "--email",
            "plain@example.test",
            "--password-stdin",
        ],
        "a-long-password\n",
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        text(&output.stderr)
    );
    let plain = find(&conn, "plain@example.test").expect("the user was created");
    assert!(events(&conn, plain.id).is_empty());
}

#[test]
fn create_user_and_delete_user_run_the_callbacks_of_the_configured_app() {
    let (_guard, conn) = reset_with_log();
    // Building the app's panel makes its callbacks the ones that run.
    let _client = signed_in_app();

    let user = conn
        .run(users::create_user(
            &conn.db,
            "Linus",
            "linus@example.test",
            "a-long-password",
        ))
        .expect("create the user");
    assert_eq!(events(&conn, user.id), ["created:first", "created:second"]);

    let deleted = conn
        .run(users::delete_user(&conn.db, user.id))
        .expect("delete the user");
    assert_eq!(deleted, 1);
    assert!(find(&conn, "linus@example.test").is_none());
    assert_eq!(
        events(&conn, user.id),
        [
            "created:first",
            "created:second",
            "deleting",
            "deleted:row-gone"
        ]
    );

    let missing = conn
        .run(users::delete_user(&conn.db, user.id))
        .expect("delete a missing user");
    assert_eq!(missing, 0, "a user that does not exist is not deleted");
    assert_eq!(events(&conn, user.id).len(), 4, "and no callback runs");
}

#[test]
fn deleting_from_the_panel_runs_the_deleted_callback() {
    let (_guard, conn) = reset_with_log();
    let client = signed_in_app();
    assert_eq!(
        create_from_panel(&client, "grace@example.test"),
        Status::SeeOther
    );
    let grace = find(&conn, "grace@example.test").expect("the user was created");

    assert_eq!(delete_from_panel(&client, grace.id), Status::SeeOther);

    assert!(find(&conn, "grace@example.test").is_none());
    assert_eq!(
        events(&conn, grace.id),
        [
            "created:first",
            "created:second",
            "deleting",
            "deleted:row-gone"
        ]
    );
}

#[test]
fn a_failing_deleting_callback_rolls_the_delete_back() {
    let (_guard, conn) = reset_with_log();
    let client = signed_in_app();
    let email = format!("{REFUSE_DELETING}@example.test");
    assert_eq!(create_from_panel(&client, &email), Status::SeeOther);
    let user = find(&conn, &email).expect("the user was created");

    assert_eq!(
        delete_from_panel(&client, user.id),
        Status::InternalServerError
    );
    assert!(find(&conn, &email).is_some(), "the user is kept");
    assert_eq!(
        events(&conn, user.id),
        ["created:first", "created:second"],
        "the deleting callback's own row is rolled back, and the deleted callback never ran"
    );

    let output = app_cli(&["user", "delete", &email], "");
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("the app refused to start deleting"),
        "stderr: {stderr}"
    );
    assert!(find(&conn, &email).is_some(), "the user is kept");
}

#[test]
fn a_failing_deleted_callback_rolls_the_delete_back() {
    let (_guard, conn) = reset_with_log();
    let client = signed_in_app();
    assert_eq!(
        create_from_panel(&client, "grace@example.test"),
        Status::SeeOther
    );
    let user = find(&conn, "grace@example.test").expect("the user was created");
    conn.exec_with(
        "INSERT INTO user_callback_refuse_deleted (user_id) VALUES (?)",
        &[user.id.into()],
    );

    assert_eq!(
        delete_from_panel(&client, user.id),
        Status::InternalServerError
    );
    assert!(
        find(&conn, "grace@example.test").is_some(),
        "the delete that already ran is rolled back, so the user is kept"
    );
    assert_eq!(
        events(&conn, user.id),
        ["created:first", "created:second"],
        "the rows of both delete callbacks are rolled back too"
    );

    let output = app_cli(&["user", "delete", "grace@example.test"], "");
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("the app refused to finish deleting"),
        "stderr: {stderr}"
    );
    assert!(
        find(&conn, "grace@example.test").is_some(),
        "the user is kept"
    );
}

#[test]
fn reset_removes_the_callbacks_an_earlier_panel_installed() {
    let (guard, conn) = reset_with_log();
    // This panel's callbacks become the process's callbacks.
    drop(signed_in_app());
    drop(guard);

    // The shared setup creates the test user with no callbacks: nothing is logged for it.
    let _guard = reset();
    assert_eq!(log_rows(&conn), 0);
    let user = conn
        .run(users::create_user(
            &conn.db,
            "Plain",
            "plain@example.test",
            "a-long-password",
        ))
        .expect("create the user");
    assert!(events(&conn, user.id).is_empty());
}

#[test]
fn ignite_user_delete_through_the_app_runs_the_deleted_callback() {
    let (_guard, conn) = reset_with_log();
    let _client = signed_in_app();
    let user = conn
        .run(users::create_user(
            &conn.db,
            "Ada",
            "ada@example.test",
            "a-long-password",
        ))
        .expect("create the user");

    let output = app_cli(&["user", "delete", "ada@example.test"], "");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        text(&output.stderr)
    );
    assert!(text(&output.stdout).contains("Deleted ada@example.test."));
    assert!(find(&conn, "ada@example.test").is_none());
    assert_eq!(
        events(&conn, user.id),
        [
            "created:first",
            "created:second",
            "deleting",
            "deleted:row-gone"
        ]
    );

    let unknown = app_cli(&["user", "delete", "nobody@example.test"], "");
    assert_eq!(unknown.status.code(), Some(1));
    assert!(text(&unknown.stderr).contains("The user nobody@example.test does not exist."));
}

#[test]
fn ignite_user_delete_refuses_the_last_enabled_super_admin() {
    let (_guard, conn) = reset_with_log();

    let output = stock_cli(&["user", "delete", EMAIL], "");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        text(&output.stderr).contains("last super-admin who is enabled"),
        "stderr: {}",
        text(&output.stderr)
    );
    assert!(find(&conn, EMAIL).is_some());
}
