use ignite_core::IgniteResource;

mod schema {
    diesel::table! {
        widgets (id) {
            id -> Integer,
            pin -> Integer,
        }
    }
}

#[derive(serde::Serialize, IgniteResource)]
#[ignite(table = schema::widgets)]
pub struct Widget {
    id: i32,
    #[ignite(form_type = "password")]
    pin: i32,
}

fn main() {}
