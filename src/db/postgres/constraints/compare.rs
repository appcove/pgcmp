use super::fetch::ConstraintInfo;
use std::collections::HashMap;

/// A detailed difference found when comparing constraints
#[derive(Debug, Clone)]
pub struct ConstraintDiff {
    pub schema_name: String,
    pub constraint_name: String,
    pub differences: Vec<String>,
}

impl ConstraintDiff {
    pub fn is_empty(&self) -> bool {
        self.differences.is_empty()
    }
}

/// Compare two constraints and return detailed differences
pub fn compare_constraints(old: &ConstraintInfo, new: &ConstraintInfo) -> ConstraintDiff {
    let mut differences = Vec::new();
    let full_name = format!(
        "{}.{}.{}",
        new.schema_name, new.table_name, new.constraint_name
    );

    // Check constraint type change (shouldn't happen but let's handle it)
    if old.constraint_type != new.constraint_type {
        differences.push(format!(
            "CHANGE CONSTRAINT TYPE: {} — changed from {} to {} (requires DROP and CREATE)",
            full_name, old.constraint_type, new.constraint_type
        ));
    }

    // Check definition change
    let old_def = normalize_constraint_def(&old.definition);
    let new_def = normalize_constraint_def(&new.definition);

    if old_def != new_def {
        let change_desc = describe_constraint_change(old, new);
        differences.push(format!(
            "ALTER CONSTRAINT: {} ({}) — {}",
            full_name, new.constraint_type, change_desc
        ));
    }

    ConstraintDiff {
        schema_name: new.schema_name.clone(),
        constraint_name: new.constraint_name.clone(),
        differences,
    }
}

/// Normalize constraint definition for comparison
fn normalize_constraint_def(def: &str) -> String {
    def.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// Describe what changed in a constraint
fn describe_constraint_change(old: &ConstraintInfo, new: &ConstraintInfo) -> String {
    match new.constraint_type.as_str() {
        "PRIMARY KEY" => {
            let old_cols = extract_columns(&old.definition);
            let new_cols = extract_columns(&new.definition);
            format!(
                "primary key columns changed from ({}) to ({})",
                old_cols.unwrap_or("unknown"),
                new_cols.unwrap_or("unknown")
            )
        }
        "FOREIGN KEY" => {
            let old_ref = extract_fk_reference(&old.definition);
            let new_ref = extract_fk_reference(&new.definition);
            format!(
                "foreign key reference changed from '{}' to '{}'",
                old_ref.unwrap_or("unknown"),
                new_ref.unwrap_or("unknown")
            )
        }
        "UNIQUE" => {
            let old_cols = extract_columns(&old.definition);
            let new_cols = extract_columns(&new.definition);
            format!(
                "unique constraint columns changed from ({}) to ({})",
                old_cols.unwrap_or("unknown"),
                new_cols.unwrap_or("unknown")
            )
        }
        "CHECK" => {
            format!(
                "check expression changed from '{}' to '{}'",
                old.definition, new.definition
            )
        }
        "EXCLUDE" => {
            format!(
                "exclusion constraint changed from '{}' to '{}'",
                old.definition, new.definition
            )
        }
        _ => format!(
            "definition changed from '{}' to '{}'",
            old.definition, new.definition
        ),
    }
}

/// Extract columns from constraint definition
fn extract_columns(def: &str) -> Option<&str> {
    if let Some(start) = def.find('(') {
        let rest = &def[start + 1..];
        if let Some(end) = rest.find(')') {
            return Some(rest[..end].trim());
        }
    }
    None
}

/// Extract foreign key reference from definition
fn extract_fk_reference(def: &str) -> Option<&str> {
    let upper = def.to_uppercase();
    if let Some(pos) = upper.find("REFERENCES ") {
        Some(def[pos + 11..].trim())
    } else {
        None
    }
}

/// Compare lists of constraints and return all differences
pub fn compare_constraint_lists(
    old: &[ConstraintInfo],
    new: &[ConstraintInfo],
) -> Vec<ConstraintDiff> {
    let mut results = Vec::new();

    // Key includes table name since constraints are per-table
    let old_map: HashMap<(&str, &str, &str), &ConstraintInfo> = old
        .iter()
        .map(|c| {
            (
                (
                    c.schema_name.as_str(),
                    c.table_name.as_str(),
                    c.constraint_name.as_str(),
                ),
                c,
            )
        })
        .collect();

    let new_map: HashMap<(&str, &str, &str), &ConstraintInfo> = new
        .iter()
        .map(|c| {
            (
                (
                    c.schema_name.as_str(),
                    c.table_name.as_str(),
                    c.constraint_name.as_str(),
                ),
                c,
            )
        })
        .collect();

    // Check for added constraints
    for con in new {
        let key = (
            con.schema_name.as_str(),
            con.table_name.as_str(),
            con.constraint_name.as_str(),
        );
        if !old_map.contains_key(&key) {
            let mut diff = ConstraintDiff {
                schema_name: con.schema_name.clone(),
                constraint_name: con.constraint_name.clone(),
                differences: Vec::new(),
            };
            let detail = describe_new_constraint(con);
            diff.differences.push(format!(
                "ADD CONSTRAINT: {}.{}.{} — {}",
                con.schema_name, con.table_name, con.constraint_name, detail
            ));
            results.push(diff);
        }
    }

    // Check for removed constraints
    for con in old {
        let key = (
            con.schema_name.as_str(),
            con.table_name.as_str(),
            con.constraint_name.as_str(),
        );
        if !new_map.contains_key(&key) {
            let mut diff = ConstraintDiff {
                schema_name: con.schema_name.clone(),
                constraint_name: con.constraint_name.clone(),
                differences: Vec::new(),
            };
            diff.differences.push(format!(
                "DROP CONSTRAINT: {}.{}.{} — {} constraint no longer exists",
                con.schema_name, con.table_name, con.constraint_name, con.constraint_type
            ));
            results.push(diff);
        }
    }

    // Check for modified constraints
    for new_con in new {
        let key = (
            new_con.schema_name.as_str(),
            new_con.table_name.as_str(),
            new_con.constraint_name.as_str(),
        );
        if let Some(old_con) = old_map.get(&key) {
            let diff = compare_constraints(old_con, new_con);
            if !diff.is_empty() {
                results.push(diff);
            }
        }
    }

    results
}

/// Describe a new constraint
fn describe_new_constraint(con: &ConstraintInfo) -> String {
    match con.constraint_type.as_str() {
        "PRIMARY KEY" => {
            let cols = extract_columns(&con.definition).unwrap_or("unknown");
            format!("new PRIMARY KEY on ({})", cols)
        }
        "FOREIGN KEY" => {
            let cols = extract_columns(&con.definition).unwrap_or("unknown");
            let refs = extract_fk_reference(&con.definition).unwrap_or("unknown");
            format!("new FOREIGN KEY ({}) REFERENCES {}", cols, refs)
        }
        "UNIQUE" => {
            let cols = extract_columns(&con.definition).unwrap_or("unknown");
            format!("new UNIQUE constraint on ({})", cols)
        }
        "CHECK" => {
            format!("new CHECK constraint: {}", con.definition)
        }
        "EXCLUDE" => {
            format!("new EXCLUSION constraint: {}", con.definition)
        }
        _ => format!("new {} constraint: {}", con.constraint_type, con.definition),
    }
}
