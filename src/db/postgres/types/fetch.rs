use tokio_postgres::Client;

/// The kind of PostgreSQL type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Enum,
    Composite,
    Domain,
    Range,
}

impl TypeKind {
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'e' => Some(TypeKind::Enum),
            'c' => Some(TypeKind::Composite),
            'd' => Some(TypeKind::Domain),
            'r' => Some(TypeKind::Range),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            TypeKind::Enum => "ENUM",
            TypeKind::Composite => "COMPOSITE",
            TypeKind::Domain => "DOMAIN",
            TypeKind::Range => "RANGE",
        }
    }
}

/// Metadata for a PostgreSQL type (enum, composite, domain, or range)
#[derive(Debug)]
pub struct TypeInfo {
    pub schema_name: String,
    pub type_name: String,
    pub kind: TypeKind,
    /// For enums: the list of enum labels in order
    pub enum_labels: Vec<String>,
    /// For composites: list of (attr_name, attr_type) pairs
    pub composite_attrs: Vec<(String, String)>,
    /// For domains: the base type
    pub domain_base_type: Option<String>,
    /// For domains: the constraint expression (if any)
    pub domain_constraint: Option<String>,
    /// For domains: default value (if any)
    pub domain_default: Option<String>,
    /// For domains: whether NOT NULL is set
    pub domain_not_null: bool,
    /// For ranges: the subtype
    pub range_subtype: Option<String>,
    /// For ranges: the subtype operator class
    pub range_subtype_opclass: Option<String>,
    /// For ranges: the canonical function (if any)
    pub range_canonical: Option<String>,
    /// For ranges: the subtype diff function (if any)
    pub range_subtype_diff: Option<String>,
}

/// Fetch all user-defined types from the database
pub async fn fetch_types(client: &Client) -> anyhow::Result<Vec<TypeInfo>> {
    let mut types = Vec::new();

    // Fetch enum types
    let enum_rows = client
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                t.typname AS type_name,
                array_agg(e.enumlabel ORDER BY e.enumsortorder) AS enum_labels
            FROM pg_type t
            JOIN pg_namespace n ON n.oid = t.typnamespace
            JOIN pg_enum e ON e.enumtypid = t.oid
            WHERE t.typtype = 'e'
              AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
            GROUP BY n.nspname, t.typname
            ORDER BY n.nspname, t.typname
            "#,
            &[],
        )
        .await?;

    for row in enum_rows {
        let labels: Vec<String> = row.get("enum_labels");
        types.push(TypeInfo {
            schema_name: row.get("schema_name"),
            type_name: row.get("type_name"),
            kind: TypeKind::Enum,
            enum_labels: labels,
            composite_attrs: Vec::new(),
            domain_base_type: None,
            domain_constraint: None,
            domain_default: None,
            domain_not_null: false,
            range_subtype: None,
            range_subtype_opclass: None,
            range_canonical: None,
            range_subtype_diff: None,
        });
    }

    // Fetch composite types (excluding table row types which have relkind)
    let composite_rows = client
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                t.typname AS type_name,
                array_agg(a.attname ORDER BY a.attnum) AS attr_names,
                array_agg(pg_catalog.format_type(a.atttypid, a.atttypmod) ORDER BY a.attnum) AS attr_types
            FROM pg_type t
            JOIN pg_namespace n ON n.oid = t.typnamespace
            JOIN pg_class c ON c.oid = t.typrelid
            LEFT JOIN pg_attribute a ON a.attrelid = t.typrelid AND a.attnum > 0 AND NOT a.attisdropped
            WHERE t.typtype = 'c'
              AND c.relkind = 'c'  -- composite type, not table
              AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
            GROUP BY n.nspname, t.typname
            ORDER BY n.nspname, t.typname
            "#,
            &[],
        )
        .await?;

    for row in composite_rows {
        let attr_names: Vec<String> = row.get("attr_names");
        let attr_types: Vec<String> = row.get("attr_types");
        let attrs: Vec<(String, String)> = attr_names.into_iter().zip(attr_types).collect();

        types.push(TypeInfo {
            schema_name: row.get("schema_name"),
            type_name: row.get("type_name"),
            kind: TypeKind::Composite,
            enum_labels: Vec::new(),
            composite_attrs: attrs,
            domain_base_type: None,
            domain_constraint: None,
            domain_default: None,
            domain_not_null: false,
            range_subtype: None,
            range_subtype_opclass: None,
            range_canonical: None,
            range_subtype_diff: None,
        });
    }

    // Fetch domain types
    let domain_rows = client
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                t.typname AS type_name,
                pg_catalog.format_type(t.typbasetype, t.typtypmod) AS base_type,
                t.typnotnull AS not_null,
                t.typdefault AS default_value,
                (
                    SELECT pg_get_constraintdef(con.oid)
                    FROM pg_constraint con
                    WHERE con.contypid = t.oid
                    LIMIT 1
                ) AS constraint_def
            FROM pg_type t
            JOIN pg_namespace n ON n.oid = t.typnamespace
            WHERE t.typtype = 'd'
              AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
            ORDER BY n.nspname, t.typname
            "#,
            &[],
        )
        .await?;

    for row in domain_rows {
        types.push(TypeInfo {
            schema_name: row.get("schema_name"),
            type_name: row.get("type_name"),
            kind: TypeKind::Domain,
            enum_labels: Vec::new(),
            composite_attrs: Vec::new(),
            domain_base_type: Some(row.get("base_type")),
            domain_constraint: row.get("constraint_def"),
            domain_default: row.get("default_value"),
            domain_not_null: row.get("not_null"),
            range_subtype: None,
            range_subtype_opclass: None,
            range_canonical: None,
            range_subtype_diff: None,
        });
    }

    // Fetch range types
    let range_rows = client
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                t.typname AS type_name,
                pg_catalog.format_type(r.rngsubtype, NULL) AS subtype,
                opc.opcname AS subtype_opclass,
                rngcanonical::regproc::text AS canonical_func,
                rngsubdiff::regproc::text AS subtype_diff_func
            FROM pg_type t
            JOIN pg_namespace n ON n.oid = t.typnamespace
            JOIN pg_range r ON r.rngtypid = t.oid
            LEFT JOIN pg_opclass opc ON opc.oid = r.rngsubopc
            WHERE t.typtype = 'r'
              AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
            ORDER BY n.nspname, t.typname
            "#,
            &[],
        )
        .await?;

    for row in range_rows {
        let canonical: Option<String> = row.get("canonical_func");
        let subtype_diff: Option<String> = row.get("subtype_diff_func");

        types.push(TypeInfo {
            schema_name: row.get("schema_name"),
            type_name: row.get("type_name"),
            kind: TypeKind::Range,
            enum_labels: Vec::new(),
            composite_attrs: Vec::new(),
            domain_base_type: None,
            domain_constraint: None,
            domain_default: None,
            domain_not_null: false,
            range_subtype: Some(row.get("subtype")),
            range_subtype_opclass: row.get("subtype_opclass"),
            range_canonical: if canonical.as_deref() == Some("-") {
                None
            } else {
                canonical
            },
            range_subtype_diff: if subtype_diff.as_deref() == Some("-") {
                None
            } else {
                subtype_diff
            },
        });
    }

    // Sort all types by schema then name
    types.sort_by(|a, b| {
        a.schema_name
            .cmp(&b.schema_name)
            .then_with(|| a.type_name.cmp(&b.type_name))
    });

    Ok(types)
}
