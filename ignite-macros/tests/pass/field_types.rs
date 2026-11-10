use diesel::mysql::MysqlConnection;
use diesel::result::QueryResult;
use ignite_core::IgniteResource;

mod schema {
    diesel::table! {
        categories (id) {
            id -> Integer,
            name -> Text,
        }
    }

    diesel::table! {
        products (id) {
            id -> Integer,
            name -> Text,
            category_id -> Nullable<Integer>,
            status -> Text,
            tag -> Nullable<Text>,
            price -> Double,
            stock -> SmallInt,
            launch -> Nullable<Timestamp>,
        }
    }
}

fn load_tags(conn: &mut MysqlConnection) -> QueryResult<Vec<(String, String)>> {
    let _ = conn;
    Ok(Vec::new())
}

#[derive(diesel::Queryable, diesel::Selectable, serde::Serialize, IgniteResource)]
#[diesel(table_name = schema::products)]
#[ignite(table = schema::products)]
pub struct Product {
    id: i32,
    name: String,
    #[ignite(belongs_to = schema::categories, display = name, label = "Category")]
    category_id: Option<i32>,
    #[ignite(options = [("draft", "Draft"), ("published", "Published")])]
    status: String,
    #[ignite(options = load_tags)]
    tag: Option<String>,
    #[ignite(step = "0.01")]
    price: f64,
    stock: i16,
    launch: Option<chrono::NaiveDateTime>,
}

fn main() {}
