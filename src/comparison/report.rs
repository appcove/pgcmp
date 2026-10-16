//! XML report for `pgcmp diff` and `pgcmp test`.

use super::analysis::{AnalysisResult, ObjectAnalysis};

/// Render the comparison report. `extra_sections` is pre-rendered XML inserted
/// after the object sections (e.g. `test`'s row counts).
pub fn generate_xml(
    analysis: &AnalysisResult,
    left_conn_str: &str,
    right_conn_str: &str,
    left_version: &str,
    right_version: &str,
    extra_sections: &str,
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
        xml.push_str(&format!(
            "      <left_count>{}</left_count>\n",
            row.left_count
        ));
        xml.push_str(&format!(
            "      <right_count>{}</right_count>\n",
            row.right_count
        ));
        xml.push_str(&format!(
            "      <different>{}</different>\n",
            row.is_different().to_string().to_lowercase()
        ));
        xml.push_str("    </item>\n");
    }
    xml.push_str("  </summary>\n");

    // Schemas section
    write_object_section(&mut xml, "schemas", "schema", &analysis.schemas);

    // Extensions section
    write_object_section(&mut xml, "extensions", "extension", &analysis.extensions);

    // Types section
    write_object_section(&mut xml, "types", "type", &analysis.types);

    // Tables section
    write_object_section(&mut xml, "tables", "table", &analysis.tables);

    // Columns section
    write_object_section(&mut xml, "columns", "column", &analysis.columns);

    // Views section
    write_object_section(&mut xml, "views", "view", &analysis.views);

    // Materialized views section
    write_object_section(
        &mut xml,
        "materialized_views",
        "materialized_view",
        &analysis.materialized_views,
    );

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

    xml.push_str(extra_sections);

    // Total differences count
    xml.push_str(&format!(
        "  <number_of_differences>{}</number_of_differences>\n",
        analysis.count_differences()
    ));

    xml.push_str("</database_comparison>\n");

    xml
}

fn write_object_section(
    xml: &mut String,
    section_name: &str,
    item_name: &str,
    objects: &[ObjectAnalysis],
) {
    xml.push_str(&format!("  <{}>\n", section_name));
    for obj in objects {
        xml.push_str(&format!("    <{}>\n", item_name));
        xml.push_str(&format!("      <name>{}</name>\n", xml_escape(&obj.name)));
        xml.push_str(&format!(
            "      <action>{}</action>\n",
            xml_escape(&obj.action_description)
        ));
        if let Some(ref detail) = obj.modification_detail {
            xml.push_str(&format!("      <detail>{}</detail>\n", xml_escape(detail)));
        }
        xml.push_str(&format!("    </{}>\n", item_name));
    }
    xml.push_str(&format!("  </{}>\n", section_name));
}

/// Escape special XML characters
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
