use crate::App;
use crate::cli::DiffArgs;
use crate::config::Config;
use crate::db::postgres::DbConnection;
use crate::db::postgres::constraints::fetch_constraints;
use crate::db::postgres::functions::fetch_functions;
use crate::db::postgres::indexes::fetch_indexes;
use crate::db::postgres::sequences::fetch_sequences;
use crate::db::postgres::tables::fetch_tables;
use crate::db::postgres::triggers::fetch_triggers;
use crate::db::postgres::types::fetch_types;
use crate::db::postgres::views::{fetch_materialized_views, fetch_views};
use anyhow::Context;
use std::collections::{HashMap, HashSet};

pub async fn run_diff(app: &'static App, _args: DiffArgs) -> anyhow::Result<()> {
    let config = Config::load(&app.path).context("Failed to load CONFIG.toml")?;

    let new_config = config
        .new
        .as_ref()
        .context("No 'new' database configured in CONFIG.toml")?;

    let old_config = config
        .old
        .as_ref()
        .context("No 'old' database configured in CONFIG.toml")?;

    // Connect to both databases
    // Left = new (source/what you want), Right = old (target/what you have)
    eprintln!("Connecting to left (new) database...");
    let left_conn = DbConnection::connect(&new_config.connection_string())
        .await
        .context("Failed to connect to new database")?;

    eprintln!("Connecting to right (old) database...");
    let right_conn = DbConnection::connect(&old_config.connection_string())
        .await
        .context("Failed to connect to old database")?;

    // Get version info
    let left_version = get_postgres_version(&left_conn).await?;
    let right_version = get_postgres_version(&right_conn).await?;

    // Fetch schema objects from both databases
    eprintln!("Fetching schemas...");
    let analysis = analyze_databases(&left_conn, &right_conn).await?;

    // Generate XML output
    let xml = generate_xml(
        &analysis,
        &new_config.connection_string(),
        &old_config.connection_string(),
        &left_version,
        &right_version,
    );

    println!("{}", xml);

    // Exit with code 2 if there are differences
    if analysis.has_differences() {
        std::process::exit(2);
    }

    Ok(())
}

async fn get_postgres_version(conn: &DbConnection) -> anyhow::Result<String> {
    let row = conn
        .client()
        .query_one("SHOW server_version", &[])
        .await?;
    let version: String = row.get(0);
    // Extract major version (e.g., "15.4" -> "15")
    let major = version.split('.').next().unwrap_or(&version);
    Ok(major.to_string())
}

// =============================================================================
// Analysis Types
// =============================================================================

/// Summary row for an object type
struct SummaryRow {
    object_type: &'static str,
    left_count: usize,
    right_count: usize,
}

impl SummaryRow {
    fn is_different(&self) -> bool {
        self.left_count != self.right_count
    }
}

/// Analysis result for a single object
struct ObjectAnalysis {
    name: String,           // Full qualified name
    action_description: String,
    modification_detail: Option<String>,
}

/// Complete analysis result
struct AnalysisResult {
    summary: Vec<SummaryRow>,
    schemas: Vec<ObjectAnalysis>,
    types: Vec<ObjectAnalysis>,
    tables: Vec<ObjectAnalysis>,
    columns: Vec<ObjectAnalysis>,
    views: Vec<ObjectAnalysis>,
    materialized_views: Vec<ObjectAnalysis>,
    functions: Vec<ObjectAnalysis>,
    indexes: Vec<ObjectAnalysis>,
    constraints: Vec<ObjectAnalysis>,
    triggers: Vec<ObjectAnalysis>,
    sequences: Vec<ObjectAnalysis>,
}

