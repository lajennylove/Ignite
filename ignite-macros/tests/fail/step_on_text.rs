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
pub struct Widget {
    id: i32,
    #[ignite(step = "0.01")]
    name: String,
}

fn main() {}
