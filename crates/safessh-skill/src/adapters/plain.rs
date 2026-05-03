//! Plain adapter — canonical body verbatim. For tools that consume
//! markdown without framework-specific wrapping. Requires --path.

pub fn format(body: &str) -> String {
    if body.ends_with('\n') {
        body.to_string()
    } else {
        format!("{body}\n")
    }
}
