use super::fetch::ViewInfo;

/// Format a view into CREATE VIEW DDL
pub fn format_view_ddl(view: &ViewInfo) -> String {
    format!(
        "CREATE VIEW {}.{} AS\n{};\n",
        quote_ident(&view.schema_name),
        quote_ident(&view.view_name),
        view.definition.trim_end_matches(';').trim()
    )
}

/// Format a materialized view into CREATE MATERIALIZED VIEW DDL
pub fn format_matview_ddl(view: &ViewInfo) -> String {
    format!(
        "CREATE MATERIALIZED VIEW {}.{} AS\n{};\n",
        quote_ident(&view.schema_name),
        quote_ident(&view.view_name),
        view.definition.trim_end_matches(';').trim()
    )
}

/// Quote an identifier if needed
fn quote_ident(name: &str) -> String {
    let needs_quoting = name.chars().next().is_none_or(|c| !c.is_ascii_lowercase())
        || name
            .chars()
            .any(|c| !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '_');

    if needs_quoting {
        format!("\"{}\"", name.replace('"', "\"\""))
    } else {
        name.to_string()
    }
}
