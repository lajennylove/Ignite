use ignite_core::IgniteResource;

#[derive(orbit_orm::OrbitModel, serde::Serialize, IgniteResource)]
#[orbit(table = "widgets")]
#[ignite(table = widgets, title = "Widgets")]
pub struct Widget {
    id: i32,
    #[orbit(fillable)]
    #[ignite(searchable, sortable)]
    name: String,
    #[orbit(fillable)]
    #[ignite(sortable, label = "Price (cents)")]
    price: i32,
    #[orbit(fillable)]
    #[ignite(form_type = "textarea")]
    note: Option<String>,
    #[orbit(fillable)]
    active: bool,
    #[orbit(fillable)]
    due: Option<chrono::NaiveDate>,
}

fn main() {
    let _ = Widget::fields();
    let _ = Widget::columns();
}
