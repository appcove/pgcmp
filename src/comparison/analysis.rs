//! Live comparison of two databases, shared by `pgcmp diff` and `pgcmp test`.
//!
//! "Left" is the new (target) database and "right" is the old one.

use super::normalize::{
    normalize_constraint_definition, normalize_function_def, normalize_trigger_def,
    normalize_view_definition,
};
use crate::db::postgres::constraints::{ConstraintInfo, fetch_constraints};
use crate::db::postgres::extensions::{ExtensionInfo, fetch_extensions};
use crate::db::postgres::functions::{FunctionInfo, fetch_functions};
use crate::db::postgres::indexes::{IndexInfo, fetch_indexes};
use crate::db::postgres::sequences::{SequenceInfo, fetch_sequences};
use crate::db::postgres::tables::{TableInfo, fetch_tables};
use crate::db::postgres::triggers::{TriggerInfo, fetch_triggers};
use crate::db::postgres::types::{TypeInfo, fetch_types};
use crate::db::postgres::views::{ViewInfo, fetch_materialized_views, fetch_views};
use crate::db::postgres::{DbConnection, DbPool};
use std::collections::{HashMap, HashSet};

pub async fn get_postgres_version(conn: &DbConnection) -> anyhow::Result<String> {
    let row = conn.client().query_one("SHOW server_version", &[]).await?;
    let version: String = row.get(0);
    // Extract major version (e.g., "15.4" -> "15")
    let major = version.split('.').next().unwrap_or(&version);
    Ok(major.to_string())
}

// =============================================================================
// Analysis Types
// =============================================================================

/// Summary row for an object type
pub struct SummaryRow {
    pub object_type: &'static str,
    pub left_count: usize,
    pub right_count: usize,
}

impl SummaryRow {
    pub fn is_different(&self) -> bool {
        self.left_count != self.right_count
    }
}

/// Analysis result for a single object
pub struct ObjectAnalysis {
    pub name: String, // Full qualified name
    pub action_description: String,
    pub modification_detail: Option<String>,
}

/// Complete analysis result
pub struct AnalysisResult {
    pub summary: Vec<SummaryRow>,
    pub schemas: Vec<ObjectAnalysis>,
    pub extensions: Vec<ObjectAnalysis>,
    pub types: Vec<ObjectAnalysis>,
    pub tables: Vec<ObjectAnalysis>,
    pub columns: Vec<ObjectAnalysis>,
    pub views: Vec<ObjectAnalysis>,
    pub materialized_views: Vec<ObjectAnalysis>,
    pub functions: Vec<ObjectAnalysis>,
    pub indexes: Vec<ObjectAnalysis>,
    pub constraints: Vec<ObjectAnalysis>,
    pub triggers: Vec<ObjectAnalysis>,
    pub sequences: Vec<ObjectAnalysis>,
}

impl AnalysisResult {
    pub fn has_differences(&self) -> bool {
        !self.schemas.is_empty()
            || !self.extensions.is_empty()
            || !self.types.is_empty()
            || !self.tables.is_empty()
            || !self.columns.is_empty()
            || !self.views.is_empty()
            || !self.materialized_views.is_empty()
            || !self.functions.is_empty()
            || !self.indexes.is_empty()
            || !self.constraints.is_empty()
            || !self.triggers.is_empty()
            || !self.sequences.is_empty()
    }

    pub fn count_differences(&self) -> usize {
        self.schemas.len()
            + self.extensions.len()
            + self.types.len()
            + self.tables.len()
            + self.columns.len()
            + self.views.len()
            + self.materialized_views.len()
            + self.functions.len()
            + self.indexes.len()
            + self.constraints.len()
            + self.triggers.len()
            + self.sequences.len()
    }
}

// =============================================================================
// Database Analysis
// =============================================================================

/// Every object type fetched from one database.
pub struct SchemaSnapshot {
    tables: Vec<TableInfo>,
    views: Vec<ViewInfo>,
    matviews: Vec<ViewInfo>,
    functions: Vec<FunctionInfo>,
    indexes: Vec<IndexInfo>,
    constraints: Vec<ConstraintInfo>,
    triggers: Vec<TriggerInfo>,
    sequences: Vec<SequenceInfo>,
    types: Vec<TypeInfo>,
    extensions: Vec<ExtensionInfo>,
}

