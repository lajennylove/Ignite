# Ignite

An admin panel for [Rocket.rs](https://rocket.rs). Ignite builds a back office for a MariaDB or MySQL table: a sign-in page, a dashboard, and screens to list, search, sort, create, edit and delete its rows. It renders with [Propel](https://github.com/lajennylove/Propel) views and talks to the database through [Diesel](https://diesel.rs).

Documentation: [ignite.rocketboost.rs](https://ignite.rocketboost.rs).

## Install

Ignite is not on crates.io. Add it from git, with the Rocket and Propel revisions it is built against:

```toml
[dependencies]
rocket = { git = "https://github.com/rwf2/Rocket", rev = "3a54d079" }
ignite-core = { git = "https://github.com/lajennylove/Ignite", tag = "v0.1.0" }
ignite-panel = { git = "https://github.com/lajennylove/Ignite", tag = "v0.1.0" }
propel_rocket = { git = "https://github.com/lajennylove/Propel", tag = "v0.2.0" }
```

Ignite uses a patched Diesel, with a fix for MariaDB 13 and the libmariadb 3.4 client. Until a Diesel release contains the fix, depend on the fork that Ignite's `Cargo.toml` names.

## Configure

Ignite reads two environment variables. Keep them out of git.

- `DATABASE_URL`: `mysql://user:password@host:port/database`.
- `IGNITE_SECRET_KEY`: signs the login session and the flash messages. Generate one with `openssl rand -base64 32`. Without it, a debug build makes a random key for each run, and every restart signs everyone out.

## Start

Attach the panel to your Rocket app, with a resource for each table you want to manage:

```rust
rocket::build().attach(
    Ignite::new()
        .resource::<Users>()
        .fairing(),
)
```

The panel creates its own `users` table when the app starts. To create the first admin, run:

```sh
cargo run --bin ignite -- user create --email you@example.com --name "You"
```

The command asks for a password twice without echoing it, or reads it from standard input with `--password-stdin`.

The panel is served under `/admin`. The documentation explains how to define a resource, its fields and columns, and how to extend the layout.

## Requirements

- Rust (stable).
- MariaDB or MySQL.
