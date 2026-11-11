use ignite_core::IgniteResource;

#[derive(orbit_orm::OrbitModel, serde::Serialize)]
#[orbit(table = "categories")]
pub struct Category {
    id: i32,
    name: String,
}

async fn load_tags(db: &ignite_core::Database) -> Result<Vec<(String, String)>, orbit_orm::Error> {
    let _ = db;
    Ok(Vec::new())
}

#[derive(orbit_orm::OrbitModel, serde::Serialize, IgniteResource)]
#[orbit(table = "products")]
#[ignite(table = products)]
pub struct Product {
    id: i32,
    #[orbit(fillable)]
    name: String,
    #[orbit(fillable)]
    #[ignite(belongs_to = Category, display = name, label = "Category")]
    category_id: Option<i32>,
    #[orbit(fillable)]
    #[ignite(options = [("draft", "Draft"), ("published", "Published")])]
    status: String,
    #[orbit(fillable)]
    #[ignite(options = load_tags)]
    tag: Option<String>,
    #[orbit(fillable)]
    #[ignite(step = "0.01")]
    price: f64,
    #[orbit(fillable)]
    stock: i16,
    #[orbit(fillable)]
    launch: Option<chrono::NaiveDateTime>,
}

fn main() {}