impl SchemaSnapshot {
    /// Fetch all object types, each on its own pooled connection.
    pub async fn fetch(pool: &DbPool) -> anyhow::Result<Self> {
        let (
            tables,
            views,
            matviews,
            functions,
            indexes,
            constraints,
            triggers,
            sequences,
            types,
            extensions,
        ) = tokio::try_join!(
            async { fetch_tables(pool.get().await?.client()).await },
            async { fetch_views(pool.get().await?.client()).await },
            async { fetch_materialized_views(pool.get().await?.client()).await },
            async { fetch_functions(pool.get().await?.client()).await },
            async { fetch_indexes(pool.get().await?.client()).await },
            async { fetch_constraints(pool.get().await?.client()).await },
            async { fetch_triggers(pool.get().await?.client()).await },
            async { fetch_sequences(pool.get().await?.client()).await },
            async { fetch_types(pool.get().await?.client()).await },
            async { fetch_extensions(pool.get().await?.client()).await },
        )?;

        Ok(Self {
            tables,
            views,
            matviews,
            functions,
            indexes,
            constraints,
            triggers,
            sequences,
            types,
            extensions,
        })
    }
}

/// Fetch both databases concurrently and compare them.
pub async fn analyze_databases(
    left_pool: &DbPool,
    right_pool: &DbPool,
) -> anyhow::Result<AnalysisResult> {
    let (left, right) = tokio::try_join!(
        SchemaSnapshot::fetch(left_pool),
        SchemaSnapshot::fetch(right_pool)
    )?;
    Ok(compare_snapshots(&left, &right))
}

/// Compare two fetched snapshots: `left` is new (target), `right` is old.
pub fn compare_snapshots(left: &SchemaSnapshot, right: &SchemaSnapshot) -> AnalysisResult {
    let SchemaSnapshot {
        tables: left_tables,
        views: left_views,
        matviews: left_matviews,
        functions: left_functions,
        indexes: left_indexes,
        constraints: left_constraints,
        triggers: left_triggers,
        sequences: left_sequences,
        types: left_types,
        extensions: left_extensions,
    } = left;
    let SchemaSnapshot {
        tables: right_tables,
        views: right_views,
        matviews: right_matviews,
        functions: right_functions,
        indexes: right_indexes,
        constraints: right_constraints,
        triggers: right_triggers,
        sequences: right_sequences,
        types: right_types,
        extensions: right_extensions,
    } = right;

    // Extract schemas from tables
    let left_schemas: HashSet<&str> = left_tables.iter().map(|t| t.schema_name.as_str()).collect();
    let right_schemas: HashSet<&str> = right_tables
        .iter()
        .map(|t| t.schema_name.as_str())
        .collect();

    // Build summary
    let summary = vec![
        SummaryRow {
            object_type: "Schemas",
            left_count: left_schemas.len(),
            right_count: right_schemas.len(),
        },
        SummaryRow {
            object_type: "Extensions",
            left_count: left_extensions.len(),
            right_count: right_extensions.len(),
        },
        SummaryRow {
            object_type: "Types",
            left_count: left_types.len(),
            right_count: right_types.len(),
        },
        SummaryRow {
            object_type: "Tables",
            left_count: left_tables.len(),
            right_count: right_tables.len(),
        },
        SummaryRow {
            object_type: "Columns",
            left_count: left_tables.iter().map(|t| t.columns.len()).sum(),
            right_count: right_tables.iter().map(|t| t.columns.len()).sum(),
        },
        SummaryRow {
            object_type: "Views",
            left_count: left_views.len(),
            right_count: right_views.len(),
        },
        SummaryRow {
            object_type: "Materialized Views",
            left_count: left_matviews.len(),
            right_count: right_matviews.len(),
        },
        SummaryRow {
            object_type: "Functions",
            left_count: left_functions.len(),
            right_count: right_functions.len(),
        },
        SummaryRow {
            object_type: "Indexes",
            left_count: left_indexes.len(),
            right_count: right_indexes.len(),
        },
        SummaryRow {
            object_type: "Constraints",
            left_count: left_constraints.len(),
            right_count: right_constraints.len(),
        },
        SummaryRow {
            object_type: "Triggers",
            left_count: left_triggers.len(),
            right_count: right_triggers.len(),
        },
        SummaryRow {
            object_type: "Sequences",
            left_count: left_sequences.len(),
            right_count: right_sequences.len(),
        },
    ];

    // Analyze schemas
    let schemas = analyze_schemas(&left_schemas, &right_schemas);

    // Analyze extensions
    let extensions = analyze_extensions(left_extensions, right_extensions);

    // Analyze types
    let types = analyze_types(left_types, right_types);

    // Analyze tables
    let (tables, columns) = analyze_tables(left_tables, right_tables);

    // Analyze views
    let views = analyze_views(left_views, right_views, false);
    let materialized_views = analyze_views(left_matviews, right_matviews, true);

    // Analyze functions
    let functions = analyze_functions(left_functions, right_functions);

    // Analyze indexes
    let indexes = analyze_indexes(left_indexes, right_indexes);

    // Analyze constraints
    let constraints = analyze_constraints(left_constraints, right_constraints);

    // Analyze triggers
    let triggers = analyze_triggers(left_triggers, right_triggers);

    // Analyze sequences
    let sequences = analyze_sequences(left_sequences, right_sequences);

    AnalysisResult {
        summary,
        schemas,
        extensions,
        types,
        tables,
        columns,
        views,
        materialized_views,
        functions,
        indexes,
        constraints,
        triggers,
        sequences,
    }
}

