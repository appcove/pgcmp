use super::fetch::FunctionInfo;
use std::collections::HashMap;

/// A detailed difference found when comparing functions
#[derive(Debug, Clone)]
pub struct FunctionDiff {
    pub schema_name: String,
    pub function_name: String,
    pub differences: Vec<String>,
}

impl FunctionDiff {
    pub fn is_empty(&self) -> bool {
        self.differences.is_empty()
    }
}

/// Compare two functions and return detailed differences
pub fn compare_functions(old: &FunctionInfo, new: &FunctionInfo) -> FunctionDiff {
    let mut differences = Vec::new();
    let full_name = format!("{}.{}", new.schema_name, new.function_name);

    // Normalize definitions for comparison
    let old_def = normalize_function_def(&old.definition);
    let new_def = normalize_function_def(&new.definition);

    if old_def != new_def {
        // Try to identify what changed
        let changes = identify_function_changes(&old.definition, &new.definition);
        if changes.is_empty() {
            differences.push(format!(
                "ALTER FUNCTION: {} — definition changed (use diff tool for details)",
                full_name
            ));
        } else {
            for change in changes {
                differences.push(format!("ALTER FUNCTION: {} — {}", full_name, change));
            }
        }
    }

    FunctionDiff {
        schema_name: new.schema_name.clone(),
        function_name: new.function_name.clone(),
        differences,
    }
}

/// Normalize function definition for comparison
fn normalize_function_def(def: &str) -> String {
    def.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Try to identify specific changes between function definitions
fn identify_function_changes(old: &str, new: &str) -> Vec<String> {
    let mut changes = Vec::new();

    // Check for RETURNS clause change
    let old_returns = extract_returns(old);
    let new_returns = extract_returns(new);
    if old_returns != new_returns {
        changes.push(format!(
            "return type changed from '{}' to '{}'",
            old_returns.unwrap_or("unknown"),
            new_returns.unwrap_or("unknown")
        ));
    }

    // Check for LANGUAGE clause change
    let old_lang = extract_language(old);
    let new_lang = extract_language(new);
    if old_lang != new_lang {
        changes.push(format!(
            "language changed from '{}' to '{}'",
            old_lang.unwrap_or("unknown"),
            new_lang.unwrap_or("unknown")
        ));
    }

    // Check for volatility change (IMMUTABLE, STABLE, VOLATILE)
    let old_vol = extract_volatility(old);
    let new_vol = extract_volatility(new);
    if old_vol != new_vol {
        changes.push(format!(
            "volatility changed from '{}' to '{}'",
            old_vol.unwrap_or("VOLATILE"),
            new_vol.unwrap_or("VOLATILE")
        ));
    }

    // Check for SECURITY change
    let old_sec = old.to_uppercase().contains("SECURITY DEFINER");
    let new_sec = new.to_uppercase().contains("SECURITY DEFINER");
    if old_sec != new_sec {
        if new_sec {
            changes.push("added SECURITY DEFINER".to_string());
        } else {
            changes.push("removed SECURITY DEFINER (now SECURITY INVOKER)".to_string());
        }
    }

    // If we couldn't identify specific changes, the body probably changed
    if changes.is_empty() {
        changes.push("function body changed".to_string());
    }

    changes
}

/// Extract RETURNS clause from function definition
fn extract_returns(def: &str) -> Option<&str> {
    let upper = def.to_uppercase();
    if let Some(pos) = upper.find("RETURNS ") {
        let start = pos + 8;
        let rest = &def[start..];
        // Find end of returns clause (AS, LANGUAGE, or newline)
        let end = rest
            .find('\n')
            .or_else(|| {
                let upper_rest = rest.to_uppercase();
                upper_rest
                    .find(" AS ")
                    .or_else(|| upper_rest.find(" LANGUAGE "))
            })
            .unwrap_or(rest.len());
        Some(rest[..end].trim())
    } else {
        None
    }
}

/// Extract LANGUAGE clause from function definition
fn extract_language(def: &str) -> Option<&str> {
    let upper = def.to_uppercase();
    if let Some(pos) = upper.find("LANGUAGE ") {
        let start = pos + 9;
        let rest = &def[start..];
        let end = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        Some(rest[..end].trim())
    } else {
        None
    }
}

/// Extract volatility from function definition
fn extract_volatility(def: &str) -> Option<&'static str> {
    let upper = def.to_uppercase();
    if upper.contains("IMMUTABLE") {
        Some("IMMUTABLE")
    } else if upper.contains("STABLE") {
        Some("STABLE")
    } else if upper.contains("VOLATILE") {
        Some("VOLATILE")
    } else {
        None
    }
}

/// Compare lists of functions and return all differences
pub fn compare_function_lists(old: &[FunctionInfo], new: &[FunctionInfo]) -> Vec<FunctionDiff> {
    let mut results = Vec::new();

    let old_map: HashMap<(&str, &str), &FunctionInfo> = old
        .iter()
        .map(|f| ((f.schema_name.as_str(), f.function_name.as_str()), f))
        .collect();

    let new_map: HashMap<(&str, &str), &FunctionInfo> = new
        .iter()
        .map(|f| ((f.schema_name.as_str(), f.function_name.as_str()), f))
        .collect();

    // Check for added functions
    for func in new {
        let key = (func.schema_name.as_str(), func.function_name.as_str());
        if !old_map.contains_key(&key) {
            let mut diff = FunctionDiff {
                schema_name: func.schema_name.clone(),
                function_name: func.function_name.clone(),
                differences: Vec::new(),
            };
            let returns = extract_returns(&func.definition).unwrap_or("unknown");
            let lang = extract_language(&func.definition).unwrap_or("unknown");
            diff.differences.push(format!(
                "CREATE FUNCTION: {}.{} — new function returning '{}' in language '{}'",
                func.schema_name, func.function_name, returns, lang
            ));
            results.push(diff);
        }
    }

    // Check for removed functions
    for func in old {
        let key = (func.schema_name.as_str(), func.function_name.as_str());
        if !new_map.contains_key(&key) {
            let mut diff = FunctionDiff {
                schema_name: func.schema_name.clone(),
                function_name: func.function_name.clone(),
                differences: Vec::new(),
            };
            diff.differences.push(format!(
                "DROP FUNCTION: {}.{} — function no longer exists",
                func.schema_name, func.function_name
            ));
            results.push(diff);
        }
    }

    // Check for modified functions
    for new_func in new {
        let key = (
            new_func.schema_name.as_str(),
            new_func.function_name.as_str(),
        );
        if let Some(old_func) = old_map.get(&key) {
            let diff = compare_functions(old_func, new_func);
            if !diff.is_empty() {
                results.push(diff);
            }
        }
    }

    results
}
