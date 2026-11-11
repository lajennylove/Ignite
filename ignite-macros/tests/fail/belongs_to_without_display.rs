use ignite_core::IgniteResource;

#[derive(orbit_orm::OrbitModel, serde::Serialize, IgniteResource)]
#[orbit(table = "widgets")]
#[ignite(table = widgets)]
pub struct Widget {
    id: i32,
    #[ignite(belongs_to = Category)]
    category_id: i32,
}

fn main() {}
