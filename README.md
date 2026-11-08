# Ignite

A Filament-inspired admin panel for [Rocket.rs](https://rocket.rs), built on [Propel](https://github.com/lajennylove/propel) templates and [Diesel](https://diesel.rs).

Status: early scaffold. The workspace has three crates, and none of them implements the plan yet.

- `ignite-core`: the engine, resource traits and Diesel context.
- `ignite-macros`: derive macros for admin resources.
- `ignite-panel`: the Rocket fairing and admin routes, rendered with Propel.

Documentation and examples live in [RocketIgnite](https://github.com/lajennylove/RocketIgnite) and [RocketIgniteExample](https://github.com/lajennylove/RocketIgniteExample).

## Build

```sh
cargo check --workspace
```