fn analyze_extensions(
    left: &[crate::db::postgres::extensions::ExtensionInfo],
    right: &[crate::db::postgres::extensions::ExtensionInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::postgres::extensions::ExtensionInfo;

    let left_map: HashMap<&str, &ExtensionInfo> =
        left.iter().map(|e| (e.name.as_str(), e)).collect();

    let right_map: HashMap<&str, &ExtensionInfo> =
        right.iter().map(|e| (e.name.as_str(), e)).collect();

    let mut result = Vec::new();

    // Check for extensions to create (in left but not in right)
    for ext in left {
        if let Some(right_ext) = right_map.get(ext.name.as_str()) {
            let version_changed = ext.version != right_ext.version;
            let schema_changed = ext.schema_name != right_ext.schema_name;

            let mut mods = Vec::new();
            if version_changed {
                mods.push(format!("version: {} -> {}", right_ext.version, ext.version));
            }
            if schema_changed {
                mods.push(format!(
                    "schema: {} -> {}",
                    right_ext.schema_name, ext.schema_name
                ));
            }
            if !mods.is_empty() {
                let action = match (version_changed, schema_changed) {
                    (true, true) => format!(
                        "alter extension {} update to '{}' + set schema {}",
                        ext.name, ext.version, ext.schema_name
                    ),
                    (true, false) => {
                        format!("alter extension {} update to '{}'", ext.name, ext.version)
                    }
                    _ => format!(
                        "alter extension {} set schema {}",
                        ext.name, ext.schema_name
                    ),
                };
                result.push(ObjectAnalysis {
                    name: ext.name.clone(),
                    action_description: action,
                    modification_detail: Some(mods.join("\n")),
                });
            }
        } else {
            result.push(ObjectAnalysis {
                name: ext.name.clone(),
                action_description: format!("create extension {}", ext.name),
                modification_detail: Some(format!(
                    "schema: {}\nversion: {}",
                    ext.schema_name, ext.version
                )),
            });
        }
    }

    // Check for extensions to drop (in right but not in left)
    for ext in right {
        if !left_map.contains_key(ext.name.as_str()) {
            result.push(ObjectAnalysis {
                name: ext.name.clone(),
                action_description: format!("drop extension {}", ext.name),
                modification_detail: Some(format!(
                    "schema: {}\nversion: {}",
                    ext.schema_name, ext.version
                )),
            });
        }
    }

    result
}

fn analyze_schemas(left: &HashSet<&str>, right: &HashSet<&str>) -> Vec<ObjectAnalysis> {
    let mut result = Vec::new();

    // Added schemas (in left, not in right)
    for schema in left.difference(right) {
        result.push(ObjectAnalysis {
            name: schema.to_string(),
            action_description: format!("create schema {}", schema),
            modification_detail: None,
        });
    }

    // Removed schemas (in right, not in left)
    for schema in right.difference(left) {
        result.push(ObjectAnalysis {
            name: schema.to_string(),
            action_description: format!("drop schema {}", schema),
            modification_detail: None,
        });
    }

    result
}

fn analyze_types(
    left: &[crate::db::postgres::types::TypeInfo],
    right: &[crate::db::postgres::types::TypeInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::postgres::types::TypeInfo;

    let left_map: HashMap<(&str, &str), &TypeInfo> = left
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.type_name.as_str()), t))
        .collect();

    let right_map: HashMap<(&str, &str), &TypeInfo> = right
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.type_name.as_str()), t))
        .collect();

    let mut result = Vec::new();

    // Check for types to create (in left but not in right)
    for type_info in left {
        let key = (type_info.schema_name.as_str(), type_info.type_name.as_str());
        let full_name = format!("{}.{}", type_info.schema_name, type_info.type_name);

        if let Some(right_type) = right_map.get(&key) {
            // Type exists in both - check for differences
            let diffs = compare_type_details(type_info, right_type);
            if !diffs.is_empty() {
                result.push(ObjectAnalysis {
                    name: full_name.clone(),
                    action_description: format!(
                        "alter {} type {}.{}",
                        type_info.kind.label().to_lowercase(),
                        type_info.schema_name,
                        type_info.type_name
                    ),
                    modification_detail: Some(diffs.join("\n")),
                });
            }
        } else {
            // Type only in left - needs to be created
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!(
                    "create {} type {}.{}",
                    type_info.kind.label().to_lowercase(),
                    type_info.schema_name,
                    type_info.type_name
                ),
                modification_detail: None,
            });
        }
    }

    // Check for types to drop (in right but not in left)
    for type_info in right {
        let key = (type_info.schema_name.as_str(), type_info.type_name.as_str());
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}", type_info.schema_name, type_info.type_name),
                action_description: format!(
                    "drop {} type {}.{}",
                    type_info.kind.label().to_lowercase(),
                    type_info.schema_name,
                    type_info.type_name
                ),
                modification_detail: None,
            });
        }
    }

    result
}

