use super::fetch::ConstraintInfo;
use crate::db::postgres::quoting::quote_ident;

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
