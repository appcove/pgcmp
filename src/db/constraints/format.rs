use super::fetch::ConstraintInfo;

/// Format a constraint into ALTER TABLE ADD CONSTRAINT DDL
pub fn format_constraint_ddl(con: &ConstraintInfo) -> String {
    format!(
        "ALTER TABLE {}.{} ADD CONSTRAINT {} {};\n",
        quote_ident(&con.schema_name),
        quote_ident(&con.table_name),
        quote_ident(&con.constraint_name),
        con.definition
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
