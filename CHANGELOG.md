# Changelog

All notable changes to Ignite are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Migrations run on Orbit (`orbit-orm`) instead of Diesel. `ignite migrate`, `ignite migrate:status`
  and `ignite migrate:rollback` keep their names. The first `ignite migrate` records the migrations
  Diesel had applied, without running them, so nothing runs twice.
- `ignite migrate:rollback` reverts the last batch, not one migration. A batch is the set of
  migrations one `ignite migrate` applied. The batch imported from Diesel is one batch, so the
  first rollback after the move reverts every panel migration.
- The migration functions `migrate`, `run_migrations`, `pending_migrations`, `migration_status`
  and `rollback` are async and take an `orbit_orm::Database`, not a Diesel connection. `rollback`
  returns the names it reverted.
- The database layer is Orbit (`orbit-orm`) instead of Diesel. Diesel and r2d2 are gone from
  Ignite and from the build, so no MariaDB client library is needed to build.
- `IgniteResource` is async and takes an `orbit_orm::Database`. Its methods (`list`, `find`,
  `create`, `update`, `delete`, `latest`, `widgets`, `options`) return futures. `Pool`, `Conn` and
  `build_pool` are removed from `ignite-core`.
- `#[derive(IgniteResource)]` takes `#[ignite(table = <name>)]`, which must match the model's
  `#[orbit(table = ...)]`, checked at compile time. `belongs_to` takes the related model type, and
  `options = <fn>` takes an async `fn(&Database)`. The model derives `OrbitModel` and marks its
  writable columns `#[orbit(fillable)]`.
- Connection failure at start stops the launch with an error. `Ignite::new()` reads the environment
  and no longer connects.

### Added

- Roles and permissions, modelled on Filament and Filament Shield. Each resource has the
  abilities `<slug>.viewAny`, `<slug>.view`, `<slug>.create`, `<slug>.update` and
  `<slug>.delete`, which the panel registers when the app starts. The routes answer 403
  Forbidden without the ability. The navigation, the dashboard and the buttons show only
  what the user may do. The role `super-admin` passes every check.
- The `ignite role create`, `ignite role grant`, `ignite user role add`,
  `ignite user role remove` and `ignite user roles` commands.
- `AdminUser::can`, which checks an ability in an app's own route.
- The migration `20261113000000_roles_permissions`, which adds the tables `roles`,
  `permissions`, `role_permission` and `role_user`.
- `ignite user disable <email>` and `ignite user enable <email>`. A disabled user gets the
  normal wrong-password message when they sign in, and their sessions end at once.
- `ignite user password <email>`, which sets a new password for an existing user and ends
  their sessions. It reads the password the way `ignite user create` does.
- The migration `20261114000000_account_lifecycle`, which adds `users.disabled_at` and a
  foreign key from `sessions.user_id` to `users.id` with `ON DELETE CASCADE`, so deleting a
  user deletes their sessions.
- Field types for `#[derive(IgniteResource)]`: `i16`, `f32` and `f64` (a decimal input,
  with `step = "0.01"` for its browser step), and `chrono::NaiveDateTime` (a date-and-time
  input). Each is checked as its type says: a decimal refuses text and values that are not
  finite, and a date-and-time refuses what is not one. The profile's decimal and date-time
  fields are checked the same way.
- Static selects: `#[ignite(options = [("draft", "Draft"), ("published", "Published")])]` on
  a `String` field. A submitted value must be one of the choices.
- Dynamic selects: `#[ignite(options = load_choices)]` on a `String` or integer field, where
  `load_choices` is a function `fn(&mut MysqlConnection) -> QueryResult<Vec<(String, String)>>`
  that the panel calls each time it shows the form. A new `IgniteResource::options` method
  returns the choices of a field by name, and the panel checks a submitted value against them.
- Relation selects: `#[ignite(belongs_to = schema::categories, display = name)]` on an `i32`
  or `i64` field stores the related row's `id`. The select lists the related rows by their
  `display` column, and the list and the dashboard show that name in place of the id.
- A select starts with a "Select an option" placeholder, so a submitted form never sends the
  first choice by accident. Profile selects show it too.
- Sending email, behind the `Mailer` trait, with `Ignite::mailer` to choose the provider. The
  default, `NotConfigured`, refuses to send. `RecordingMailer` keeps messages in memory for
  tests. `EmailitMailer` sends through the Emailit API, and reads `EMAILIT_API_KEY` and
  `IGNITE_MAIL_FROM` with `EmailitMailer::from_env`. Failures come back as a typed `MailError`.
