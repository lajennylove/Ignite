//! Engine, resource traits and the database context for Ignite.
//!
//! A resource describes one table the admin panel manages: its form
//! fields, its list columns, and the queries that list, find, create,
//! update and delete its rows. The `ignite-panel` crate turns a set of
//! resources into Rocket routes.

pub mod field;
pub mod form;
pub mod password;
pub mod query;
pub mod resource;
pub mod runtime_resource;
pub mod widget;

pub use field::{Column, Field, FieldKind};
pub use form::{FieldErrors, FormInput};
/// Derives `IgniteResource` for an Orbit model. See `ignite_macros` for the attributes.
pub use ignite_macros::IgniteResource;
pub use orbit_orm::Database;
pub use password::{hash_password, verify_password};
pub use query::{escape_like, Direction, ListQuery, Page};
pub use resource::{table_name_is, IgniteResource};
pub use runtime_resource::{
    register_runtime, reserve_slugs, runtime_resource, runtime_resources, unregister_runtime,
    ColumnDef, FieldDef, FieldKindDef, RegisteredResource, Row, RuntimeData, RuntimeError,
    RuntimeFuture, RuntimeResource, MAX_FIELD_NAME,
};
pub use widget::{LatestRecords, StatWidget};