impl AnalysisResult {
    fn has_differences(&self) -> bool {
        !self.schemas.is_empty()
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

    fn count_differences(&self) -> usize {
        self.schemas.len()
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

async fn analyze_databases(
    left_conn: &DbConnection,
    right_conn: &DbConnection,
) -> anyhow::Result<AnalysisResult> {
    let left_client = left_conn.client();
    let right_client = right_conn.client();

    // Fetch all object types from both databases in parallel
    let (left_tables, right_tables) =
        tokio::try_join!(fetch_tables(left_client), fetch_tables(right_client))?;

    let (left_views, right_views) =
        tokio::try_join!(fetch_views(left_client), fetch_views(right_client))?;

    let (left_matviews, right_matviews) = tokio::try_join!(
        fetch_materialized_views(left_client),
        fetch_materialized_views(right_client)
    )?;

    let (left_functions, right_functions) =
        tokio::try_join!(fetch_functions(left_client), fetch_functions(right_client))?;

    let (left_indexes, right_indexes) =
        tokio::try_join!(fetch_indexes(left_client), fetch_indexes(right_client))?;

    let (left_constraints, right_constraints) =
        tokio::try_join!(fetch_constraints(left_client), fetch_constraints(right_client))?;

    let (left_triggers, right_triggers) =
        tokio::try_join!(fetch_triggers(left_client), fetch_triggers(right_client))?;

    let (left_sequences, right_sequences) =
        tokio::try_join!(fetch_sequences(left_client), fetch_sequences(right_client))?;

    let (left_types, right_types) =
        tokio::try_join!(fetch_types(left_client), fetch_types(right_client))?;

    // Extract schemas from tables
    let left_schemas: HashSet<&str> = left_tables.iter().map(|t| t.schema_name.as_str()).collect();
    let right_schemas: HashSet<&str> = right_tables.iter().map(|t| t.schema_name.as_str()).collect();

    // Build summary
    let summary = vec![
        SummaryRow {
            object_type: "Schemas",
            left_count: left_schemas.len(),
            right_count: right_schemas.len(),
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

    // Analyze types
    let types = analyze_types(&left_types, &right_types);

    // Analyze tables
    let (tables, columns) = analyze_tables(&left_tables, &right_tables);

    // Analyze views
    let views = analyze_views(&left_views, &right_views, false);
    let materialized_views = analyze_views(&left_matviews, &right_matviews, true);

    // Analyze functions
    let functions = analyze_functions(&left_functions, &right_functions);

    // Analyze indexes
    let indexes = analyze_indexes(&left_indexes, &right_indexes);

    // Analyze constraints
    let constraints = analyze_constraints(&left_constraints, &right_constraints);

    // Analyze triggers
    let triggers = analyze_triggers(&left_triggers, &right_triggers);

    // Analyze sequences
    let sequences = analyze_sequences(&left_sequences, &right_sequences);

    Ok(AnalysisResult {
        summary,
        schemas,
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
    })
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
                    diffs.push(format!("remove enum value '{}' (requires recreating type)", label));
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
                action_description: format!("create table {}.{}", table.schema_name, table.table_name),
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
                action_description: format!("drop table {}.{}", table.schema_name, table.table_name),
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
        let full_name = format!("{}.{}.{}", left_table.schema_name, left_table.table_name, col.name);

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
                name: format!("{}.{}.{}", right_table.schema_name, right_table.table_name, col.name),
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
        let old_null = if right.is_nullable { "null" } else { "not null" };
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
            if view.definition.trim() != right_view.definition.trim() {
                let detail = format!(
                    "replace {} {}\n  old: {}\n  new: {}",
                    type_name,
                    full_name,
                    truncate(&right_view.definition, 100),
                    truncate(&view.definition, 100)
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!("replace {} {}.{}", type_name, view.schema_name, view.view_name),
                    modification_detail: Some(detail),
                });
            }
        } else {
            // View only in left - ADD
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!("create {} {}.{}", type_name, view.schema_name, view.view_name),
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
                action_description: format!("drop {} {}.{}", type_name, view.schema_name, view.view_name),
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

    let left_map: HashMap<(&str, &str), &FunctionInfo> = left
        .iter()
        .map(|f| ((f.schema_name.as_str(), f.function_name.as_str()), f))
        .collect();

    let right_map: HashMap<(&str, &str), &FunctionInfo> = right
        .iter()
        .map(|f| ((f.schema_name.as_str(), f.function_name.as_str()), f))
        .collect();

    let mut result = Vec::new();

    // Check functions in left
    for func in left {
        let key = (func.schema_name.as_str(), func.function_name.as_str());
        let full_name = format!("{}.{}", func.schema_name, func.function_name);

        if let Some(right_func) = right_map.get(&key) {
            // Function exists in both - check for modifications
            let left_def = normalize_function_def(&func.definition);
            let right_def = normalize_function_def(&right_func.definition);
            if left_def != right_def {
                let detail = format!(
                    "replace function {}\n  definition changed",
                    full_name
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!("replace function {}.{}", func.schema_name, func.function_name),
                    modification_detail: Some(detail),
                });
            }
        } else {
            // Function only in left - ADD
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!("create function {}.{}", func.schema_name, func.function_name),
                modification_detail: None,
            });
        }
    }

    // Check for functions only in right - REMOVE
    for func in right {
        let key = (func.schema_name.as_str(), func.function_name.as_str());
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}", func.schema_name, func.function_name),
                action_description: format!("drop function {}.{}", func.schema_name, func.function_name),
                modification_detail: None,
            });
        }
    }

    result
}

