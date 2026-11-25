use ignite_core::{FieldConditions, FormLayout, IgniteResource, Operation, Section};

#[derive(orbit_orm::OrbitModel, serde::Serialize, IgniteResource)]
#[orbit(table = "widgets")]
#[ignite(
    table = widgets,
    title = "Widgets",
    conditions = widget_conditions,
    form_layout = widget_layout
)]
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

fn widget_layout() -> FormLayout {
    FormLayout::new()
        .columns(3)
        .schema([Section::make("Widget").column_span(2).schema(["name", "price", "note"])])
}

fn main() {
    let _ = Widget::fields();
    let _ = Widget::columns();
    assert!(Widget::conditions().is_live("active"));
    assert_eq!(Widget::form_layout().column_count(), Some(3));
}
