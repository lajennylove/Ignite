use ignite_core::IgniteResource;

mod schema {
    diesel::table! {
        widgets (id) {
            id -> Integer,
            weight -> Double,
        }
    }
}

#[derive(serde::Serialize, IgniteResource)]
#[ignite(table = schema::widgets)]
pub struct Widget {
    id: i32,
    weight: u64,
}

fn main() {}
