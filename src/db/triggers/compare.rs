use super::fetch::TriggerInfo;
use std::collections::HashMap;

/// A detailed difference found when comparing triggers
#[derive(Debug, Clone)]
pub struct TriggerDiff {
    pub schema_name: String,
    pub trigger_name: String,
    pub differences: Vec<String>,
}

impl TriggerDiff {
    pub fn is_empty(&self) -> bool {
        self.differences.is_empty()
    }
}

/// Compare two triggers and return detailed differences
pub fn compare_triggers(old: &TriggerInfo, new: &TriggerInfo) -> TriggerDiff {
    let mut differences = Vec::new();
    let full_name = format!("{}.{}", new.schema_name, new.trigger_name);

    // Normalize definitions for comparison
    let old_def = normalize_trigger_def(&old.definition);
    let new_def = normalize_trigger_def(&new.definition);

    if old_def != new_def {
        let changes = identify_trigger_changes(&old.definition, &new.definition);
        for change in changes {
            differences.push(format!("ALTER TRIGGER: {} — {}", full_name, change));
        }
    }

    TriggerDiff {
        schema_name: new.schema_name.clone(),
        trigger_name: new.trigger_name.clone(),
        differences,
    }
}

/// Normalize trigger definition for comparison
fn normalize_trigger_def(def: &str) -> String {
    def.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// Try to identify specific changes between trigger definitions
fn identify_trigger_changes(old: &str, new: &str) -> Vec<String> {
    let mut changes = Vec::new();

    // Check timing change (BEFORE/AFTER/INSTEAD OF)
    let old_timing = extract_timing(old);
    let new_timing = extract_timing(new);
    if old_timing != new_timing {
        changes.push(format!(
            "timing changed from '{}' to '{}'",
            old_timing.unwrap_or("unknown"),
            new_timing.unwrap_or("unknown")
        ));
    }

    // Check event change (INSERT/UPDATE/DELETE/TRUNCATE)
    let old_events = extract_events(old);
    let new_events = extract_events(new);
    if old_events != new_events {
        changes.push(format!(
            "events changed from '{}' to '{}'",
            old_events.as_deref().unwrap_or("unknown"),
            new_events.as_deref().unwrap_or("unknown")
        ));
    }

    // Check FOR EACH ROW/STATEMENT change
    let old_foreach = extract_for_each(old);
    let new_foreach = extract_for_each(new);
    if old_foreach != new_foreach {
        changes.push(format!(
            "changed from FOR EACH {} to FOR EACH {}",
            old_foreach.unwrap_or("unknown"),
            new_foreach.unwrap_or("unknown")
        ));
    }

    // Check function change
    let old_func = extract_execute_function(old);
    let new_func = extract_execute_function(new);
    if old_func != new_func {
        changes.push(format!(
            "execute function changed from '{}' to '{}'",
            old_func.unwrap_or("unknown"),
            new_func.unwrap_or("unknown")
        ));
    }

    // Check WHEN clause
    let old_when = extract_when_clause(old);
    let new_when = extract_when_clause(new);
    match (&old_when, &new_when) {
        (None, Some(w)) => {
            changes.push(format!("added WHEN clause: {}", w));
        }
        (Some(w), None) => {
            changes.push(format!("removed WHEN clause (was: {})", w));
        }
        (Some(old_w), Some(new_w)) if old_w != new_w => {
            changes.push(format!(
                "WHEN clause changed from '{}' to '{}'",
                old_w, new_w
            ));
        }
        _ => {}
    }

    if changes.is_empty() {
        changes.push("trigger definition changed".to_string());
    }

    changes
}

/// Extract timing (BEFORE/AFTER/INSTEAD OF)
fn extract_timing(def: &str) -> Option<&'static str> {
    let upper = def.to_uppercase();
    if upper.contains("BEFORE ") {
        Some("BEFORE")
    } else if upper.contains("AFTER ") {
        Some("AFTER")
    } else if upper.contains("INSTEAD OF") {
        Some("INSTEAD OF")
    } else {
        None
    }
}

