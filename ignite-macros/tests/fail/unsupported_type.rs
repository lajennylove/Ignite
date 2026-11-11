use ignite_core::IgniteResource;

#[derive(orbit_orm::OrbitModel, serde::Serialize, IgniteResource)]
#[orbit(table = "widgets")]
#[ignite(table = widgets)]
pub struct Widget {
    id: i32,
    weight: u64,
}

fn main() {}
