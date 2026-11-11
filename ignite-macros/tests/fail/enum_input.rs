use ignite_core::IgniteResource;

#[derive(serde::Serialize, IgniteResource)]
#[ignite(table = widgets)]
pub enum Widget {
    Small,
}

fn main() {}
