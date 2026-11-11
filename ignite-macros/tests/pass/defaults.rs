use ignite_core::IgniteResource;

// The primary key is marked explicitly here; the title defaults to the table name.
#[derive(orbit_orm::OrbitModel, serde::Serialize, IgniteResource)]
#[orbit(table = "users", primary_key = "uid")]
#[ignite(table = users)]
pub struct User {
    #[ignite(primary_key)]
    uid: i64,
    #[orbit(fillable)]
    #[ignite(required)]
    email: String,
}

fn main() {
    assert_eq!(<User as IgniteResource>::TITLE, "Users");
    assert_eq!(<User as IgniteResource>::SLUG, "users");
}