fn compare_type_details(
    left: &crate::db::postgres::types::TypeInfo,
    right: &crate::db::postgres::types::TypeInfo,
) -> Vec<String> {
    use crate::db::postgres::types::TypeKind;

    let mut diffs = Vec::new();

    // Check if kind changed
    if left.kind != right.kind {
        diffs.push(format!(
            "type kind changed from {} to {} (requires DROP and CREATE)",
            right.kind.label(),
            left.kind.label()
        ));
        return diffs;
    }

    match left.kind {
        TypeKind::Enum => {
            // Check for added enum values
            for label in &left.enum_labels {
                if !right.enum_labels.contains(label) {
                    diffs.push(format!("add enum value '{}'", label));
                }
            }
            // Check for removed enum values
            for label in &right.enum_labels {
                if !left.enum_labels.contains(label) {
                    diffs.push(format!(
                        "remove enum value '{}' (requires recreating type)",
                        label
                    ));
                }
            }
        }
        TypeKind::Composite => {
            let left_attrs: HashMap<&str, &str> = left
                .composite_attrs
                .iter()
                .map(|(n, t)| (n.as_str(), t.as_str()))
                .collect();
            let right_attrs: HashMap<&str, &str> = right
                .composite_attrs
                .iter()
                .map(|(n, t)| (n.as_str(), t.as_str()))
                .collect();

            for (name, typ) in &left.composite_attrs {
                if !right_attrs.contains_key(name.as_str()) {
                    diffs.push(format!("add attribute {} {}", name, typ));
                } else if right_attrs.get(name.as_str()) != Some(&typ.as_str()) {
                    diffs.push(format!(
                        "change attribute {} from {} to {}",
                        name,
                        right_attrs.get(name.as_str()).unwrap_or(&"?"),
                        typ
                    ));
                }
            }
            for (name, _) in &right.composite_attrs {
                if !left_attrs.contains_key(name.as_str()) {
                    diffs.push(format!("drop attribute {}", name));
                }
            }
        }
        TypeKind::Domain => {
            if left.domain_base_type != right.domain_base_type {
                diffs.push(format!(
                    "change base type from {:?} to {:?}",
                    right.domain_base_type, left.domain_base_type
                ));
            }
            if left.domain_not_null != right.domain_not_null {
                if left.domain_not_null {
                    diffs.push("add NOT NULL".to_string());
                } else {
                    diffs.push("drop NOT NULL".to_string());
                }
            }
            if left.domain_default != right.domain_default {
                diffs.push(format!(
                    "change default from {:?} to {:?}",
                    right.domain_default, left.domain_default
                ));
            }
            if left.domain_constraint != right.domain_constraint {
                diffs.push(format!(
                    "change constraint from {:?} to {:?}",
                    right.domain_constraint, left.domain_constraint
                ));
            }
        }
        TypeKind::Range => {
            if left.range_subtype != right.range_subtype {
                diffs.push(format!(
                    "change subtype from {:?} to {:?}",
                    right.range_subtype, left.range_subtype
                ));
            }
            if left.range_canonical != right.range_canonical {
                diffs.push(format!(
                    "change canonical from {:?} to {:?}",
                    right.range_canonical, left.range_canonical
                ));
            }
        }
    }

    diffs
}

fn analyze_tables(
    left: &[crate::db::postgres::tables::TableInfo],
    right: &[crate::db::postgres::tables::TableInfo],
) -> (Vec<ObjectAnalysis>, Vec<ObjectAnalysis>) {
    use crate::db::postgres::tables::TableInfo;

    let left_map: HashMap<(&str, &str), &TableInfo> = left
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.table_name.as_str()), t))
        .collect();

    let right_map: HashMap<(&str, &str), &TableInfo> = right
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.table_name.as_str()), t))
        .collect();

    let mut tables = Vec::new();
    let mut columns = Vec::new();

    // Check all tables in left
    for table in left {
        let key = (table.schema_name.as_str(), table.table_name.as_str());
        let full_name = format!("{}.{}", table.schema_name, table.table_name);

        if let Some(right_table) = right_map.get(&key) {
            // Table exists in both - analyze columns
            let col_diffs = analyze_columns(table, right_table);
            columns.extend(col_diffs);
        } else {
            // Table only in left - ADD
            tables.push(ObjectAnalysis {
                name: full_name,
                action_description: format!(
                    "create table {}.{}",
                    table.schema_name, table.table_name
                ),
                modification_detail: None,
            });
        }
    }

    // Check for tables only in right - REMOVE
    for table in right {
        let key = (table.schema_name.as_str(), table.table_name.as_str());
        if !left_map.contains_key(&key) {
            tables.push(ObjectAnalysis {
                name: format!("{}.{}", table.schema_name, table.table_name),
                action_description: format!(
                    "drop table {}.{}",
                    table.schema_name, table.table_name
                ),
                modification_detail: None,
            });
        }
    }

    (tables, columns)
}

