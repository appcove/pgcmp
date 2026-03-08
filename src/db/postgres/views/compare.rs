use super::fetch::ViewInfo;
use std::collections::HashMap;

/// A detailed difference found when comparing views
#[derive(Debug, Clone)]
pub struct ViewDiff {
    pub schema_name: String,
    pub view_name: String,
    pub is_materialized: bool,
    pub differences: Vec<String>,
}

impl ViewDiff {
    pub fn is_empty(&self) -> bool {
        self.differences.is_empty()
    }
}

/// Compare two views and return detailed differences
pub fn compare_views(old: &ViewInfo, new: &ViewInfo, is_materialized: bool) -> ViewDiff {
    let mut differences = Vec::new();
    let full_name = format!("{}.{}", new.schema_name, new.view_name);
    let view_type = if is_materialized {
        "MATERIALIZED VIEW"
    } else {
        "VIEW"
    };

    // Normalize definitions for comparison (trim whitespace)
    let old_def = old.definition.trim();
    let new_def = new.definition.trim();

    if old_def != new_def {
        differences.push(format!(
            "ALTER {}: {} — definition changed\n  Old: {}\n  New: {}",
            view_type,
            full_name,
            truncate_for_display(old_def, 100),
            truncate_for_display(new_def, 100)
        ));
    }

    ViewDiff {
        schema_name: new.schema_name.clone(),
        view_name: new.view_name.clone(),
        is_materialized,
        differences,
    }
}

/// Compare lists of views and return all differences
pub fn compare_view_lists(
    old: &[ViewInfo],
    new: &[ViewInfo],
    is_materialized: bool,
) -> Vec<ViewDiff> {
    let mut results = Vec::new();
    let view_type = if is_materialized {
        "MATERIALIZED VIEW"
    } else {
        "VIEW"
    };

    let old_map: HashMap<(&str, &str), &ViewInfo> = old
        .iter()
        .map(|v| ((v.schema_name.as_str(), v.view_name.as_str()), v))
        .collect();

    let new_map: HashMap<(&str, &str), &ViewInfo> = new
        .iter()
        .map(|v| ((v.schema_name.as_str(), v.view_name.as_str()), v))
        .collect();

    // Check for added views
    for view in new {
        let key = (view.schema_name.as_str(), view.view_name.as_str());
        if !old_map.contains_key(&key) {
            let mut diff = ViewDiff {
                schema_name: view.schema_name.clone(),
                view_name: view.view_name.clone(),
                is_materialized,
                differences: Vec::new(),
            };
            diff.differences.push(format!(
                "CREATE {}: {}.{} — new {} with definition: {}",
                view_type,
                view.schema_name,
                view.view_name,
                view_type.to_lowercase(),
                truncate_for_display(&view.definition, 80)
            ));
            results.push(diff);
        }
    }

    // Check for removed views
    for view in old {
        let key = (view.schema_name.as_str(), view.view_name.as_str());
        if !new_map.contains_key(&key) {
            let mut diff = ViewDiff {
                schema_name: view.schema_name.clone(),
                view_name: view.view_name.clone(),
                is_materialized,
                differences: Vec::new(),
            };
            diff.differences.push(format!(
                "DROP {}: {}.{} — {} no longer exists",
                view_type,
                view.schema_name,
                view.view_name,
                view_type.to_lowercase()
            ));
            results.push(diff);
        }
    }

    // Check for modified views
    for new_view in new {
        let key = (new_view.schema_name.as_str(), new_view.view_name.as_str());
        if let Some(old_view) = old_map.get(&key) {
            let diff = compare_views(old_view, new_view, is_materialized);
            if !diff.is_empty() {
                results.push(diff);
            }
        }
    }

    results
}

/// Truncate a string for display, adding ellipsis if needed
fn truncate_for_display(s: &str, max_len: usize) -> String {
    let s = s.replace('\n', " ").replace("  ", " ");
    if s.len() <= max_len {
        s
    } else {
        format!("{}...", &s[..max_len])
    }
}
