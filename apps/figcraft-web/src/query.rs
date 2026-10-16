//! The page's URL query: which flags it sets and the invite link it names.

/// The invite link in `?join=<link>` (`search` is `location.search`, with or without its `?`), still
/// escaped. `join` takes the rest of the query, so a pasted `figcraft://join?host=…&file=…&token=…`
/// keeps its own `&`s; put it last.
pub fn join_link(search: &str) -> Option<&str> {
    let query = search.strip_prefix('?').unwrap_or(search);
    let start = if query.starts_with("join=") { 0 } else { query.find("&join=")? + 1 };
    query.get(start + "join=".len()..).filter(|link| !link.is_empty())
}