fn analyze_columns(
    left_table: &crate::db::postgres::tables::TableInfo,
    right_table: &crate::db::postgres::tables::TableInfo,
) -> Vec<ObjectAnalysis> {
    use crate::db::postgres::tables::ColumnInfo;

    let left_map: HashMap<&str, &ColumnInfo> = left_table
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    let right_map: HashMap<&str, &ColumnInfo> = right_table
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    let mut result = Vec::new();

    // Check columns in left
    for col in &left_table.columns {
        let full_name = format!(
            "{}.{}.{}",
            left_table.schema_name, left_table.table_name, col.name
        );

        if let Some(right_col) = right_map.get(col.name.as_str()) {
            // Column exists in both - check for modifications
            let mods = get_column_modifications(col, right_col);
            if !mods.is_empty() {
                let detail = format!(
                    "alter column {}\n  old: {}\n  new: {}",
                    full_name,
                    format_column_spec(right_col),
                    format_column_spec(col)
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!(
                        "alter column {}.{}.{}",
                        left_table.schema_name, left_table.table_name, col.name
                    ),
                    modification_detail: Some(detail),
                });
            }
        } else {
            // Column only in left - ADD
            result.push(ObjectAnalysis {
                name: full_name.clone(),
                action_description: format!(
                    "add column {}.{}.{}",
                    left_table.schema_name, left_table.table_name, col.name
                ),
                modification_detail: None,
            });
        }
    }

    // Check for columns only in right - REMOVE
    for col in &right_table.columns {
        if !left_map.contains_key(col.name.as_str()) {
            result.push(ObjectAnalysis {
                name: format!(
                    "{}.{}.{}",
                    right_table.schema_name, right_table.table_name, col.name
                ),
                action_description: format!(
                    "drop column {}.{}.{}",
                    right_table.schema_name, right_table.table_name, col.name
                ),
                modification_detail: None,
            });
        }
    }

    result
}

fn get_column_modifications(
    left: &crate::db::postgres::tables::ColumnInfo,
    right: &crate::db::postgres::tables::ColumnInfo,
) -> Vec<String> {
    let mut mods = Vec::new();

    if left.data_type != right.data_type {
        mods.push(format!("type: {} -> {}", right.data_type, left.data_type));
    }
    if left.is_nullable != right.is_nullable {
        let old_null = if right.is_nullable {
            "null"
        } else {
            "not null"
        };
        let new_null = if left.is_nullable { "null" } else { "not null" };
        mods.push(format!("nullable: {} -> {}", old_null, new_null));
    }
    if left.column_default != right.column_default {
        mods.push(format!(
            "default: {:?} -> {:?}",
            right.column_default, left.column_default
        ));
    }

    mods
}

fn format_column_spec(col: &crate::db::postgres::tables::ColumnInfo) -> String {
    let mut parts = vec![col.data_type.clone()];
    parts.push(if col.is_nullable { "null" } else { "not null" }.to_string());
    if let Some(ref default) = col.column_default {
        parts.push(format!("default {}", default));
    }
    parts.join(" ")
}

fn analyze_views(
    left: &[crate::db::postgres::views::ViewInfo],
    right: &[crate::db::postgres::views::ViewInfo],
    is_materialized: bool,
) -> Vec<ObjectAnalysis> {
    use crate::db::postgres::views::ViewInfo;

    let type_name = if is_materialized {
        "materialized view"
    } else {
        "view"
    };

    let left_map: HashMap<(&str, &str), &ViewInfo> = left
        .iter()
        .map(|v| ((v.schema_name.as_str(), v.view_name.as_str()), v))
        .collect();

    let right_map: HashMap<(&str, &str), &ViewInfo> = right
        .iter()
        .map(|v| ((v.schema_name.as_str(), v.view_name.as_str()), v))
        .collect();

    let mut result = Vec::new();

    // Check views in left
    for view in left {
        let key = (view.schema_name.as_str(), view.view_name.as_str());
        let full_name = format!("{}.{}", view.schema_name, view.view_name);

        if let Some(right_view) = right_map.get(&key) {
            // View exists in both - check for modifications
            if normalize_view_definition(&view.definition)
                != normalize_view_definition(&right_view.definition)
            {
                let detail = format!(
                    "replace {} {}\n{}",
                    type_name,
                    full_name,
                    definition_diff(&right_view.definition, &view.definition)
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!(
                        "replace {} {}.{}",
                        type_name, view.schema_name, view.view_name
                    ),
                    modification_detail: Some(detail),
                });
            }
        } else {
            // View only in left - ADD
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!(
                    "create {} {}.{}",
                    type_name, view.schema_name, view.view_name
                ),
                modification_detail: None,
            });
        }
    }

    // Check for views only in right - REMOVE
    for view in right {
        let key = (view.schema_name.as_str(), view.view_name.as_str());
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}", view.schema_name, view.view_name),
                action_description: format!(
                    "drop {} {}.{}",
                    type_name, view.schema_name, view.view_name
                ),
                modification_detail: None,
            });
        }
    }

    result
}

