use ignite_core::IgniteResource;

mod schema {
    diesel::table! {
        widgets (id) {
            id -> Integer,
            pin -> Text,
        }
    }
}

#[derive(serde::Serialize, IgniteResource)]
#[ignite(table = schema::widgets)]
#[ignite(latest_records(columns = "pin"))]
pub struct Widget {
    id: i32,
    #[ignite(form_type = "password")]
    pin: String,
}

fn main() {}
