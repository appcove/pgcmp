use super::fetch::TriggerInfo;

/// Format a trigger into DDL
/// pg_get_triggerdef already returns full CREATE TRIGGER statement
pub fn format_trigger_ddl(trig: &TriggerInfo) -> String {
    let mut ddl = trig.definition.clone();
    // Ensure it ends with a semicolon
    if !ddl.trim().ends_with(';') {
        ddl.push(';');
    }
    ddl.push('\n');
    ddl
}
