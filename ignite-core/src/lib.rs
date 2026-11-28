//! Engine, resource traits and the database context for Ignite.
//!
//! A resource describes one table the admin panel manages: its form
//! fields, its list columns, and the queries that list, find, create,
//! update and delete its rows. The `ignite-panel` crate turns a set of
//! resources into Rocket routes.

pub mod action;
pub mod condition;
pub mod editor;
pub mod field;
pub mod form;
pub mod layout;
pub mod list;
pub mod media;
pub mod password;
pub mod policy;
pub mod query;
pub mod resource;
pub mod runtime_resource;
pub mod structure;
mod value;
pub mod widget;

pub use action::{ActionAbility, ActionResult, RowAction, DEFAULT_CONFIRMATION};
pub use condition::{FieldConditions, FormContext, Operation};
pub use editor::{editor_script, register_editor};
pub use field::{Block, Column, Field, FieldKind};
pub use form::{FieldErrors, FormInput, ValidationError};
/// Derives `IgniteResource` for an Orbit model. See `ignite_macros` for the attributes.
pub use ignite_macros::IgniteResource;
pub use layout::{ColumnSpan, Component, FormLayout, Group, Section, Tabs, Width};
pub use list::{active_tab, BadgeColor, ColumnBadges, FilterCondition, ListFilter, Tab};
pub use media::{media_library, register_media_library, MediaItem, MediaLibrary};
pub use orbit_orm::Database;
pub use password::{hash_password, verify_password};
pub use policy::{policy, register_policy, unregister_policy, ActingUser, Policy};
pub use query::{escape_like, Direction, ListQuery, Page};
pub use resource::{table_name_is, IgniteResource};
pub use runtime_resource::{
    register_runtime, reserve_slugs, runtime_resource, runtime_resources, unregister_runtime,
    BlockDef, ColumnDef, FieldDef, FieldKindDef, RegisteredResource, Row, RuntimeData,
    RuntimeError, RuntimeFuture, RuntimeResource, MAX_FIELD_NAME,
};
pub use structure::{RepeaterSettings, Repeaters};
pub use widget::{
    FormWidget, LatestRecords, ListWidget, ListWidgetRow, Stat, StatWidget, StatsOverviewWidget,
};
