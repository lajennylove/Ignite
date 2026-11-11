use ignite_core::IgniteResource;

#[derive(orbit_orm::OrbitModel, serde::Serialize, IgniteResource)]
#[orbit(table = "widgets")]
#[ignite(table = widgets)]
#[ignite(latest_records(columns = "pin"))]
pub struct Widget {
    id: i32,
    #[ignite(form_type = "password")]
    pin: String,
}

fn main() {}
