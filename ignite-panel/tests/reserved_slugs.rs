//! A resource may not use a slug the panel's own routes need. `Ignite::resource`
//! panics at build time, before any request, so the app fails at startup.

mod common;

use ignite_core::{
    Column, Database, Field, FieldErrors, FormInput, IgniteResource, ListQuery, Page,
};
use ignite_panel::Ignite;
use orbit_orm::Error as OrbitError;

/// Declares a resource with only a slug. The builder reads the slug and never calls
/// the query methods, so they are unreachable here.
macro_rules! named_resource {
    ($name:ident, $slug:literal) => {
        struct $name;

        impl IgniteResource for $name {
            type Record = ();
            type Input = ();

            const SLUG: &'static str = $slug;
            const TITLE: &'static str = "Named";

            fn fields() -> &'static [Field] {
                &[]
            }

            fn columns() -> &'static [Column] {
                &[]
            }

            async fn list(_: &Database, _: &ListQuery) -> Result<Page<()>, OrbitError> {
                unreachable!("the builder does not query")
            }

            async fn find(_: &Database, _: i64) -> Result<Option<()>, OrbitError> {
                unreachable!("the builder does not query")
            }

            async fn create(_: &Database, _: ()) -> Result<(), OrbitError> {
                unreachable!("the builder does not query")
            }

            async fn update(_: &Database, _: i64, _: ()) -> Result<(), OrbitError> {
                unreachable!("the builder does not query")
            }

            async fn delete(_: &Database, _: i64) -> Result<u64, OrbitError> {
                unreachable!("the builder does not query")
            }

            fn validate(_: &FormInput) -> Result<(), FieldErrors> {
                unreachable!("the builder does not validate")
            }
        }
    };
}

named_resource!(ProfileResource, "profile");
named_resource!(BuildResource, "build");
named_resource!(ReportsResource, "reports");
named_resource!(UpResource, "up");

#[test]
#[should_panic(expected = "the slug `profile` is reserved")]
fn a_resource_with_a_reserved_slug_panics_at_build_time() {
    common::ensure_secret();
    let _ = Ignite::new().resource::<ProfileResource>();
}

#[test]
#[should_panic(expected = "the slug `build` is reserved")]
fn the_build_slug_is_reserved_because_of_the_asset_route() {
    common::ensure_secret();
    let _ = Ignite::new().resource::<BuildResource>();
}

#[test]
fn the_up_slug_is_not_reserved_because_the_health_check_is_at_the_root() {
    common::ensure_secret();
    let _ = Ignite::new().resource::<UpResource>().fairing();
}

#[test]
fn a_valid_slug_builds_without_a_panic() {
    common::ensure_secret();
    let _ = Ignite::new().resource::<ReportsResource>().fairing();
}
