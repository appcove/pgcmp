use super::fetch::{TypeInfo, TypeKind};
use std::collections::HashMap;

/// A detailed difference found when comparing types
#[derive(Debug, Clone)]
pub struct TypeDiff {
    pub schema_name: String,
    pub type_name: String,
    pub differences: Vec<String>,
}

impl TypeDiff {
    pub fn is_empty(&self) -> bool {
        self.differences.is_empty()
    }
}

/// Compare two types and return detailed differences
pub fn compare_types(old: &TypeInfo, new: &TypeInfo) -> TypeDiff {
    let mut differences = Vec::new();
    let full_name = format!("{}.{}", new.schema_name, new.type_name);

    // Check if type kind changed (this is a major change)
    if old.kind != new.kind {
        differences.push(format!(
            "TYPE KIND CHANGED: {} — was {} type, now {} type (requires DROP and CREATE)",
            full_name,
            old.kind.label(),
            new.kind.label()
        ));
        return TypeDiff {
            schema_name: new.schema_name.clone(),
            type_name: new.type_name.clone(),
            differences,
        };
    }

    // Compare based on type kind
    match new.kind {
        TypeKind::Enum => compare_enum_types(old, new, &full_name, &mut differences),
        TypeKind::Composite => compare_composite_types(old, new, &full_name, &mut differences),
        TypeKind::Domain => compare_domain_types(old, new, &full_name, &mut differences),
        TypeKind::Range => compare_range_types(old, new, &full_name, &mut differences),
    }

    TypeDiff {
        schema_name: new.schema_name.clone(),
        type_name: new.type_name.clone(),
        differences,
    }
}

fn compare_enum_types(old: &TypeInfo, new: &TypeInfo, full_name: &str, differences: &mut Vec<String>) {
    // Check for added enum values
    for (idx, label) in new.enum_labels.iter().enumerate() {
        if !old.enum_labels.contains(label) {
            let position = if idx == 0 {
                "FIRST".to_string()
            } else {
                format!("AFTER '{}'", new.enum_labels[idx - 1])
            };
            differences.push(format!(
                "ADD ENUM VALUE: {} — add '{}' {}",
                full_name, label, position
            ));
        }
    }

    // Check for removed enum values (PostgreSQL doesn't support this easily)
    for label in &old.enum_labels {
        if !new.enum_labels.contains(label) {
            differences.push(format!(
                "REMOVE ENUM VALUE: {} — remove '{}' (requires recreating type)",
                full_name, label
            ));
        }
    }

    // Check for reordered enum values (also problematic)
    let old_existing: Vec<&String> = old
        .enum_labels
        .iter()
        .filter(|l| new.enum_labels.contains(l))
        .collect();
    let new_existing: Vec<&String> = new
        .enum_labels
        .iter()
        .filter(|l| old.enum_labels.contains(l))
        .collect();

    if old_existing != new_existing {
        differences.push(format!(
            "ENUM ORDER CHANGED: {} — enum values reordered (requires recreating type)",
            full_name
        ));
    }
}

fn compare_composite_types(
    old: &TypeInfo,
    new: &TypeInfo,
    full_name: &str,
    differences: &mut Vec<String>,
) {
    let old_attrs: HashMap<&str, &str> = old
        .composite_attrs
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();
    let new_attrs: HashMap<&str, &str> = new
        .composite_attrs
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();

    // Check for added attributes
    for (name, typ) in &new.composite_attrs {
        if !old_attrs.contains_key(name.as_str()) {
            differences.push(format!(
                "ADD COMPOSITE ATTRIBUTE: {} — add attribute {} {}",
                full_name, name, typ
            ));
        }
    }

    // Check for removed attributes
    for (name, _) in &old.composite_attrs {
        if !new_attrs.contains_key(name.as_str()) {
            differences.push(format!(
                "DROP COMPOSITE ATTRIBUTE: {} — remove attribute {}",
                full_name, name
            ));
        }
    }

    // Check for modified attributes
    for (name, new_type) in &new.composite_attrs {
        if let Some(old_type) = old_attrs.get(name.as_str()) {
            if old_type != new_type {
                differences.push(format!(
                    "ALTER COMPOSITE ATTRIBUTE: {} — change {} from {} to {}",
                    full_name, name, old_type, new_type
                ));
            }
        }
    }
}

