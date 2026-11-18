use ignite_core::IgniteResource;

#[derive(orbit_orm::OrbitModel, serde::Serialize, IgniteResource)]
#[orbit(table = "widgets")]
#[ignite(table = widgets, navigation_icon = "dashicons dashicons-admin-post")]
pub struct Widget {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

fn main() {}
