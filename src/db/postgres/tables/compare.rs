use super::fetch::{ColumnInfo, TableInfo};
use std::collections::HashMap;

/// A detailed difference found when comparing tables
#[derive(Debug, Clone)]
pub struct TableDiff {
    pub schema_name: String,
    pub table_name: String,
    pub differences: Vec<String>,
}

impl TableDiff {
    pub fn is_empty(&self) -> bool {
        self.differences.is_empty()
    }
}

/// Compare two tables and return detailed differences
pub fn compare_tables(old: &TableInfo, new: &TableInfo) -> TableDiff {
    let mut differences = Vec::new();
    let full_name = format!("{}.{}", new.schema_name, new.table_name);

    // Build column maps
    let old_cols: HashMap<&str, &ColumnInfo> =
        old.columns.iter().map(|c| (c.name.as_str(), c)).collect();
    let new_cols: HashMap<&str, &ColumnInfo> =
        new.columns.iter().map(|c| (c.name.as_str(), c)).collect();

    // Check for added columns
    for col in &new.columns {
        if !old_cols.contains_key(col.name.as_str()) {
            differences.push(format!(
                "ADD COLUMN: {}.{} — {}",
                full_name,
                col.name,
                describe_column(col)
            ));
        }
    }

    // Check for removed columns
    for col in &old.columns {
        if !new_cols.contains_key(col.name.as_str()) {
            differences.push(format!(
                "DROP COLUMN: {}.{} — was {}",
                full_name,
                col.name,
                describe_column(col)
            ));
        }
    }

    // Check for modified columns
    for new_col in &new.columns {
        if let Some(old_col) = old_cols.get(new_col.name.as_str()) {
            compare_columns(&full_name, old_col, new_col, &mut differences);
        }
    }

    TableDiff {
        schema_name: new.schema_name.clone(),
        table_name: new.table_name.clone(),
        differences,
    }
}

/// Compare two columns and add any differences to the list
fn compare_columns(
    table_name: &str,
    old: &ColumnInfo,
    new: &ColumnInfo,
    differences: &mut Vec<String>,
) {
    let col_name = format!("{}.{}", table_name, new.name);

    // Type change
    if old.data_type != new.data_type {
        differences.push(format!(
            "ALTER COLUMN TYPE: {} — change from '{}' to '{}'",
            col_name, old.data_type, new.data_type
        ));
    }

    // Nullability change
    if old.is_nullable != new.is_nullable {
        if new.is_nullable {
            differences.push(format!(
                "DROP NOT NULL: {} — change from NOT NULL to nullable",
                col_name
            ));
        } else {
            differences.push(format!(
                "SET NOT NULL: {} — change from nullable to NOT NULL",
                col_name
            ));
        }
    }

    // Default value change
    match (&old.column_default, &new.column_default) {
        (None, Some(new_default)) => {
            differences.push(format!(
                "SET DEFAULT: {} — add default value '{}'",
                col_name, new_default
            ));
        }
        (Some(old_default), None) => {
            differences.push(format!(
                "DROP DEFAULT: {} — remove default value '{}' (was '{}')",
                col_name, old_default, old_default
            ));
        }
        (Some(old_default), Some(new_default)) if old_default != new_default => {
            differences.push(format!(
                "ALTER DEFAULT: {} — change default from '{}' to '{}'",
                col_name, old_default, new_default
            ));
        }
        _ => {}
    }

    // Identity change
    match (&old.identity_generation, &new.identity_generation) {
        (None, Some(new_identity)) => {
            let identity_type = identity_type_str(new_identity);
            differences.push(format!(
                "ADD IDENTITY: {} — add GENERATED {} AS IDENTITY",
                col_name, identity_type
            ));
        }
        (Some(old_identity), None) => {
            let identity_type = identity_type_str(old_identity);
            differences.push(format!(
                "DROP IDENTITY: {} — remove GENERATED {} AS IDENTITY",
                col_name, identity_type
            ));
        }
        (Some(old_identity), Some(new_identity)) if old_identity != new_identity => {
            let old_type = identity_type_str(old_identity);
            let new_type = identity_type_str(new_identity);
            differences.push(format!(
                "ALTER IDENTITY: {} — change from GENERATED {} AS IDENTITY to GENERATED {} AS IDENTITY",
                col_name, old_type, new_type
            ));
        }
        _ => {}
    }
}

/// Describe a column for display
fn describe_column(col: &ColumnInfo) -> String {
    let mut parts = vec![col.data_type.clone()];

    if let Some(ref identity) = col.identity_generation {
        let identity_type = identity_type_str(identity);
        parts.push(format!("GENERATED {} AS IDENTITY", identity_type));
    }

    if !col.is_nullable {
        parts.push("NOT NULL".to_string());
    }

    if let Some(ref default) = col.column_default {
        parts.push(format!("DEFAULT {}", default));
    }

    parts.join(" ")
}

/// Convert identity generation code to readable string
fn identity_type_str(code: &str) -> &'static str {
    match code {
        "a" => "ALWAYS",
        "d" => "BY DEFAULT",
        _ => "ALWAYS",
    }
}

/// Compare lists of tables and return all differences
pub fn compare_table_lists(old: &[TableInfo], new: &[TableInfo]) -> Vec<TableDiff> {
    let mut results = Vec::new();

    let old_map: HashMap<(&str, &str), &TableInfo> = old
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.table_name.as_str()), t))
        .collect();

    let new_map: HashMap<(&str, &str), &TableInfo> = new
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.table_name.as_str()), t))
        .collect();

    // Check for added tables
    for table in new {
        let key = (table.schema_name.as_str(), table.table_name.as_str());
        if !old_map.contains_key(&key) {
            let mut diff = TableDiff {
                schema_name: table.schema_name.clone(),
                table_name: table.table_name.clone(),
                differences: Vec::new(),
            };
            diff.differences.push(format!(
                "CREATE TABLE: {}.{} — new table with {} column(s): {}",
                table.schema_name,
                table.table_name,
                table.columns.len(),
                table
                    .columns
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            results.push(diff);
        }
    }

    // Check for removed tables
    for table in old {
        let key = (table.schema_name.as_str(), table.table_name.as_str());
        if !new_map.contains_key(&key) {
            let mut diff = TableDiff {
                schema_name: table.schema_name.clone(),
                table_name: table.table_name.clone(),
                differences: Vec::new(),
            };
            diff.differences.push(format!(
                "DROP TABLE: {}.{} — table no longer exists (had {} column(s))",
                table.schema_name,
                table.table_name,
                table.columns.len()
            ));
            results.push(diff);
        }
    }

    // Check for modified tables
    for new_table in new {
        let key = (
            new_table.schema_name.as_str(),
            new_table.table_name.as_str(),
        );
        if let Some(old_table) = old_map.get(&key) {
            let diff = compare_tables(old_table, new_table);
            if !diff.is_empty() {
                results.push(diff);
            }
        }
    }

    results
}
