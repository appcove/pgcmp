use super::fetch::ViewInfo;
use crate::db::postgres::quoting::quote_ident;

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
