use super::fetch::IndexInfo;
use std::collections::HashMap;

/// A detailed difference found when comparing indexes
#[derive(Debug, Clone)]
pub struct IndexDiff {
    pub schema_name: String,
    pub index_name: String,
    pub differences: Vec<String>,
}

impl IndexDiff {
    pub fn is_empty(&self) -> bool {
        self.differences.is_empty()
    }
}

/// Compare two indexes and return detailed differences
pub fn compare_indexes(old: &IndexInfo, new: &IndexInfo) -> IndexDiff {
    let mut differences = Vec::new();
    let full_name = format!("{}.{}", new.schema_name, new.index_name);

    // Normalize definitions for comparison
    let old_def = normalize_index_def(&old.definition);
    let new_def = normalize_index_def(&new.definition);

    if old_def != new_def {
        let changes = identify_index_changes(&old.definition, &new.definition);
        for change in changes {
            differences.push(format!("ALTER INDEX: {} — {}", full_name, change));
        }
    }

    IndexDiff {
        schema_name: new.schema_name.clone(),
        index_name: new.index_name.clone(),
        differences,
    }
}

/// Normalize index definition for comparison
fn normalize_index_def(def: &str) -> String {
    def.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// Try to identify specific changes between index definitions
fn identify_index_changes(old: &str, new: &str) -> Vec<String> {
    let mut changes = Vec::new();

    // Check for UNIQUE change
    let old_unique = old.to_uppercase().contains("UNIQUE INDEX");
    let new_unique = new.to_uppercase().contains("UNIQUE INDEX");
    if old_unique != new_unique {
        if new_unique {
            changes.push("changed to UNIQUE index".to_string());
        } else {
            changes.push("changed from UNIQUE to non-unique index".to_string());
        }
    }

    // Check for method change (btree, hash, gist, gin, etc.)
    let old_method = extract_index_method(old);
    let new_method = extract_index_method(new);
    if old_method != new_method {
        changes.push(format!(
            "index method changed from '{}' to '{}'",
            old_method.unwrap_or("btree"),
            new_method.unwrap_or("btree")
        ));
    }

    // Check for columns change
    let old_cols = extract_index_columns(old);
    let new_cols = extract_index_columns(new);
    if old_cols != new_cols {
        changes.push(format!(
            "index columns changed from '{}' to '{}'",
            old_cols.unwrap_or("unknown"),
            new_cols.unwrap_or("unknown")
        ));
    }

    // Check for WHERE clause change
    let old_where = extract_where_clause(old);
    let new_where = extract_where_clause(new);
    match (&old_where, &new_where) {
        (None, Some(w)) => {
            changes.push(format!("added WHERE clause: {}", w));
        }
        (Some(w), None) => {
            changes.push(format!("removed WHERE clause (was: {})", w));
        }
        (Some(old_w), Some(new_w)) if old_w != new_w => {
            changes.push(format!(
                "WHERE clause changed from '{}' to '{}'",
                old_w, new_w
            ));
        }
        _ => {}
    }

    if changes.is_empty() {
        changes.push("index definition changed".to_string());
    }

    changes
}

/// Extract index method (USING clause)
fn extract_index_method(def: &str) -> Option<&str> {
    let upper = def.to_uppercase();
    if let Some(pos) = upper.find(" USING ") {
        let start = pos + 7;
        let rest = &def[start..];
        let end = rest
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(rest.len());
        Some(rest[..end].trim())
    } else {
        None
    }
}

/// Extract columns from index definition
fn extract_index_columns(def: &str) -> Option<&str> {
    // Find content between first ( and matching )
    if let Some(start) = def.find('(') {
        let rest = &def[start + 1..];
        if let Some(end) = rest.find(')') {
            return Some(rest[..end].trim());
        }
    }
    None
}

/// Extract WHERE clause from partial index
fn extract_where_clause(def: &str) -> Option<&str> {
    let upper = def.to_uppercase();
    if let Some(pos) = upper.find(" WHERE ") {
        Some(def[pos + 7..].trim())
    } else {
        None
    }
}

/// Compare lists of indexes and return all differences
pub fn compare_index_lists(old: &[IndexInfo], new: &[IndexInfo]) -> Vec<IndexDiff> {
    let mut results = Vec::new();

    let old_map: HashMap<(&str, &str), &IndexInfo> = old
        .iter()
        .map(|i| ((i.schema_name.as_str(), i.index_name.as_str()), i))
        .collect();

    let new_map: HashMap<(&str, &str), &IndexInfo> = new
        .iter()
        .map(|i| ((i.schema_name.as_str(), i.index_name.as_str()), i))
        .collect();

    // Check for added indexes
    for idx in new {
        let key = (idx.schema_name.as_str(), idx.index_name.as_str());
        if !old_map.contains_key(&key) {
            let mut diff = IndexDiff {
                schema_name: idx.schema_name.clone(),
                index_name: idx.index_name.clone(),
                differences: Vec::new(),
            };
            let cols = extract_index_columns(&idx.definition).unwrap_or("unknown");
            let method = extract_index_method(&idx.definition).unwrap_or("btree");
            let unique = if idx.definition.to_uppercase().contains("UNIQUE") {
                "UNIQUE "
            } else {
                ""
            };
            diff.differences.push(format!(
                "CREATE INDEX: {}.{} — new {}index using {} on ({})",
                idx.schema_name, idx.index_name, unique, method, cols
            ));
            results.push(diff);
        }
    }

    // Check for removed indexes
    for idx in old {
        let key = (idx.schema_name.as_str(), idx.index_name.as_str());
        if !new_map.contains_key(&key) {
            let mut diff = IndexDiff {
                schema_name: idx.schema_name.clone(),
                index_name: idx.index_name.clone(),
                differences: Vec::new(),
            };
            diff.differences.push(format!(
                "DROP INDEX: {}.{} — index no longer exists",
                idx.schema_name, idx.index_name
            ));
            results.push(diff);
        }
    }

    // Check for modified indexes
    for new_idx in new {
        let key = (new_idx.schema_name.as_str(), new_idx.index_name.as_str());
        if let Some(old_idx) = old_map.get(&key) {
            let diff = compare_indexes(old_idx, new_idx);
            if !diff.is_empty() {
                results.push(diff);
            }
        }
    }

    results
}
