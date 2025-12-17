use super::fetch::IndexInfo;

/// Format an index into DDL
/// pg_get_indexdef already returns full CREATE INDEX statement
pub fn format_index_ddl(idx: &IndexInfo) -> String {
    let mut ddl = idx.definition.clone();
    // Ensure it ends with a semicolon
    if !ddl.trim().ends_with(';') {
        ddl.push(';');
    }
    ddl.push('\n');
    ddl
}
