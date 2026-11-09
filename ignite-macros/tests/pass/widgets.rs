use ignite_core::IgniteResource;

mod schema {
    diesel::table! {
        widgets (id) {
            id -> Integer,
            name -> Text,
            price -> Integer,
            note -> Nullable<Text>,
            active -> Bool,
            due -> Nullable<Date>,
        }
    }
}

#[derive(diesel::Queryable, diesel::Selectable, serde::Serialize, IgniteResource)]
#[diesel(table_name = schema::widgets)]
#[ignite(table = schema::widgets, title = "Widgets")]
pub struct Widget {
    id: i32,
    #[ignite(searchable, sortable)]
    name: String,
    #[ignite(sortable, label = "Price (cents)")]
    price: i32,
    #[ignite(form_type = "textarea")]
    note: Option<String>,
    active: bool,
    due: Option<chrono::NaiveDate>,
}

fn main() {
    let _ = Widget::fields();
    let _ = Widget::columns();
}
