use super::fetch::{ColumnInfo, TableInfo};
use crate::db::postgres::quoting::quote_ident;

/// Format a table into CREATE TABLE DDL
pub fn format_table_ddl(table: &TableInfo) -> String {
    let mut ddl = format!(
        "CREATE TABLE {}.{} (\n",
        quote_ident(&table.schema_name),
        quote_ident(&table.table_name)
    );

    let column_defs: Vec<String> = table.columns.iter().map(format_column).collect();

    ddl.push_str(&column_defs.join(",\n"));
    ddl.push_str("\n);\n");

    ddl
}

/// Format a single column definition
fn format_column(col: &ColumnInfo) -> String {
    let mut parts = vec![format!("    {}", quote_ident(&col.name))];

    // Data type (with identity handling)
    if let Some(ref identity) = col.identity_generation {
        // Identity columns: use GENERATED ... AS IDENTITY
        let identity_type = match identity.as_str() {
            "a" => "ALWAYS",
            "d" => "BY DEFAULT",
            _ => "ALWAYS",
        };
        // For identity columns, extract base type (usually integer/bigint)
        let base_type = col.data_type.as_str();
        parts.push(format!(
            "{} GENERATED {} AS IDENTITY",
            base_type, identity_type
        ));
    } else {
        parts.push(col.data_type.clone());
    }

    // NOT NULL constraint
    if !col.is_nullable {
        parts.push("NOT NULL".to_string());
    }

    // Default value (skip for identity columns as it's implicit)
    if col.identity_generation.is_none() && col.column_default.is_some() {
        parts.push(format!("DEFAULT {}", col.column_default.as_ref().unwrap()));
    }

    parts.join(" ")
}