/// Extract events (INSERT/UPDATE/DELETE/TRUNCATE)
fn extract_events(def: &str) -> Option<String> {
    let upper = def.to_uppercase();

    // Find position after timing keyword
    let start = upper
        .find("BEFORE ")
        .or_else(|| upper.find("AFTER "))
        .or_else(|| upper.find("INSTEAD OF "))?;

    let rest = &upper[start..];

    // Find end of events (ON keyword)
    let end = rest.find(" ON ")?;
    let events_part = &rest[..end];

    // Extract just the events
    let events: Vec<&str> = events_part
        .split_whitespace()
        .filter(|w| matches!(*w, "INSERT" | "UPDATE" | "DELETE" | "TRUNCATE" | "OR"))
        .collect();

    Some(events.join(" "))
}

/// Extract FOR EACH ROW/STATEMENT
fn extract_for_each(def: &str) -> Option<&'static str> {
    let upper = def.to_uppercase();
    if upper.contains("FOR EACH ROW") {
        Some("ROW")
    } else if upper.contains("FOR EACH STATEMENT") {
        Some("STATEMENT")
    } else {
        None
    }
}

/// Extract EXECUTE FUNCTION/PROCEDURE name
fn extract_execute_function(def: &str) -> Option<&str> {
    let upper = def.to_uppercase();
    let pos = upper
        .find("EXECUTE FUNCTION ")
        .or_else(|| upper.find("EXECUTE PROCEDURE "))?;

    let keyword_len = if upper[pos..].starts_with("EXECUTE FUNCTION") {
        17
    } else {
        18
    };

    let start = pos + keyword_len;
    let rest = &def[start..];
    let end = rest
        .find(';')
        .or_else(|| rest.find('\n'))
        .unwrap_or(rest.len());
    Some(rest[..end].trim())
}

/// Extract WHEN clause
fn extract_when_clause(def: &str) -> Option<&str> {
    let upper = def.to_uppercase();
    if let Some(pos) = upper.find(" WHEN (") {
        let start = pos + 6;
        let rest = &def[start..];
        // Find matching closing paren
        let mut depth = 0;
        for (i, c) in rest.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    if depth == 0 {
                        return Some(rest[..i].trim());
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }
    None
}

/// Compare lists of triggers and return all differences
pub fn compare_trigger_lists(old: &[TriggerInfo], new: &[TriggerInfo]) -> Vec<TriggerDiff> {
    let mut results = Vec::new();

    let old_map: HashMap<(&str, &str), &TriggerInfo> = old
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.trigger_name.as_str()), t))
        .collect();

    let new_map: HashMap<(&str, &str), &TriggerInfo> = new
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.trigger_name.as_str()), t))
        .collect();

    // Check for added triggers
    for trig in new {
        let key = (trig.schema_name.as_str(), trig.trigger_name.as_str());
        if !old_map.contains_key(&key) {
            let mut diff = TriggerDiff {
                schema_name: trig.schema_name.clone(),
                trigger_name: trig.trigger_name.clone(),
                differences: Vec::new(),
            };
            let timing = extract_timing(&trig.definition).unwrap_or("unknown");
            let events = extract_events(&trig.definition).unwrap_or_else(|| "unknown".to_string());
            let func = extract_execute_function(&trig.definition).unwrap_or("unknown");
            diff.differences.push(format!(
                "CREATE TRIGGER: {}.{} — {} {} executing {}",
                trig.schema_name, trig.trigger_name, timing, events, func
            ));
            results.push(diff);
        }
    }

    // Check for removed triggers
    for trig in old {
        let key = (trig.schema_name.as_str(), trig.trigger_name.as_str());
        if !new_map.contains_key(&key) {
            let mut diff = TriggerDiff {
                schema_name: trig.schema_name.clone(),
                trigger_name: trig.trigger_name.clone(),
                differences: Vec::new(),
            };
            diff.differences.push(format!(
                "DROP TRIGGER: {}.{} — trigger no longer exists",
                trig.schema_name, trig.trigger_name
            ));
            results.push(diff);
        }
    }

    // Check for modified triggers
    for new_trig in new {
        let key = (
            new_trig.schema_name.as_str(),
            new_trig.trigger_name.as_str(),
        );
        if let Some(old_trig) = old_map.get(&key) {
            let diff = compare_triggers(old_trig, new_trig);
            if !diff.is_empty() {
                results.push(diff);
            }
        }
    }

    results
}
