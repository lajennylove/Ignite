use ignite_core::IgniteResource;

mod schema {
    diesel::table! {
        widgets (id) {
            id -> Integer,
            name -> Text,
        }
    }
}

#[derive(serde::Serialize, IgniteResource)]
#[ignite(table = schema::widgets)]
#[ignite(latest_records(columns = "name,colour"))]
pub struct Widget {
    id: i32,
    name: String,
}

fn main() {}
