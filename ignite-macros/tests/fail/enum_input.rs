use ignite_core::IgniteResource;

mod schema {
    diesel::table! {
        widgets (id) {
            id -> Integer,
        }
    }
}

#[derive(serde::Serialize, IgniteResource)]
#[ignite(table = schema::widgets)]
pub enum Widget {
    Small,
}

fn main() {}
