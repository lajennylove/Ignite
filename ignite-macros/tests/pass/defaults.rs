use ignite_core::IgniteResource;

mod schema {
    diesel::table! {
        users (uid) {
            uid -> BigInt,
            email -> Text,
        }
    }
}

// The primary key is marked explicitly here; the title defaults to the table name.
#[derive(diesel::Queryable, diesel::Selectable, serde::Serialize, IgniteResource)]
#[diesel(table_name = schema::users)]
#[ignite(table = schema::users)]
pub struct User {
    #[ignite(primary_key)]
    uid: i64,
    #[ignite(required)]
    email: String,
}

fn main() {
    assert_eq!(<User as IgniteResource>::TITLE, "Users");
    assert_eq!(<User as IgniteResource>::SLUG, "users");
}
