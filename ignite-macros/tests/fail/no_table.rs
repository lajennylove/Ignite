use ignite_core::IgniteResource;

#[derive(serde::Serialize, IgniteResource)]
pub struct Widget {
    id: i32,
}

fn main() {}
