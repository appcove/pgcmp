use super::fetch::SequenceInfo;

/// Format a sequence into CREATE SEQUENCE DDL
pub fn format_sequence_ddl(seq: &SequenceInfo) -> String {
    let mut parts = vec![format!(
        "CREATE SEQUENCE {}.{}",
        quote_ident(&seq.schema_name),
        quote_ident(&seq.sequence_name)
    )];

    // Data type (if not bigint, which is default)
    if seq.data_type != "bigint" {
        parts.push(format!("    AS {}", seq.data_type));
    }

    // Start value
    parts.push(format!("    START WITH {}", seq.start_value));

    // Increment
    if seq.increment_by != 1 {
        parts.push(format!("    INCREMENT BY {}", seq.increment_by));
    }

    // Min value (only if non-default)
    let default_min = if seq.increment_by > 0 { 1 } else { i64::MIN };
    if seq.min_value != default_min {
        parts.push(format!("    MINVALUE {}", seq.min_value));
    }

    // Max value (only if non-default)
    let default_max = if seq.increment_by > 0 { i64::MAX } else { -1 };
    if seq.max_value != default_max {
        parts.push(format!("    MAXVALUE {}", seq.max_value));
    }

    // Cache
    if seq.cache_size != 1 {
        parts.push(format!("    CACHE {}", seq.cache_size));
    }

    // Cycle
    if seq.cycle {
        parts.push("    CYCLE".to_string());
    }

    format!("{};\n", parts.join("\n"))
}

/// Quote an identifier if needed
fn quote_ident(name: &str) -> String {
    let needs_quoting = name.chars().next().is_none_or(|c| !c.is_ascii_lowercase())
        || name
            .chars()
            .any(|c| !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '_');

    if needs_quoting {
        format!("\"{}\"", name.replace('"', "\"\""))
    } else {
        name.to_string()
    }
}
