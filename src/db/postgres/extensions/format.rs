use super::fetch::ExtensionInfo;
use crate::db::postgres::quoting::quote_ident;

/// Format an extension into CREATE EXTENSION DDL
pub fn format_extension_ddl(ext: &ExtensionInfo) -> String {
    format!(
        "CREATE EXTENSION IF NOT EXISTS {} WITH SCHEMA {} VERSION '{}';\n",
        quote_ident(&ext.name),
        quote_ident(&ext.schema_name),
        ext.version.replace('\'', "''")
    )
}
