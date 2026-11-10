use ignite_core::IgniteResource;

mod schema {
    diesel::table! {
        widgets (id) {
            id -> SmallInt,
            name -> Text,
        }
    }
}

#[derive(serde::Serialize, IgniteResource)]
#[ignite(table = schema::widgets)]
pub struct Widget {
    id: i16,
    name: String,
}

fn main() {}