fn normalize_function_def(def: &str) -> String {
    def.split_whitespace().collect::<Vec<_>>().join(" ")
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
                    full_name,
                    right_idx.definition,
                    idx.definition
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!("recreate index {}.{}", idx.schema_name, idx.index_name),
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
        let full_name = format!("{}.{}.{}", con.schema_name, con.table_name, con.constraint_name);

        if let Some(right_con) = right_map.get(&key) {
            // Constraint exists in both - check for modifications using normalized comparison
            let left_def = normalize_constraint_definition(&con.definition);
            let right_def = normalize_constraint_definition(&right_con.definition);
            if left_def != right_def {
                let detail = format!(
                    "replace constraint {}\n  old: {}\n  new: {}",
                    full_name,
                    right_con.definition,
                    con.definition
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
                name: format!("{}.{}.{}", con.schema_name, con.table_name, con.constraint_name),
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

/// Normalize constraint definition to reduce false positives from formatting differences.
/// Removes type casts like ::character varying, ::text, etc. and normalizes whitespace.
fn normalize_constraint_definition(def: &str) -> String {
    let mut result = def.to_string();

    // Remove type casts
    result = result.replace("::character varying[]", "");
    result = result.replace("::character varying", "");
    result = result.replace("::text[]", "");
    result = result.replace("::text", "");

    // Convert any non-alphanumeric characters to a single space
    let mut normalized = String::with_capacity(result.len());
    let mut last_was_space = true;
    for c in result.chars() {
        if c.is_alphanumeric() {
            normalized.push(c);
            last_was_space = false;
        } else if !last_was_space {
            normalized.push(' ');
            last_was_space = true;
        }
    }

    normalized.trim().to_string()
}

fn analyze_triggers(
    left: &[crate::db::postgres::triggers::TriggerInfo],
    right: &[crate::db::postgres::triggers::TriggerInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::postgres::triggers::TriggerInfo;

    let left_map: HashMap<(&str, &str), &TriggerInfo> = left
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.trigger_name.as_str()), t))
        .collect();

    let right_map: HashMap<(&str, &str), &TriggerInfo> = right
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.trigger_name.as_str()), t))
        .collect();

    let mut result = Vec::new();

    // Check triggers in left
    for trig in left {
        let key = (trig.schema_name.as_str(), trig.trigger_name.as_str());
        let full_name = format!("{}.{}.{}", trig.schema_name, trig.table_name, trig.trigger_name);

        if let Some(right_trig) = right_map.get(&key) {
            // Trigger exists in both - check for modifications
            let left_def = normalize_trigger_def(&trig.definition);
            let right_def = normalize_trigger_def(&right_trig.definition);
            if left_def != right_def {
                let detail = format!(
                    "replace trigger {}\n  definition changed",
                    full_name
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!(
                        "replace trigger {}.{}.{}",
                        trig.schema_name, trig.table_name, trig.trigger_name
                    ),
                    modification_detail: Some(detail),
                });
            }
        } else {
            // Trigger only in left - ADD
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!(
                    "create trigger {}.{}.{}",
                    trig.schema_name, trig.table_name, trig.trigger_name
                ),
                modification_detail: None,
            });
        }
    }

    // Check for triggers only in right - REMOVE
    for trig in right {
        let key = (trig.schema_name.as_str(), trig.trigger_name.as_str());
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}.{}", trig.schema_name, trig.table_name, trig.trigger_name),
                action_description: format!(
                    "drop trigger {}.{}.{}",
                    trig.schema_name, trig.table_name, trig.trigger_name
                ),
                modification_detail: None,
            });
        }
    }

    result
}

