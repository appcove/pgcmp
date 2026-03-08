use super::fetch::FunctionInfo;

/// Format a function into DDL
/// pg_get_functiondef already returns full CREATE FUNCTION statement
pub fn format_function_ddl(func: &FunctionInfo) -> String {
    let mut ddl = func.definition.clone();
    // Ensure it ends with a semicolon
    if !ddl.trim().ends_with(';') {
        ddl.push(';');
    }
    ddl.push('\n');
    ddl
}
