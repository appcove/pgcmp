use super::fetch::{ColumnInfo, TableInfo};

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

/// Quote an identifier if needed (simple version)
fn quote_ident(name: &str) -> String {
    // Check if identifier needs quoting
    let needs_quoting = name.chars().next().is_none_or(|c| !c.is_ascii_lowercase())
        || name
            .chars()
            .any(|c| !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '_')
        || is_reserved_word(name);

    if needs_quoting {
        format!("\"{}\"", name.replace('"', "\"\""))
    } else {
        name.to_string()
    }
}

/// Check if a name is a PostgreSQL reserved word (subset of common ones)
fn is_reserved_word(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "all"
            | "analyse"
            | "analyze"
            | "and"
            | "any"
            | "array"
            | "as"
            | "asc"
            | "asymmetric"
            | "both"
            | "case"
            | "cast"
            | "check"
            | "collate"
            | "column"
            | "constraint"
            | "create"
            | "current_catalog"
            | "current_date"
            | "current_role"
            | "current_time"
            | "current_timestamp"
            | "current_user"
            | "default"
            | "deferrable"
            | "desc"
            | "distinct"
            | "do"
            | "else"
            | "end"
            | "except"
            | "false"
            | "fetch"
            | "for"
            | "foreign"
            | "from"
            | "grant"
            | "group"
            | "having"
            | "in"
            | "initially"
            | "intersect"
            | "into"
            | "lateral"
            | "leading"
            | "limit"
            | "localtime"
            | "localtimestamp"
            | "not"
            | "null"
            | "offset"
            | "on"
            | "only"
            | "or"
            | "order"
            | "placing"
            | "primary"
            | "references"
            | "returning"
            | "select"
            | "session_user"
            | "some"
            | "symmetric"
            | "table"
            | "then"
            | "to"
            | "trailing"
            | "true"
            | "union"
            | "unique"
            | "user"
            | "using"
            | "variadic"
            | "when"
            | "where"
            | "window"
            | "with"
    )
}