fn analyze_functions(
    left: &[crate::db::postgres::functions::FunctionInfo],
    right: &[crate::db::postgres::functions::FunctionInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::postgres::functions::FunctionInfo;

    let left_map: HashMap<(&str, &str, &str), &FunctionInfo> =
        left.iter().map(|f| (f.key(), f)).collect();

    let right_map: HashMap<(&str, &str, &str), &FunctionInfo> =
        right.iter().map(|f| (f.key(), f)).collect();

    let mut result = Vec::new();

    // Check functions in left
    for func in left {
        let full_name = func.signature();

        if let Some(right_func) = right_map.get(&func.key()) {
            // Function exists in both - check for modifications
            let left_def = normalize_function_def(&func.definition);
            let right_def = normalize_function_def(&right_func.definition);
            if left_def != right_def {
                let detail = format!(
                    "replace function {}\n{}",
                    full_name,
                    definition_diff(&right_func.definition, &func.definition)
                );
                result.push(ObjectAnalysis {
                    action_description: format!("replace function {}", full_name),
                    name: full_name,
                    modification_detail: Some(detail),
                });
            }
        } else {
            // Function only in left - ADD
            result.push(ObjectAnalysis {
                action_description: format!("create function {}", full_name),
                name: full_name,
                modification_detail: None,
            });
        }
    }

    // Check for functions only in right - REMOVE
    for func in right {
        if !left_map.contains_key(&func.key()) {
            let full_name = func.signature();
            result.push(ObjectAnalysis {
                action_description: format!("drop function {}", full_name),
                name: full_name,
                modification_detail: None,
            });
        }
    }

    result
}

fn analyze_indexes(
    left: &[crate::db::postgres::indexes::IndexInfo],
    right: &[crate::db::postgres::indexes::IndexInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::postgres::indexes::IndexInfo;

    let left_map: HashMap<(&str, &str), &IndexInfo> = left
        .iter()
        .map(|i| ((i.schema_name.as_str(), i.index_name.as_str()), i))
        .collect();

    let right_map: HashMap<(&str, &str), &IndexInfo> = right
        .iter()
        .map(|i| ((i.schema_name.as_str(), i.index_name.as_str()), i))
        .collect();

    let mut result = Vec::new();

    // Check indexes in left
    for idx in left {
        let key = (idx.schema_name.as_str(), idx.index_name.as_str());
        let full_name = format!("{}.{}", idx.schema_name, idx.index_name);

        if let Some(right_idx) = right_map.get(&key) {
            // Index exists in both - check for modifications
            if idx.definition != right_idx.definition {
                let detail = format!(
                    "recreate index {}\n  old: {}\n  new: {}",
                    full_name, right_idx.definition, idx.definition
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!(
                        "recreate index {}.{}",
                        idx.schema_name, idx.index_name
                    ),
                    modification_detail: Some(detail),
                });
            }
        } else {
            // Index only in left - ADD
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!("create index {}.{}", idx.schema_name, idx.index_name),
                modification_detail: None,
            });
        }
    }

    // Check for indexes only in right - REMOVE
    for idx in right {
        let key = (idx.schema_name.as_str(), idx.index_name.as_str());
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}", idx.schema_name, idx.index_name),
                action_description: format!("drop index {}.{}", idx.schema_name, idx.index_name),
                modification_detail: None,
            });
        }
    }

    result
}

