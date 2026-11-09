//! Engine, resource traits and Diesel context for Ignite.
//!
//! A resource describes one table the admin panel manages: its form
//! fields, its list columns, and the queries that list, find, create,
//! update and delete its rows. The `ignite-panel` crate turns a set of
//! resources into Rocket routes.

pub mod database;
pub mod field;
pub mod form;
pub mod password;
pub mod query;
pub mod resource;
pub mod widget;

pub use database::{build_pool, Conn, Pool};
pub use field::{Column, Field, FieldKind};
pub use form::{FieldErrors, FormInput};
/// Derives `IgniteResource` for a Diesel model. See `ignite_macros` for the attributes.
pub use ignite_macros::IgniteResource;
pub use password::{hash_password, verify_password};
pub use query::{escape_like, Direction, ListQuery, Page};
pub use resource::IgniteResource;
pub use widget::StatWidget;
