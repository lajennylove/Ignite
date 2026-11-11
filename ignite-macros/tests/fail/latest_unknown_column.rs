use ignite_core::IgniteResource;

#[derive(orbit_orm::OrbitModel, serde::Serialize, IgniteResource)]
#[orbit(table = "widgets")]
#[ignite(table = widgets)]
#[ignite(latest_records(columns = "name,colour"))]
pub struct Widget {
    id: i32,
    name: String,
}

fn main() {}
