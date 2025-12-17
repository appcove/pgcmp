"""XML report generation for database schema comparison."""

from xml.etree.ElementTree import Element, SubElement, tostring
from xml.dom import minidom

from .analysis import AnalysisResult


def generate_xml_report(
    result: AnalysisResult,
    row_counts_before: dict[str, int] | None = None,
    row_counts_after: dict[str, int] | None = None,
) -> str:
    """Generate an XML report from analysis results.

    Args:
        result: The AnalysisResult containing comparison data.
        row_counts_before: Optional dict of table row counts before SQL was applied.
        row_counts_after: Optional dict of table row counts after SQL was applied.

    Returns:
        Pretty-printed XML string.
    """
    root = Element("database_comparison")

    # Add connection info
    connections = SubElement(root, "connections")
    left_conn = SubElement(connections, "left")
    left_conn.text = result.left_db.connection_string
    left_conn.set("postgres_version", result.left_db.major_version)
    right_conn = SubElement(connections, "right")
    right_conn.text = result.right_db.connection_string
    right_conn.set("postgres_version", result.right_db.major_version)

    # Add version warning if applicable
    if result.version_warning:
        SubElement(root, "version_warning").text = result.version_warning

    # Add summary section
    summary = SubElement(root, "summary")
    for row in result.summary:
        item = SubElement(summary, "item")
        SubElement(item, "type").text = row.object_type
        SubElement(item, "left_count").text = str(row.left_count)
        SubElement(item, "right_count").text = str(row.right_count)
        SubElement(item, "different").text = str(row.is_different).lower()

    # Add all object type sections
    schemas = SubElement(root, "schemas")
    tables = SubElement(root, "tables")
    columns = SubElement(root, "columns")
    indexes = SubElement(root, "indexes")
    constraints = SubElement(root, "constraints")
    views = SubElement(root, "views")
    materialized_views = SubElement(root, "materialized_views")
    triggers = SubElement(root, "triggers")
    functions = SubElement(root, "functions")

    # Single iteration over schemas to process schemas and their objects
    # Only output items that have differences (require action)
    for schema in result.schemas:
        # Output schema info only if different
        if schema.is_different:
            schema_elem = SubElement(schemas, "schema")
            SubElement(schema_elem, "name").text = schema.name
            SubElement(schema_elem, "action").text = schema.action_description

        # Output tables, columns, indexes, and triggers for this schema
        for table in schema.tables:
            if table.is_different:
                table_elem = SubElement(tables, "table")
                SubElement(table_elem, "name").text = table.full_name
                SubElement(table_elem, "action").text = table.action_description

            # Output columns for this table
            for col in table.columns:
                if col.is_different:
                    col_elem = SubElement(columns, "column")
                    SubElement(col_elem, "name").text = col.full_name
                    SubElement(col_elem, "action").text = col.action_description
                    if col.is_modified:
                        SubElement(col_elem, "detail").text = col.modification_detail

            # Output indexes for this table
            for index in table.indexes:
                if index.is_different:
                    index_elem = SubElement(indexes, "index")
                    SubElement(index_elem, "name").text = index.full_name
                    SubElement(index_elem, "action").text = index.action_description
                    if index.is_modified:
                        SubElement(
                            index_elem, "detail"
                        ).text = index.modification_detail

            # Output constraints for this table
            for constraint in table.constraints:
                if constraint.is_different:
                    constraint_elem = SubElement(constraints, "constraint")
                    SubElement(constraint_elem, "name").text = constraint.full_name
                    SubElement(
                        constraint_elem, "action"
                    ).text = constraint.action_description
                    if constraint.is_modified:
                        SubElement(
                            constraint_elem, "detail"
                        ).text = constraint.modification_detail

            # Output triggers for this table
            for trigger in table.triggers:
                if trigger.is_different:
                    trigger_elem = SubElement(triggers, "trigger")
                    SubElement(trigger_elem, "name").text = trigger.full_name
                    SubElement(trigger_elem, "action").text = trigger.action_description
                    if trigger.is_modified:
                        SubElement(
                            trigger_elem, "detail"
                        ).text = trigger.modification_detail

        # Output views for this schema
        for view in schema.views:
            if view.is_different:
                view_elem = SubElement(views, "view")
                SubElement(view_elem, "name").text = view.full_name
                SubElement(view_elem, "action").text = view.action_description
                if view.is_modified:
                    SubElement(view_elem, "detail").text = view.modification_detail

        # Output materialized views for this schema
        for matview in schema.materialized_views:
            if matview.is_different:
                matview_elem = SubElement(materialized_views, "materialized_view")
                SubElement(matview_elem, "name").text = matview.full_name
                SubElement(matview_elem, "action").text = matview.action_description
                if matview.is_modified:
                    SubElement(
                        matview_elem, "detail"
                    ).text = matview.modification_detail

        # Output functions for this schema
        for func in schema.functions:
            if func.is_different:
                func_elem = SubElement(functions, "function")
                SubElement(func_elem, "name").text = func.full_name
                SubElement(func_elem, "action").text = func.action_description
                if func.is_modified:
                    SubElement(func_elem, "detail").text = func.modification_detail

    # Add row_counts section if before/after data is provided
    if row_counts_before is not None and row_counts_after is not None:
        row_counts_elem = SubElement(root, "row_counts")
        all_tables = sorted(
            set(row_counts_before.keys()) | set(row_counts_after.keys())
        )
        for table_key in all_tables:
            in_before = table_key in row_counts_before
            in_after = table_key in row_counts_after
            before_count = row_counts_before.get(table_key, 0)
            after_count = row_counts_after.get(table_key, 0)

            # Determine action based on presence in before/after
            if in_before and in_after:
                # Table exists in both - show change if any
                if before_count != after_count:
                    row_count_elem = SubElement(row_counts_elem, "row_count")
                    SubElement(row_count_elem, "name").text = table_key
                    SubElement(row_count_elem, "action").text = "modified"
                    SubElement(row_count_elem, "before_count").text = str(before_count)
                    SubElement(row_count_elem, "after_count").text = str(after_count)
                    diff = after_count - before_count
                    SubElement(row_count_elem, "change").text = (
                        f"+{diff}" if diff > 0 else str(diff)
                    )
            elif in_before and not in_after:
                # Table was removed - show rows that were removed
                row_count_elem = SubElement(row_counts_elem, "row_count")
                SubElement(row_count_elem, "name").text = table_key
                SubElement(row_count_elem, "action").text = "removed"
                SubElement(row_count_elem, "rows_removed").text = str(before_count)
            else:
                # Table was added - show rows that were added
                row_count_elem = SubElement(row_counts_elem, "row_count")
                SubElement(row_count_elem, "name").text = table_key
                SubElement(row_count_elem, "action").text = "added"
                SubElement(row_count_elem, "rows_added").text = str(after_count)

    # Add difference count
    SubElement(root, "number_of_differences").text = str(result.count_differences())

    # Pretty print the XML
    rough_string = tostring(root, encoding="unicode")
    reparsed = minidom.parseString(rough_string)
    return reparsed.toprettyxml(indent="  ")
