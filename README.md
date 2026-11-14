# Ignite

An admin panel for [Rocket.rs](https://rocket.rs). Ignite builds a back office for a MariaDB or MySQL table: a sign-in page, a dashboard, a profile page, and screens to list, search, sort, create, edit and delete its rows. It renders with [Propel](https://github.com/lajennylove/Propel) views and talks to the database through [Orbit](https://github.com/lajennylove/Orbit), an ORM for Rocket.

Documentation: [ignite.rocketboost.rs](https://ignite.rocketboost.rs). Changes between releases are in [CHANGELOG.md](CHANGELOG.md). Ignite is released under the [MIT License](LICENSE).

## Install

Ignite is not on crates.io. Add it from git, at the revision the install page and the example app pin. Orbit, Rocket and Propel come from crates.io, so the app builds one copy of each crate:

```toml
[dependencies]
ignite-panel = { git = "https://github.com/lajennylove/Ignite", rev = "d3d1975ae107b4e7e7dce92a20f1c83a73428866" }
ignite-core = { git = "https://github.com/lajennylove/Ignite", rev = "d3d1975ae107b4e7e7dce92a20f1c83a73428866" }
orbit-orm = "0.1.0"
rocket = "0.5.2"
propel_rocket = "0.2.0"
propel_runtime = "0.2.0"
serde = { version = "1", features = ["derive"] }
chrono = { version = "0.4", features = ["serde"] }
dotenvy = "0.15"

[build-dependencies]
propel_compiler = "0.2.0"
```

- **Ignite** is pinned to revision `d3d1975`, the revision of the Ignite main branch that the install page documents. Cargo takes it from git. That revision is before 0.3.0; the pin moves to the 0.3.0 revision when that release is published. Upgrading to 0.3.0 has breaking changes: see the migration notes in [CHANGELOG.md](CHANGELOG.md).
- **Orbit** (`orbit-orm`) 0.1.0 comes from crates.io. The models derive its `OrbitModel`.
- **Rocket** 0.5.2 and **Propel** 0.2.0 come from crates.io. A git pin of either would build a second copy, and the panel's views would not match the app's.
- **propel_compiler** 0.2.0 is a build dependency. An app with its own views compiles them in its `build.rs`.
- **chrono** is needed for the date fields of a resource, and **dotenvy** for the `ignite` command.

### Build requirements

- Rust (stable).
- Node.js is needed only to change the panel's own interface (its CSS and JavaScript under `ignite-panel/ui`). The built files are in the repository, so building and running an app does not need Node.

## Configure

Ignite reads its settings from the environment. When a `.env` file is in the working directory, it is loaded first. Variables already set in the environment win over the file, as in Laravel. A missing `.env` is fine. A `.env` that exists but cannot be parsed stops the app with an error that names the file. Keep `.env` files and secrets out of git.

| Variable | Required | What it does |
| --- | --- | --- |
| `DATABASE_URL` | Yes | `mysql://user:password@host:port/database`. |
| `IGNITE_SECRET_KEY` | Yes in a release build | Signs the sign-in session and the flash messages. Generate one with `openssl rand -base64 32`. A debug build makes a random key for each run if it is unset, so every restart signs everyone out. A release build refuses to start without a valid key. |
| `IGNITE_TRUSTED_PROXIES` | No | Comma-separated IP addresses and CIDR ranges of the proxies in front of the app, such as `10.0.0.0/8, 192.0.2.10`. The panel believes their `X-Forwarded-For` and `X-Forwarded-Proto` headers. See below. |
| `IGNITE_VIEWS_DIR` | No | Debug builds only: reads the panel's views from this directory, so they can be edited without a rebuild. |
| `IGNITE_APP_URL` | No | The app's own URL, such as `https://app.example.test`. Password reset links are built from it, as Laravel builds them from `APP_URL`. Without it, the links use the scheme and host of the request, which a client can set, so set it in production. |
| `EMAILIT_API_KEY` | Only for Emailit | The Emailit API key, read by `EmailitMailer::from_env`. It is never logged. |
| `IGNITE_MAIL_FROM` | Only for Emailit | The sender, such as `Ignite <hello@mail.example.test>`. The domain must be verified in Emailit. |

### Client addresses and proxies

The login throttle counts failed sign-ins per email and per client address. The client address is the socket address of the connection. Forwarded headers are believed only when the socket address is listed in `IGNITE_TRUSTED_PROXIES`. Then the panel uses the right-most `X-Forwarded-For` entry that is not itself a trusted proxy. `X-Real-IP` is never read. If the socket address is unknown, the panel counts those requests together under the key `unknown`.

The same rule decides whether a request is secure. The panel's cookies get the `Secure` flag when the request was made over HTTPS: when every listener of the server is TLS, or when the socket is a trusted proxy whose right-most `X-Forwarded-Proto` is `https`. A plain HTTP request, such as a local development server, gets cookies without the flag, so the browser keeps them. `X-Forwarded-Host` is not read.

Rocket does not report, for a single request, which listener it arrived on. A server with both an HTTP and an HTTPS listener is therefore treated as not direct TLS. Behind a proxy, that case is handled by `X-Forwarded-Proto`.

## Deploy and start

Migrations are a deploy step. The panel does not migrate when the app starts, so two instances never race on the schema. Run the migrations before the first start, and again after each upgrade:

```sh
ignite migrate
```

The command prints each migration it applies, or `Nothing to migrate.`. It exits with status 1 on failure. `ignite user create` refuses to run while migrations are pending, and says to run `ignite migrate` first.

To see where the database is:

```sh
ignite migrate:status
```

lists each migration as `Applied` or `Pending`.

To undo the last batch, run `ignite migrate:rollback`. A batch is the set of migrations that one `ignite migrate` applied, so a run reverts all of them, last first, each with its `down.sql`. After a `migrate` that applied one migration, a rollback reverts that one. It is not one step per run: when a batch has several migrations, one run reverts all of them. It says so in its output, and changes nothing else.

**Batch 0.** The panel records its migrations in the `__diesel_schema_migrations` table. A database that was migrated before the move to Orbit has its earlier migrations there as batch 0. They are not run again. A rollback that reaches batch 0 reverts those migrations too, which drops the panel's tables. Back up the database before you roll back that far.

**If a migration fails.** MySQL and MariaDB commit each schema statement as it runs, and a failed migration does not roll back the statements before it. A failed `ignite migrate` can therefore leave the database part way through a migration. To recover:

1. Read the error. Fix its cause, such as a duplicate value or a missing column the migration needs.
2. Run `ignite migrate:status` to see which migrations are recorded as applied.
3. Run `ignite migrate` again. A migration that was only partly applied can need its partial change removed first. Check the database by hand, and use `ignite migrate:rollback` when the last migration is the one that failed and its `down.sql` undoes what it did.

Take a database backup before each deploy that migrates.

To get the `ignite` command in your app, add a small binary that loads `.env` and calls the panel's command line:

```rust
// src/bin/ignite.rs
fn main() {
    let _ = dotenvy::dotenv();
    ignite_panel::cli::main();
}
```

Create the first admin with the command. It asks for a password twice without echoing it, or reads it from standard input with `--password-stdin`:

```sh
cargo run --bin ignite -- user create --email you@example.com --name "You"
```

Attach the panel to your Rocket app, with a resource for each table you want to manage:

```rust
rocket::build().attach(
    Ignite::new()
        .resource::<Users>()
        .fairing(),
)
```

The panel is served under `/admin`. Its health check is `GET /up`, at the application root. It answers with a minimal HTML page, "Application up", and needs no sign-in. The documentation explains how to define a resource, its fields and columns, the profile fields an app adds, and how to extend the layout.

## Resources

A resource is an Orbit model with `#[derive(OrbitModel)]` and `#[derive(IgniteResource)]`. The model also derives `Serialize`. The derive writes the form, the list and the queries. Its `#[ignite(table = ...)]` must name the table of the model's `#[orbit(table = ...)]`, which is checked at compile time. Each writable field needs `#[orbit(fillable)]`, because Orbit refuses to insert or update a column that is not fillable. Each field's type sets its input and its check:

| Rust type | Input | Checked as |
| --- | --- | --- |
| `String` | text, or `textarea`, `password` by attribute | any text, required unless `Option` |
| `i16`, `i32`, `i64` | number | a whole number in the type's range |
| `f32`, `f64` | number, with `step = "0.01"` (default `any`) | a finite number |
| `bool` | checkbox | ticked or not |
| `chrono::NaiveDate` | date | `YYYY-MM-DD` |
| `chrono::NaiveDateTime` | date and time | `YYYY-MM-DD HH:MM`, with seconds if given |

A select is a `String` field with a list of choices, or a field whose choices come from the database:

```rust
#[ignite(options = [("draft", "Draft"), ("published", "Published")])]
status: String,

#[ignite(options = load_categories)]
tag: Option<String>,

#[ignite(belongs_to = Category, display = name)]
category_id: Option<i32>,
```

`load_categories` is an async function `async fn(&Database) -> Result<Vec<(String, String)>, orbit_orm::Error>`, called each time a form is shown. A `belongs_to` field stores the related row's `id`. Its select lists the rows of the related table by their `display` column, and the list shows that name. A submitted value must be one of the choices, so a value with no row is refused. The derive does not write a foreign key: add one in the migration if the database should enforce it.

### Saving a form

Create and update write the fields of the form. A blank `Option` field is saved as NULL, on edit as on create, so a user clears an optional value by emptying it and saving (since 0.3.0; before, a blank optional field was skipped on edit and kept its old value). An `Option` field that the request does not send at all is left out of an update, so its stored value is kept; `FormInput::has` tells a field sent blank from one not sent. A password is the exception: left blank on edit, it keeps the stored hash. A field that is not an `Option` is required, and a blank one is refused with "<label> is required." on create and on edit. A checkbox is never blank: unticked, or not sent, is `false`.

The errors of a form are a `FieldErrors`, a `HashMap<String, String>` from the field name to its message, as Laravel's `MessageBag` keys messages by field. A resource that implements `IgniteResource` by hand writes literal keys with `.to_string()`:

```rust
fn validate(form: &FormInput) -> Result<WidgetInput, FieldErrors> {
    let mut errors = FieldErrors::new();
    let price = match form.get("price").map(str::parse::<i32>) {
        Some(Ok(price)) => price,
        _ => {
            errors.insert("price".to_string(), "Price must be a whole number.".to_string());
            0
        }
    };
    // ...
}
```

The checks on `Field` (`check_required`, `check_whole_number`, `check_decimal`, `check_date` and the others) record their message under the field's name, so most resources never write a key by hand.

### Runtime resources

A resource can also be defined by data while the app runs, such as a custom post type that a site owner creates in the admin. Build a `RuntimeResource` from its slug, titles, `FieldDef`s and `ColumnDef`s (owned twins of `Field` and `Column`; `FieldDef::from(field)` converts a static one), give it a `RuntimeData` that reads and writes its rows as JSON maps, and add it with `register_runtime`. Definitions stay owned: editing and registering a resource again keeps no copy of the old one, and removing it frees everything, field names included (its form errors are keyed by owned `String`s). A field name must be a lowercase identifier: lowercase letters, digits and `_`, starting with a letter, at most 64 bytes, as a column name is. `register_runtime` refuses any other name with `RuntimeError::InvalidFieldName`, and two fields with one name with `RuntimeError::DuplicateField`.

```rust
use ignite_core::{register_runtime, unregister_runtime, ColumnDef, FieldDef, FieldKindDef, RuntimeResource};

register_runtime(
    RuntimeResource::new("books", "Books", "Book", BookData)
        .field(FieldDef { name: "title".into(), label: "Title".into(), kind: FieldKindDef::Text, required: true })
        .column(ColumnDef { key: "title".into(), label: "Title".into(), sortable: true, searchable: true }),
)?;
unregister_runtime("books")?;
```

The panel serves it at `/admin/<slug>` with the same list, forms, checks and delete action as a static resource, and shows it in the navigation, from the next request on. Ignite assumes no table layout: `RuntimeData` decides where the rows live. A row needs a numeric `id`. A slug that is not valid is refused with `RuntimeError::InvalidSlug`, and a slug that a static resource, another runtime resource or a panel page (such as `login`) uses with `RuntimeError::SlugTaken`; `unregister_runtime` of a slug that is not registered returns `RuntimeError::NotRegistered`. On edit, an optional field sent blank comes to `RuntimeData::update` as `null`, and one the form did not send and a blank password are left out of the row, as for a derived resource. Call `ignite_panel::register_runtime_permissions(&db, "books").await?` right after `register_runtime` (it refuses a slug that is not registered), so its five permissions exist at once and a role can be granted them; without the call, the panel creates them on the next signed-in request. They stay when the resource is removed. A unique-index violation shows as "<label> is already taken." only when the index is named `{slug}_{field}_unique`, as for a static resource; a `RuntimeData` that stores values elsewhere should check uniqueness itself. A runtime resource has no dashboard cards or table yet.

## Roles and permissions

The panel follows Filament and its standard plugin, Filament Shield (built on Spatie laravel-permission). Each resource has five abilities, named after its slug:

| Ability | Lets the user |
| --- | --- |
| `<slug>.viewAny` | See the list, and the resource in the navigation and on the dashboard. |
| `<slug>.view` | Reserved for a detail page. The panel has none yet, so nothing checks it. |
| `<slug>.create` | Open the new form and save a new record. |
| `<slug>.update` | Open the edit form and save a change. |
| `<slug>.delete` | Delete a record. |

A resource with the slug `users` has `users.viewAny`, `users.view`, `users.create`, `users.update` and `users.delete`. The panel registers them in the `permissions` table when the app starts, so declare the resources before the first start, and start the app once after you add one.

A user holds the abilities of their roles. Manage them with the `ignite` command:

```sh
ignite role create editors
ignite role grant editors users.viewAny
ignite role grant editors users.update
ignite user role add editor@example.com editors
ignite user roles editor@example.com
ignite user role remove editor@example.com editors
```

- `super-admin` is a role that passes every check, as Filament Shield's super admin does.
- The migration that adds roles gives `super-admin` to every user who exists when it runs, so an upgrade locks nobody out. A user created later has no roles until `ignite user role add` gives one. The last super-admin cannot lose the role.
- `role grant` refuses a permission that the app has not registered yet. It also refuses an unknown role.
- The server answers 403 Forbidden to a request the user may not make, and an unknown resource stays 404. The navigation lists only the resources the user may view, the "Add new" link and the New button need `create`, the Edit button needs `update`, and the Delete button needs `delete`. The dashboard shows a resource's stat cards and table only when the user may view it.
- An app's own route can check an ability: `user.can("reports.view")`, where `user` is an `AdminUser` request guard.

The panel does not edit roles yet. Use the command.

## Account lifecycle

Manage a user's account with the `ignite` command:

```sh
ignite user disable editor@example.com
ignite user enable editor@example.com
ignite user password editor@example.com
```

- A disabled user cannot sign in. They get the same "The email or password is incorrect." message as a wrong password, which does not reveal that the account exists, and the attempt counts against the sign-in throttle. Disabling ends the user's sessions at once, so a signed-in browser is sent to the login page on its next request. Enabling restores sign-in, and the old sessions stay ended.
- The last enabled super-admin cannot be disabled, and a disabled super-admin does not count as one who keeps the panel open, so the role cannot be removed from the last enabled super-admin either.
- `ignite user password` reads the new password the way `ignite user create` does: asked for twice in a terminal, or from standard input with `--password-stdin`. It ends the user's sessions, as the panel's change-password form does for other devices.
- Deleting a user deletes their session rows too, through the foreign key on `sessions.user_id`. The sign-in throttle rows are keyed by email and address rather than by user, and they expire after one minute, so they need no cleanup.

## Mail and password reset

The panel sends email through a mailer that the app chooses. Until it does, the panel has no mailer, and a send is refused and logged, so nothing is sent by accident. To send with Emailit, read the key and the sender from the environment and give the mailer to the builder:

```rust
rocket::build().attach(
    Ignite::new()
        .mailer(EmailitMailer::from_env())
        .resource::<Users>()
        .fairing(),
)
```

Set `EMAILIT_API_KEY` and `IGNITE_MAIL_FROM` in the `.env` file or the environment. `from_env` stops the app at start when either is missing. Emailit answers 401 or 403 for a key or a domain it does not accept, 402 when the account has no credits, 422 for a message it refuses, 429 with `retry_after` when it limits sends, and 404 for an unknown template. Each of these is a `MailError`. Another provider is added as one more implementation of the `Mailer` trait, and the password reset does not change.

Password reset follows Laravel's:

- `/admin/password/forgot` asks for an email. Every request gets the same answer, so the form does not show which emails have an account. A request counts under a throttle key of its own, with the same limit as sign-in: five requests in 60 seconds for each email and address.
- A link is sent only to an account that is enabled. It carries a random token. The table keeps only a hash of it, and the link works for 60 minutes and once.
- A link is not sent again within 60 seconds of the last one to the same email, as Laravel's `throttle` setting does. A double submit sends one message.
- The reset form asks for the new password twice, with the same rules as the change-password form. A reset ends every session of the user, as a password change does.
- The sign-in page links to the forgot page.

A failed send is logged with the error and does not change the answer.

The tests never send mail: they use `RecordingMailer`. One opt-in test, `tests/mail_live.rs`, sends a real message through Emailit. It runs only when `EMAILIT_API_KEY`, `IGNITE_MAIL_FROM` and `IGNITE_TEST_MAIL_TO` are all set, and prints a notice and passes otherwise. The email text follows Laravel's reset notification.

## Tests

The test suite needs a MariaDB or MySQL database that it may drop and rewrite tables in. Point `DATABASE_URL` at a database set aside for tests, such as `ignite_test`, and never at a database with data you keep:

```sh
DATABASE_URL=mysql://user@localhost/ignite_test cargo test --workspace
```

Continuous integration runs the same checks on every push and pull request. It uses a MariaDB 11.4 service, and runs format, lint, the tests and a release build of the panel. The workflow is in `.github/workflows/ci.yml`. Locally, the suite has run against MariaDB 13.0.2.

## Requirements

- Rust (stable).
- MariaDB or MySQL. No MariaDB client library is needed to build.
