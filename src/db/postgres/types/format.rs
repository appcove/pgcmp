use super::fetch::{TypeInfo, TypeKind};
use crate::db::postgres::quoting::quote_ident;

/// Format a type into CREATE TYPE/DOMAIN DDL
pub fn format_type_ddl(type_info: &TypeInfo) -> String {
    match type_info.kind {
        TypeKind::Enum => format_enum_ddl(type_info),
        TypeKind::Composite => format_composite_ddl(type_info),
        TypeKind::Domain => format_domain_ddl(type_info),
        TypeKind::Range => format_range_ddl(type_info),
    }
}

fn format_enum_ddl(type_info: &TypeInfo) -> String {
    let labels = type_info
        .enum_labels
        .iter()
        .map(|l| format!("'{}'", l.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",\n    ");

    format!(
        "CREATE TYPE {}.{} AS ENUM (\n    {}\n);\n",
        quote_ident(&type_info.schema_name),
        quote_ident(&type_info.type_name),
        labels
    )
}

fn format_composite_ddl(type_info: &TypeInfo) -> String {
    let attrs = type_info
        .composite_attrs
        .iter()
        .map(|(name, typ)| format!("{} {}", quote_ident(name), typ))
        .collect::<Vec<_>>()
        .join(",\n    ");

    format!(
        "CREATE TYPE {}.{} AS (\n    {}\n);\n",
        quote_ident(&type_info.schema_name),
        quote_ident(&type_info.type_name),
        attrs
    )
}

fn format_domain_ddl(type_info: &TypeInfo) -> String {
    let base_type = type_info.domain_base_type.as_deref().unwrap_or("unknown");

    let mut ddl = format!(
        "CREATE DOMAIN {}.{} AS {}",
        quote_ident(&type_info.schema_name),
        quote_ident(&type_info.type_name),
        base_type
    );

    if type_info.domain_not_null {
        ddl.push_str(" NOT NULL");
    }

    if let Some(ref default) = type_info.domain_default {
        ddl.push_str(&format!(" DEFAULT {}", default));
    }

    if let Some(ref constraint) = type_info.domain_constraint {
        ddl.push_str(&format!(" {}", constraint));
    }

    ddl.push_str(";\n");
    ddl
}

fn format_range_ddl(type_info: &TypeInfo) -> String {
    let subtype = type_info.range_subtype.as_deref().unwrap_or("unknown");

    let mut parts = vec![format!("SUBTYPE = {}", subtype)];

    if let Some(ref opclass) = type_info.range_subtype_opclass {
        parts.push(format!("SUBTYPE_OPCLASS = {}", opclass));
    }

    if let Some(ref canonical) = type_info.range_canonical {
        parts.push(format!("CANONICAL = {}", canonical));
    }

    if let Some(ref diff) = type_info.range_subtype_diff {
        parts.push(format!("SUBTYPE_DIFF = {}", diff));
    }

    format!(
        "CREATE TYPE {}.{} AS RANGE (\n    {}\n);\n",
        quote_ident(&type_info.schema_name),
        quote_ident(&type_info.type_name),
        parts.join(",\n    ")
    )
}
