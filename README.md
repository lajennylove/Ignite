# Ignite

An admin panel for [Rocket.rs](https://rocket.rs). Ignite builds a back office for a MariaDB or MySQL table: a sign-in page, a dashboard, a profile page, and screens to list, search, sort, create, edit and delete its rows. It renders with [Propel](https://github.com/lajennylove/Propel) views and talks to the database through [Orbit](https://github.com/lajennylove/Orbit), an ORM for Rocket.

Documentation: [ignite.rocketboost.rs](https://ignite.rocketboost.rs). Changes between releases are in [CHANGELOG.md](CHANGELOG.md). Ignite is released under the [MIT License](LICENSE).

## Install

Ignite is not on crates.io. Add it from git, with the pins below. Each pin is needed for a reason:

```toml
[dependencies]
# Rocket is pinned to a revision the panel is built against.
rocket = { git = "https://github.com/rwf2/Rocket", rev = "3a54d079" }
# Orbit is pinned to a revision of its main branch.
orbit-orm = { git = "https://github.com/lajennylove/Orbit", rev = "2457a1b3e19ff840514613a7237bbc5ba8776b09" }
ignite-core = { git = "https://github.com/lajennylove/Ignite", tag = "v0.2.0" }
ignite-panel = { git = "https://github.com/lajennylove/Ignite", tag = "v0.2.0" }
propel_rocket = { git = "https://github.com/lajennylove/Propel", rev = "09c8a02ba0f3ed75b02c7f3d36b5a0e6f4d047b7" }
propel_macros = { git = "https://github.com/lajennylove/Propel", rev = "09c8a02ba0f3ed75b02c7f3d36b5a0e6f4d047b7" }
propel_runtime = { git = "https://github.com/lajennylove/Propel", rev = "09c8a02ba0f3ed75b02c7f3d36b5a0e6f4d047b7" }
```

- **Rocket** is pinned to revision `3a54d079`, the Rocket revision Ignite is built against. The panel uses Rocket APIs of this revision.
- **Propel** is pinned to one revision, `09c8a02` (the commit of Propel's `v0.2.0` tag), for all the Propel crates. Pin the revision, not the tag: Cargo treats a tag and a revision as two different sources and would build Propel twice, and the panel's views would not be found. `propel_rocket` is the Rocket integration that renders the views.

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

**After the move from Diesel.** The panel keeps its migrations in Diesel's `__diesel_schema_migrations` table. The rows Diesel wrote are the history, as batch 0, and they are not run again. A rollback that reaches batch 0 reverts those migrations too, which drops the panel's tables. Back up the database before you roll back that far.

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

A resource is an Orbit model with `#[derive(OrbitModel)]` and `#[derive(IgniteResource)]`. The derive writes the form, the list and the queries. Each field's type sets its input and its check:

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

- `/admin/password/forgot` asks for an email. Every request gets the same answer, so the form does not show which emails have an account. A request counts against the same throttle as sign-in, five in a minute for each email and address.
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

Continuous integration runs the same checks on every push and pull request. It uses a MariaDB 11.4 service, and runs format, lint, the tests and a release build of the panel. The workflow is in `.github/workflows/ci.yml`. Locally, the suite has run against MariaDB 13.0.2 with the libmariadb 3.4.10 client.

## Requirements

- Rust (stable).
- MariaDB or MySQL, and the client library described under Build requirements.
