use ignite_core::IgniteResource;

mod schema {
    diesel::table! {
        widgets (id) {
            id -> Integer,
            category_id -> Integer,
        }
    }
}

#[derive(serde::Serialize, IgniteResource)]
#[ignite(table = schema::widgets)]
pub struct Widget {
    id: i32,
    #[ignite(belongs_to = schema::categories)]
    category_id: i32,
}

fn main() {}