fn compare_domain_types(old: &TypeInfo, new: &TypeInfo, full_name: &str, differences: &mut Vec<String>) {
    // Check base type
    if old.domain_base_type != new.domain_base_type {
        differences.push(format!(
            "DOMAIN BASE TYPE: {} — change base type from {:?} to {:?} (requires recreating)",
            full_name, old.domain_base_type, new.domain_base_type
        ));
    }

    // Check NOT NULL
    if old.domain_not_null != new.domain_not_null {
        if new.domain_not_null {
            differences.push(format!(
                "DOMAIN SET NOT NULL: {} — add NOT NULL constraint",
                full_name
            ));
        } else {
            differences.push(format!(
                "DOMAIN DROP NOT NULL: {} — remove NOT NULL constraint",
                full_name
            ));
        }
    }

    // Check default
    if old.domain_default != new.domain_default {
        match (&old.domain_default, &new.domain_default) {
            (None, Some(d)) => {
                differences.push(format!(
                    "DOMAIN SET DEFAULT: {} — set default to {}",
                    full_name, d
                ));
            }
            (Some(_), None) => {
                differences.push(format!(
                    "DOMAIN DROP DEFAULT: {} — remove default value",
                    full_name
                ));
            }
            (Some(old_d), Some(new_d)) => {
                differences.push(format!(
                    "DOMAIN ALTER DEFAULT: {} — change default from {} to {}",
                    full_name, old_d, new_d
                ));
            }
            _ => {}
        }
    }

    // Check constraint
    if old.domain_constraint != new.domain_constraint {
        match (&old.domain_constraint, &new.domain_constraint) {
            (None, Some(c)) => {
                differences.push(format!(
                    "DOMAIN ADD CONSTRAINT: {} — add constraint {}",
                    full_name, c
                ));
            }
            (Some(_), None) => {
                differences.push(format!(
                    "DOMAIN DROP CONSTRAINT: {} — remove constraint",
                    full_name
                ));
            }
            (Some(old_c), Some(new_c)) => {
                differences.push(format!(
                    "DOMAIN ALTER CONSTRAINT: {} — change constraint from {} to {}",
                    full_name, old_c, new_c
                ));
            }
            _ => {}
        }
    }
}

fn compare_range_types(old: &TypeInfo, new: &TypeInfo, full_name: &str, differences: &mut Vec<String>) {
    if old.range_subtype != new.range_subtype {
        differences.push(format!(
            "RANGE SUBTYPE: {} — change subtype from {:?} to {:?} (requires recreating)",
            full_name, old.range_subtype, new.range_subtype
        ));
    }

    if old.range_subtype_opclass != new.range_subtype_opclass {
        differences.push(format!(
            "RANGE OPCLASS: {} — change subtype_opclass from {:?} to {:?}",
            full_name, old.range_subtype_opclass, new.range_subtype_opclass
        ));
    }

    if old.range_canonical != new.range_canonical {
        differences.push(format!(
            "RANGE CANONICAL: {} — change canonical function from {:?} to {:?}",
            full_name, old.range_canonical, new.range_canonical
        ));
    }

    if old.range_subtype_diff != new.range_subtype_diff {
        differences.push(format!(
            "RANGE SUBTYPE_DIFF: {} — change subtype_diff function from {:?} to {:?}",
            full_name, old.range_subtype_diff, new.range_subtype_diff
        ));
    }
}

/// Compare lists of types and return all differences
pub fn compare_type_lists(old: &[TypeInfo], new: &[TypeInfo]) -> Vec<TypeDiff> {
    let mut results = Vec::new();

    let old_map: HashMap<(&str, &str), &TypeInfo> = old
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.type_name.as_str()), t))
        .collect();

    let new_map: HashMap<(&str, &str), &TypeInfo> = new
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.type_name.as_str()), t))
        .collect();

    // Check for added types
    for type_info in new {
        let key = (type_info.schema_name.as_str(), type_info.type_name.as_str());
        if !old_map.contains_key(&key) {
            let mut diff = TypeDiff {
                schema_name: type_info.schema_name.clone(),
                type_name: type_info.type_name.clone(),
                differences: Vec::new(),
            };
            diff.differences.push(format!(
                "CREATE TYPE: {}.{} — new {} type",
                type_info.schema_name,
                type_info.type_name,
                type_info.kind.label()
            ));
            results.push(diff);
        }
    }

    // Check for removed types
    for type_info in old {
        let key = (type_info.schema_name.as_str(), type_info.type_name.as_str());
        if !new_map.contains_key(&key) {
            let mut diff = TypeDiff {
                schema_name: type_info.schema_name.clone(),
                type_name: type_info.type_name.clone(),
                differences: Vec::new(),
            };
            diff.differences.push(format!(
                "DROP TYPE: {}.{} — {} type no longer exists",
                type_info.schema_name,
                type_info.type_name,
                type_info.kind.label()
            ));
            results.push(diff);
        }
    }

    // Check for modified types
    for new_type in new {
        let key = (new_type.schema_name.as_str(), new_type.type_name.as_str());
        if let Some(old_type) = old_map.get(&key) {
            let diff = compare_types(old_type, new_type);
            if !diff.is_empty() {
                results.push(diff);
            }
        }
    }

    results
}
