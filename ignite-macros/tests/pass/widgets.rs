use ignite_core::{FieldConditions, IgniteResource, Operation};

#[derive(orbit_orm::OrbitModel, serde::Serialize, IgniteResource)]
#[orbit(table = "widgets")]
#[ignite(table = widgets, title = "Widgets", conditions = widget_conditions)]
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

fn widget_conditions() -> FieldConditions {
    FieldConditions::new()
        .live("active")
        .visible("due", |form| form.checked("active"))
        .hidden_on("note", &[Operation::Create])
}

fn main() {
    let _ = Widget::fields();
    let _ = Widget::columns();
    assert!(Widget::conditions().is_live("active"));
}