fn analyze_constraints(
    left: &[crate::db::postgres::constraints::ConstraintInfo],
    right: &[crate::db::postgres::constraints::ConstraintInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::postgres::constraints::ConstraintInfo;

    let left_map: HashMap<(&str, &str, &str), &ConstraintInfo> = left
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

    let right_map: HashMap<(&str, &str, &str), &ConstraintInfo> = right
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

    let mut result = Vec::new();

    // Check constraints in left
    for con in left {
        let key = (
            con.schema_name.as_str(),
            con.table_name.as_str(),
            con.constraint_name.as_str(),
        );
        let full_name = format!(
            "{}.{}.{}",
            con.schema_name, con.table_name, con.constraint_name
        );

        if let Some(right_con) = right_map.get(&key) {
            // Constraint exists in both - check for modifications using normalized comparison
            let left_def = normalize_constraint_definition(&con.definition);
            let right_def = normalize_constraint_definition(&right_con.definition);
            if left_def != right_def {
                let detail = format!(
                    "replace constraint {}\n  old: {}\n  new: {}",
                    full_name, right_con.definition, con.definition
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!(
                        "replace constraint {}.{}.{}",
                        con.schema_name, con.table_name, con.constraint_name
                    ),
                    modification_detail: Some(detail),
                });
            }
        } else {
            // Constraint only in left - ADD
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!(
                    "add constraint {}.{}.{}",
                    con.schema_name, con.table_name, con.constraint_name
                ),
                modification_detail: None,
            });
        }
    }

    // Check for constraints only in right - REMOVE
    for con in right {
        let key = (
            con.schema_name.as_str(),
            con.table_name.as_str(),
            con.constraint_name.as_str(),
        );
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!(
                    "{}.{}.{}",
                    con.schema_name, con.table_name, con.constraint_name
                ),
                action_description: format!(
                    "drop constraint {}.{}.{}",
                    con.schema_name, con.table_name, con.constraint_name
                ),
                modification_detail: None,
            });
        }
    }

    result
}

fn analyze_triggers(
    left: &[crate::db::postgres::triggers::TriggerInfo],
    right: &[crate::db::postgres::triggers::TriggerInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::postgres::triggers::TriggerInfo;

    let left_map: HashMap<(&str, &str, &str), &TriggerInfo> =
        left.iter().map(|t| (t.key(), t)).collect();

    let right_map: HashMap<(&str, &str, &str), &TriggerInfo> =
        right.iter().map(|t| (t.key(), t)).collect();

    let mut result = Vec::new();

    // Check triggers in left
    for trig in left {
        let full_name = format!(
            "{}.{}.{}",
            trig.schema_name, trig.table_name, trig.trigger_name
        );

        if let Some(right_trig) = right_map.get(&trig.key()) {
            // Trigger exists in both - check for modifications
            let left_def = normalize_trigger_def(&trig.definition);
            let right_def = normalize_trigger_def(&right_trig.definition);
            if left_def != right_def {
                let detail = format!(
                    "replace trigger {}\n  old: {}\n  new: {}",
                    full_name, right_trig.definition, trig.definition
                );
                result.push(ObjectAnalysis {
                    action_description: format!("replace trigger {}", full_name),
                    name: full_name,
                    modification_detail: Some(detail),
                });
            }
        } else {
            // Trigger only in left - ADD
            result.push(ObjectAnalysis {
                action_description: format!("create trigger {}", full_name),
                name: full_name,
                modification_detail: None,
            });
        }
    }

    // Check for triggers only in right - REMOVE
    for trig in right {
        if !left_map.contains_key(&trig.key()) {
            let full_name = format!(
                "{}.{}.{}",
                trig.schema_name, trig.table_name, trig.trigger_name
            );
            result.push(ObjectAnalysis {
                action_description: format!("drop trigger {}", full_name),
                name: full_name,
                modification_detail: None,
            });
        }
    }

    result
}

fn analyze_sequences(
    left: &[crate::db::postgres::sequences::SequenceInfo],
    right: &[crate::db::postgres::sequences::SequenceInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::postgres::sequences::SequenceInfo;

    let left_map: HashMap<(&str, &str), &SequenceInfo> = left
        .iter()
        .map(|s| ((s.schema_name.as_str(), s.sequence_name.as_str()), s))
        .collect();

    let right_map: HashMap<(&str, &str), &SequenceInfo> = right
        .iter()
        .map(|s| ((s.schema_name.as_str(), s.sequence_name.as_str()), s))
        .collect();

    let mut result = Vec::new();

    // Check sequences in left
    for seq in left {
        let key = (seq.schema_name.as_str(), seq.sequence_name.as_str());
        let full_name = format!("{}.{}", seq.schema_name, seq.sequence_name);

        if let Some(right_seq) = right_map.get(&key) {
            // Sequence exists in both - check for modifications
            let mods = get_sequence_modifications(seq, right_seq);
            if !mods.is_empty() {
                let detail = format!("alter sequence {}\n  {}", full_name, mods.join("\n  "));
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!(
                        "alter sequence {}.{}",
                        seq.schema_name, seq.sequence_name
                    ),
                    modification_detail: Some(detail),
                });
            }
        } else {
            // Sequence only in left - ADD
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!(
                    "create sequence {}.{}",
                    seq.schema_name, seq.sequence_name
                ),
                modification_detail: None,
            });
        }
    }

    // Check for sequences only in right - REMOVE
    for seq in right {
        let key = (seq.schema_name.as_str(), seq.sequence_name.as_str());
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}", seq.schema_name, seq.sequence_name),
                action_description: format!(
                    "drop sequence {}.{}",
                    seq.schema_name, seq.sequence_name
                ),
                modification_detail: None,
            });
        }
    }

    result
}