fn normalize_trigger_def(def: &str) -> String {
    def.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
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
                let detail = format!(
                    "alter sequence {}\n  {}",
                    full_name,
                    mods.join("\n  ")
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!("alter sequence {}.{}", seq.schema_name, seq.sequence_name),
                    modification_detail: Some(detail),
                });
            }
        } else {
            // Sequence only in left - ADD
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!("create sequence {}.{}", seq.schema_name, seq.sequence_name),
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
                action_description: format!("drop sequence {}.{}", seq.schema_name, seq.sequence_name),
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
        mods.push(format!("start: {} -> {}", right.start_value, left.start_value));
    }
    if left.increment_by != right.increment_by {
        mods.push(format!("increment: {} -> {}", right.increment_by, left.increment_by));
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

fn truncate(s: &str, max_len: usize) -> String {
    let s = s.replace('\n', " ").replace("  ", " ");
    if s.len() <= max_len {
        s
    } else {
        format!("{}...", &s[..max_len])
    }
}

// =============================================================================
// XML Generation
// =============================================================================

fn generate_xml(
    analysis: &AnalysisResult,
    left_conn_str: &str,
    right_conn_str: &str,
    left_version: &str,
    right_version: &str,
) -> String {
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<database_comparison>\n");

    // Connections section
    xml.push_str("  <connections>\n");
    xml.push_str(&format!(
        "    <left postgres_version=\"{}\">{}</left>\n",
        xml_escape(left_version),
        xml_escape(left_conn_str)
    ));
    xml.push_str(&format!(
        "    <right postgres_version=\"{}\">{}</right>\n",
        xml_escape(right_version),
        xml_escape(right_conn_str)
    ));
    xml.push_str("  </connections>\n");

    // Version warning if versions differ
    if left_version != right_version {
        xml.push_str(&format!(
            "  <version_warning>PostgreSQL versions differ (left: {}, right: {}). Some differences may be due to version-specific output formatting.</version_warning>\n",
            xml_escape(left_version),
            xml_escape(right_version)
        ));
    }

    // Summary section
    xml.push_str("  <summary>\n");
    for row in &analysis.summary {
        xml.push_str("    <item>\n");
        xml.push_str(&format!("      <type>{}</type>\n", row.object_type));
        xml.push_str(&format!("      <left_count>{}</left_count>\n", row.left_count));
        xml.push_str(&format!("      <right_count>{}</right_count>\n", row.right_count));
        xml.push_str(&format!(
            "      <different>{}</different>\n",
            row.is_different().to_string().to_lowercase()
        ));
        xml.push_str("    </item>\n");
    }
    xml.push_str("  </summary>\n");

    // Schemas section
    write_object_section(&mut xml, "schemas", "schema", &analysis.schemas);

    // Types section
    write_object_section(&mut xml, "types", "type", &analysis.types);

    // Tables section
    write_object_section(&mut xml, "tables", "table", &analysis.tables);

    // Columns section
    write_object_section(&mut xml, "columns", "column", &analysis.columns);

    // Views section
    write_object_section(&mut xml, "views", "view", &analysis.views);

    // Materialized views section
    write_object_section(&mut xml, "materialized_views", "materialized_view", &analysis.materialized_views);

    // Functions section
    write_object_section(&mut xml, "functions", "function", &analysis.functions);

    // Indexes section
    write_object_section(&mut xml, "indexes", "index", &analysis.indexes);

    // Constraints section
    write_object_section(&mut xml, "constraints", "constraint", &analysis.constraints);

    // Triggers section
    write_object_section(&mut xml, "triggers", "trigger", &analysis.triggers);

    // Sequences section
    write_object_section(&mut xml, "sequences", "sequence", &analysis.sequences);

    // Total differences count
    xml.push_str(&format!(
        "  <number_of_differences>{}</number_of_differences>\n",
        analysis.count_differences()
    ));

    xml.push_str("</database_comparison>\n");

    xml
}

fn write_object_section(xml: &mut String, section_name: &str, item_name: &str, objects: &[ObjectAnalysis]) {
    xml.push_str(&format!("  <{}>\n", section_name));
    for obj in objects {
        xml.push_str(&format!("    <{}>\n", item_name));
        xml.push_str(&format!("      <name>{}</name>\n", xml_escape(&obj.name)));
        xml.push_str(&format!("      <action>{}</action>\n", xml_escape(&obj.action_description)));
        if let Some(ref detail) = obj.modification_detail {
            xml.push_str(&format!("      <detail>{}</detail>\n", xml_escape(detail)));
        }
        xml.push_str(&format!("    </{}>\n", item_name));
    }
    xml.push_str(&format!("  </{}>\n", section_name));
}

/// Escape special XML characters
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
