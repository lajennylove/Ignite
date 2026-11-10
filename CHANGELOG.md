# Changelog

All notable changes to Ignite are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-11-09

### Added

- A new sidebar and top bar in the panel layout.
- A profile page at `/admin/profile`, with the name and email, the app's own profile
  fields, a password form, and a profile picture (PNG, JPEG or WebP, up to 1 MB).
- A light and dark theme for each user, saved with the user's account.
- `ignite migrate`, a command that runs the panel's pending migrations as a deploy step.
  It prints what it applied, or "Nothing to migrate.", and exits 1 on failure.
- `.env` loading. `Ignite::new()` reads a `.env` file from the working directory.
  Variables already set in the environment take precedence over the file.
- `IGNITE_TRUSTED_PROXIES`, a comma-separated list of IP addresses and CIDR ranges
  whose forwarded headers the panel trusts.
- `GET /up`, a health check that answers `OK` with status 200 at the application root.
  It needs no sign-in.
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
- The profile validates number and date fields with the same rules as resource fields.

### Fixed

- Password-type profile fields are stored as argon2 hashes, not as plain text. The form
  never shows the stored value, and a blank password keeps the stored hash.
- The client address for the login throttle is the socket address. `X-Real-IP` and
  `X-Forwarded-For` are read only from a proxy listed in `IGNITE_TRUSTED_PROXIES`, so a
  client can no longer choose its own bucket by sending a header.
- The login throttle table deletes rows whose window has ended on each failure. It no
  longer grows with every email that was tried.
- A password change is throttled. Wrong current passwords count against the same key as
  sign-in, and a successful change clears the count.
- A resource that declares zero rows per page no longer divides by zero on the list view.

### Migrations added since 0.1.0

- `20261110000000_add_user_theme`: the user's colour theme.
- `20261111000000_profile`: the profile picture and the app's profile values.
- `20261112000000_login_throttles_window_index`: an index on `login_throttles(window_start)`.

## [0.1.0]

The first release. It was tagged `v0.1.0` and is not described here in detail.
