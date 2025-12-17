use crate::schema::{ObjectType, SchemaObject};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatus {
    Added,
    Removed,
    Modified,
    Unchanged,
}

#[derive(Debug, Clone)]
pub struct DiffEntry {
    pub schema_name: String,
    pub object_name: String,
    pub object_type: ObjectType,
    pub status: DiffStatus,
    pub old_ddl: Option<String>,
    pub new_ddl: Option<String>,
}

#[derive(Debug, Default)]
pub struct SchemaDiff {
    pub entries: Vec<DiffEntry>,
}

impl SchemaDiff {
    pub fn has_differences(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.status != DiffStatus::Unchanged)
    }

    pub fn added(&self) -> impl Iterator<Item = &DiffEntry> {
        self.entries
            .iter()
            .filter(|e| e.status == DiffStatus::Added)
    }

    pub fn removed(&self) -> impl Iterator<Item = &DiffEntry> {
        self.entries
            .iter()
            .filter(|e| e.status == DiffStatus::Removed)
    }

    pub fn modified(&self) -> impl Iterator<Item = &DiffEntry> {
        self.entries
            .iter()
            .filter(|e| e.status == DiffStatus::Modified)
    }
}

/// Compare two sets of schema objects
pub fn compare_schemas(old: &[SchemaObject], new: &[SchemaObject]) -> SchemaDiff {
    let mut entries = Vec::new();

    // Build lookup map for old objects
    let old_map: HashMap<_, _> = old
        .iter()
        .map(|o| ((&o.schema_name, &o.object_name, o.object_type), o))
        .collect();

    // Build lookup map for new objects
    let new_map: HashMap<_, _> = new
        .iter()
        .map(|o| ((&o.schema_name, &o.object_name, o.object_type), o))
        .collect();

    // Check all new objects
    for obj in new {
        let key = (&obj.schema_name, &obj.object_name, obj.object_type);
        match old_map.get(&key) {
            Some(old_obj) => {
                let status = if old_obj.ddl == obj.ddl {
                    DiffStatus::Unchanged
                } else {
                    DiffStatus::Modified
                };
                entries.push(DiffEntry {
                    schema_name: obj.schema_name.clone(),
                    object_name: obj.object_name.clone(),
                    object_type: obj.object_type,
                    status,
                    old_ddl: Some(old_obj.ddl.clone()),
                    new_ddl: Some(obj.ddl.clone()),
                });
            }
            None => {
                entries.push(DiffEntry {
                    schema_name: obj.schema_name.clone(),
                    object_name: obj.object_name.clone(),
                    object_type: obj.object_type,
                    status: DiffStatus::Added,
                    old_ddl: None,
                    new_ddl: Some(obj.ddl.clone()),
                });
            }
        }
    }

    // Check for removed objects
    for obj in old {
        let key = (&obj.schema_name, &obj.object_name, obj.object_type);
        if !new_map.contains_key(&key) {
            entries.push(DiffEntry {
                schema_name: obj.schema_name.clone(),
                object_name: obj.object_name.clone(),
                object_type: obj.object_type,
                status: DiffStatus::Removed,
                old_ddl: Some(obj.ddl.clone()),
                new_ddl: None,
            });
        }
    }

    SchemaDiff { entries }
}