fn get_sequence_modifications(
    left: &crate::db::postgres::sequences::SequenceInfo,
    right: &crate::db::postgres::sequences::SequenceInfo,
) -> Vec<String> {
    let mut mods = Vec::new();

    if left.data_type != right.data_type {
        mods.push(format!("type: {} -> {}", right.data_type, left.data_type));
    }
    if left.start_value != right.start_value {
        mods.push(format!(
            "start: {} -> {}",
            right.start_value, left.start_value
        ));
    }
    if left.increment_by != right.increment_by {
        mods.push(format!(
            "increment: {} -> {}",
            right.increment_by, left.increment_by
        ));
    }
    if left.min_value != right.min_value {
        mods.push(format!("min: {} -> {}", right.min_value, left.min_value));
    }
    if left.max_value != right.max_value {
        mods.push(format!("max: {} -> {}", right.max_value, left.max_value));
    }
    if left.cycle != right.cycle {
        mods.push(format!("cycle: {} -> {}", right.cycle, left.cycle));
    }

    mods
}

/// Unified diff from the old definition to the new one, for change details.
fn definition_diff(old: &str, new: &str) -> String {
    let old = format!("{}\n", old.trim_end());
    let new = format!("{}\n", new.trim_end());
    similar::TextDiff::from_lines(&old, &new)
        .unified_diff()
        .context_radius(3)
        .header("old", "new")
        .to_string()
        .trim_end()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::postgres::functions::FunctionInfo;
    use crate::db::postgres::triggers::TriggerInfo;
    use crate::db::postgres::views::ViewInfo;

    fn trigger(table: &str, function: &str) -> TriggerInfo {
        TriggerInfo {
            schema_name: "app".to_string(),
            table_name: table.to_string(),
            trigger_name: "~sync".to_string(),
            definition: format!(
                "CREATE TRIGGER \"~sync\" BEFORE UPDATE ON app.{} FOR EACH ROW EXECUTE FUNCTION app.\"{}\"()",
                table, function
            ),
        }
    }

    #[test]
    fn same_named_triggers_on_different_tables_match_their_own_table() {
        let left = vec![trigger("a", "a@sync"), trigger("b", "b@sync")];
        let right = vec![trigger("a", "a@sync"), trigger("b", "b@sync")];
        assert!(analyze_triggers(&left, &right).is_empty());

        let changed = vec![trigger("a", "a@sync"), trigger("b", "b@other")];
        let result = analyze_triggers(&changed, &right);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "app.b.~sync");
        let detail = result[0].modification_detail.as_deref().unwrap_or("");
        assert!(detail.contains("app.\"b@sync\"()"), "{}", detail);
        assert!(detail.contains("app.\"b@other\"()"), "{}", detail);
    }

    fn function(args: &str, body: &str) -> FunctionInfo {
        FunctionInfo {
            schema_name: "app".to_string(),
            function_name: "f".to_string(),
            identity_args: args.to_string(),
            definition: format!("CREATE FUNCTION app.f({}) AS $$ {} $$", args, body),
        }
    }

    #[test]
    fn overloaded_functions_match_by_signature() {
        let left = vec![function("integer", "one"), function("text", "two")];
        let right = vec![function("integer", "one"), function("text", "two")];
        assert!(analyze_functions(&left, &right).is_empty());

        let changed = vec![function("integer", "one"), function("text", "three")];
        let result = analyze_functions(&changed, &right);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "app.f(text)");
    }

    #[test]
    fn view_change_detail_shows_the_full_differing_line() {
        let select = "SELECT file_struct_file_guid, create_ts, key_prefix, key_suffix, content_type, size_bytes";
        let view = |filter: &str| ViewInfo {
            schema_name: "app".to_string(),
            view_name: "v".to_string(),
            definition: format!(" {}\n   FROM file_struct.file\n  WHERE {};", select, filter),
        };
        let left = vec![view("key_prefix::text = 'new'::text")];
        let right = vec![view("key_prefix::text = 'old'::text")];

        let result = analyze_views(&left, &right, false);
        assert_eq!(result.len(), 1);
        let detail = result[0].modification_detail.as_deref().unwrap_or("");
        assert!(
            detail.starts_with("replace view app.v\n--- old\n+++ new\n"),
            "{}",
            detail
        );
        assert!(
            detail.contains("\n-  WHERE key_prefix::text = 'old'::text;"),
            "{}",
            detail
        );
        assert!(
            detail.contains("\n+  WHERE key_prefix::text = 'new'::text;"),
            "{}",
            detail
        );
    }
}
