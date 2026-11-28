//! Rows kept in memory, for the runtime resources of the field and form tests: a
//! `RuntimeData` whose rows each have a numeric `id`, and whose saves merge the validated row
//! into the stored one, as a data layer with one column per field does.

use std::sync::{Arc, Mutex};

use ignite_core::{Database, ListQuery, Page, Row, RuntimeData, RuntimeFuture};
use serde_json::{json, Value};

/// The rows of a test resource. Clone it to keep a handle on the rows the panel writes.
#[derive(Clone, Default)]
pub struct Memory(pub Arc<Mutex<Vec<Row>>>);

impl Memory {
    /// A store holding `rows`, given as JSON objects.
    pub fn with(rows: Vec<Value>) -> Self {
        let rows = rows
            .into_iter()
            .map(|row| row.as_object().expect("a JSON object").clone())
            .collect();
        Memory(Arc::new(Mutex::new(rows)))
    }

    /// The stored rows.
    pub fn rows(&self) -> Vec<Row> {
        self.0.lock().unwrap().clone()
    }

    /// The stored value of `key` in the row with `id`.
    pub fn value(&self, id: i64, key: &str) -> Value {
        self.rows()
            .iter()
            .find(|row| row["id"] == json!(id))
            .and_then(|row| row.get(key).cloned())
            .unwrap_or(Value::Null)
    }
}

impl RuntimeData for Memory {
    fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        let rows = self.rows();
        Box::pin(async move {
            Ok(Page {
                total: rows.len() as i64,
                rows,
            })
        })
    }

    fn find<'a>(&'a self, _: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>> {
        let row = self.rows().into_iter().find(|row| row["id"] == json!(id));
        Box::pin(async move { Ok(row) })
    }

    fn create<'a>(&'a self, _: &'a Database, mut input: Row) -> RuntimeFuture<'a, Row> {
        let mut rows = self.0.lock().unwrap();
        input.insert("id".into(), json!(rows.len() as i64 + 1));
        rows.push(input.clone());
        Box::pin(async move { Ok(input) })
    }

    fn update<'a>(&'a self, _: &'a Database, id: i64, input: Row) -> RuntimeFuture<'a, Row> {
        let mut rows = self.0.lock().unwrap();
        let row = rows
            .iter_mut()
            .find(|row| row["id"] == json!(id))
            .expect("the row");
        row.extend(input);
        let row = row.clone();
        Box::pin(async move { Ok(row) })
    }

    fn delete<'a>(&'a self, _: &'a Database, id: i64) -> RuntimeFuture<'a, u64> {
        let mut rows = self.0.lock().unwrap();
        let before = rows.len();
        rows.retain(|row| row["id"] != json!(id));
        let deleted = (before - rows.len()) as u64;
        Box::pin(async move { Ok(deleted) })
    }
}
