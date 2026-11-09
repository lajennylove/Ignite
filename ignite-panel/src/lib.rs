//! Rocket fairing and admin routes for Ignite, rendered with Propel views.
//!
//! Attach an [`Ignite`] builder to a Rocket instance with its resources. The
//! fairing manages the connection pool and mounts the admin routes at ignite:
//!
//! ```ignore
//! rocket::build().attach(Ignite::new().resource::<Users>().fairing())
//! ```

mod erased;
mod ignite;
mod routes;
mod views;

pub use ignite::Ignite;
pub use ignite_core;