- Password reset by email, modelled on Laravel's password reset: `/admin/password/forgot`,
  and the link `/admin/password/reset/<token>?email=`. A link works for 60 minutes and once.
  The forgot page gives the same answer whether or not the email has an account. A disabled
  user gets no link. A reset ends every session of the user. A link is not sent again within
  60 seconds of the last one. The forgot requests are throttled like sign-in.
- The `IGNITE_APP_URL` variable, which the reset links are built from.
- The migration `20261115000000_password_reset_tokens`, which adds the table
  `password_reset_tokens`. The token is stored as an argon2 hash.
- A "Forgot password?" link on the sign-in page.

### Changed

- The derive's message for an unsupported field type lists the types it now supports.
- A resource's number input still accepts any value in the browser, as before. The
  server checks an integer as a whole number and a decimal as a number.
- The last-super-admin rule counts only enabled users. A disabled super-admin cannot lose
  the role while no enabled super-admin is left, and the last enabled one cannot be disabled.
- An upgrade gives `super-admin` to every user who exists when the migration runs, so they
  keep full access. A user created after the upgrade has no roles and sees no resource
  until `ignite user role add` gives them one.


## [0.2.0] - 2026-11-09

### Added

- A new sidebar and top bar in the panel layout.
- A profile page at `/admin/profile`, with the name and email, the app's own profile
  fields, a password form, and a profile picture (PNG, JPEG or WebP, up to 1 MB).
- A light and dark theme for each user, saved with the user's account.
- `ignite migrate`, a command that runs the panel's pending migrations as a deploy step.
  It prints what it applied, or "Nothing to migrate.", and exits 1 on failure.
- `ignite migrate:status`, which lists each migration as Applied or Pending.
- `ignite migrate:rollback`, which reverts the most recently applied migration with its
  `down.sql`. It reverts one step per run and says so in its output.
- `.env` loading. `Ignite::new()` reads a `.env` file from the working directory.
  Variables already set in the environment take precedence over the file.
- `IGNITE_TRUSTED_PROXIES`, a comma-separated list of IP addresses and CIDR ranges
  whose forwarded headers the panel trusts.
- `GET /up`, a health check that answers with status 200 and a minimal HTML page, "Application
  up", at the application root. It needs no sign-in, as Laravel's `/up` does.
- The `MIT` license (`LICENSE`), and `license = "MIT"` in each crate.
- Continuous integration in `.github/workflows/ci.yml`.

### Changed

- The login throttle counts failed sign-ins per email and address together, as
  Laravel's login throttle does: five failures in 60 seconds. Before, failures were
  counted per email (five) and per address (twenty) separately. Now a failed attempt for
  one email does not count against another email from the same address, and the
  address limit of twenty is gone.
- `ignite user create` no longer runs migrations. When migrations are pending it stops
  before the password prompt and says to run `ignite migrate` first.
- The panel's fairing no longer runs migrations at startup. Run `ignite migrate` before
  the first start. `run_migrations` stays public for apps that want to call it.
- The profile's "is required" message matches the resource form ("Name is required.").
- Profile number fields accept decimals, as Laravel's `numeric` rule and Filament's numeric
  inputs do. Resource integer fields still accept whole numbers only. Number inputs carry
  `step="any"`, so browsers accept decimals.
- Profile date fields are checked with the same rule as resource date fields.
- Cookies of the panel get the `Secure` flag when the request was made over HTTPS: directly,
  or through a proxy listed in `IGNITE_TRUSTED_PROXIES` that sends `X-Forwarded-Proto: https`.
  Before, they got `Secure` in release builds only, whatever the scheme. A plain HTTP request,
  such as a local development server, now gets cookies without the flag.
- A `.env` file that exists but cannot be parsed stops the app with a clear message. A missing
  `.env` is still fine.

### Fixed

- Password-type profile fields are stored as argon2 hashes, not as plain text. The form
  never shows the stored value, and a blank password keeps the stored hash.
- The client address for the login throttle is the socket address. `X-Real-IP` and
  `X-Forwarded-For` are read only from a proxy listed in `IGNITE_TRUSTED_PROXIES`, so a
  client can no longer choose its own bucket by sending a header.
- The login throttle table deletes rows whose window has ended on each failure. It no
  longer grows with every email that was tried.
- A password change is throttled under its own key: the email and address, with the same limit
  as sign-in. A wrong current password counts against it, and a successful change clears only
  that key. It never clears the sign-in count.
- A resource that declares zero rows per page no longer divides by zero on the list view.

### Migrations added since 0.1.0

- `20261110000000_add_user_theme`: the user's colour theme.
- `20261111000000_profile`: the profile picture and the app's profile values.
- `20261112000000_login_throttles_window_index`: an index on `login_throttles(window_start)`.

## [0.1.0]

The first release. It was tagged `v0.1.0` and is not described here in detail.
