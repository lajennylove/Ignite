//! What the list view asks for: the page, the search text and the sort.

/// The sort direction of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Smallest first.
    Asc,
    /// Largest first.
    Desc,
}

/// What the list view asks a resource for. Only `search` and `sort` come from
/// the user, through the query string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListQuery {
    /// The page to return, 1-based.
    pub page: i64,
    /// The number of rows on a page.
    pub per_page: i64,
    /// The search text, if the user entered any.
    pub search: Option<String>,
    /// The column key to sort by and the direction, if the user chose one.
    /// The key is one of the resource's sortable columns.
    pub sort: Option<(String, Direction)>,
}

impl ListQuery {
    /// The number of rows to skip before the page starts. A page below 1 is
    /// treated as 1.
    pub fn offset(&self) -> i64 {
        (self.page.max(1) - 1) * self.per_page
    }
}

/// One page of rows and the total number of matching rows, for the pager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    /// The rows on this page.
    pub rows: Vec<T>,
    /// The number of rows across all pages.
    pub total: i64,
}

/// Escapes `%`, `_` and `\` in search text, so a LIKE pattern matches them
/// literally. Use it inside `%...%` and pass `ESCAPE '\\'` where the database
/// needs the escape character named.
pub fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(page: i64, per_page: i64) -> ListQuery {
        ListQuery {
            page,
            per_page,
            search: None,
            sort: None,
        }
    }

    #[test]
    fn offset_counts_full_pages_before_the_current_one() {
        assert_eq!(query(1, 20).offset(), 0);
        assert_eq!(query(3, 20).offset(), 40);
    }

    #[test]
    fn offset_treats_a_page_below_one_as_the_first() {
        assert_eq!(query(0, 20).offset(), 0);
        assert_eq!(query(-4, 20).offset(), 0);
    }

    #[test]
    fn escape_like_escapes_wildcards_and_the_escape_character() {
        assert_eq!(escape_like("50%_off\\"), "50\\%\\_off\\\\");
        assert_eq!(escape_like("plain"), "plain");
    }
}
