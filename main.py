#!/usr/bin/env python3
"""pgcmp - PostgreSQL Schema Comparison Tool."""

import argparse
import sys

from rich.console import Console
from rich.table import Table

from pgcmp import (
    Database,
    analyze_databases,
    Action,
    AnalysisResult,
    SchemaAnalysis,
    TableAnalysis,
    ColumnAnalysis,
    TriggerAnalysis,
    IndexAnalysis,
    ConstraintAnalysis,
    FunctionAnalysis,
    ViewAnalysis,
    MaterializedViewAnalysis,
    generate_xml_report,
)

console = Console()


def print_comparison_table(result: AnalysisResult) -> None:
    """Display a unified comparison table from analysis results."""
    table = Table(
        title="Database Comparison",
        show_header=True,
        header_style="bold",
        show_lines=True,
    )
    table.add_column("Type", style="cyan", no_wrap=True)
    table.add_column("Item", style="white", no_wrap=True)
    table.add_column("Left", justify="right", no_wrap=True)
    table.add_column("Right", justify="right", no_wrap=True)
    table.add_column("Detail", overflow="fold")

    # Summary rows
    for row in result.summary:
        table.add_row(
            "Summary",
            row.object_type,
            str(row.left_count),
            str(row.right_count),
            "",
        )

    # Add a separator
    table.add_section()

    # Schema rows (all schemas first)
    for schema in result.schemas:
        left_mark = "[green]✓[/green]" if schema.in_left else ""
        right_mark = "[green]✓[/green]" if schema.in_right else ""
        detail = _format_schema_action(schema)

        table.add_row("Schema", schema.name, left_mark, right_mark, detail)

    # Add a separator before tables
    table.add_section()

    # Table rows (all tables grouped together)
    for schema in result.schemas:
        for tbl in schema.tables:
            left_mark = "[green]✓[/green]" if tbl.in_left else ""
            right_mark = "[green]✓[/green]" if tbl.in_right else ""
            detail = _format_table_action(tbl)

            table.add_row("Table", tbl.full_name, left_mark, right_mark, detail)

    # Add a separator before columns
    table.add_section()

    # Column rows (all columns grouped together)
    for schema in result.schemas:
        for tbl in schema.tables:
            for col in tbl.columns:
                left_mark = "[green]✓[/green]" if col.in_left else ""
                right_mark = "[green]✓[/green]" if col.in_right else ""
                detail = _format_column_detail(col)

                table.add_row("Column", col.full_name, left_mark, right_mark, detail)

    # Add a separator before views
    table.add_section()

    # View rows (all views grouped together)
    for schema in result.schemas:
        for view in schema.views:
            left_mark = "[green]✓[/green]" if view.in_left else ""
            right_mark = "[green]✓[/green]" if view.in_right else ""
            detail = _format_view_detail(view)

            table.add_row("View", view.full_name, left_mark, right_mark, detail)

    # Add a separator before materialized views
    table.add_section()

    # Materialized view rows (all materialized views grouped together)
    for schema in result.schemas:
        for matview in schema.materialized_views:
            left_mark = "[green]✓[/green]" if matview.in_left else ""
            right_mark = "[green]✓[/green]" if matview.in_right else ""
            detail = _format_materialized_view_detail(matview)

            table.add_row("Matview", matview.full_name, left_mark, right_mark, detail)

    # Add a separator before indexes
    table.add_section()

    # Index rows (all indexes grouped together)
    for schema in result.schemas:
        for tbl in schema.tables:
            for index in tbl.indexes:
                left_mark = "[green]✓[/green]" if index.in_left else ""
                right_mark = "[green]✓[/green]" if index.in_right else ""
                detail = _format_index_detail(index)

                table.add_row("Index", index.full_name, left_mark, right_mark, detail)

    # Add a separator before constraints
    table.add_section()

    # Constraint rows (all constraints grouped together)
    for schema in result.schemas:
        for tbl in schema.tables:
            for constraint in tbl.constraints:
                left_mark = "[green]✓[/green]" if constraint.in_left else ""
                right_mark = "[green]✓[/green]" if constraint.in_right else ""
                detail = _format_constraint_detail(constraint)

                table.add_row(
                    "Constraint", constraint.full_name, left_mark, right_mark, detail
                )

    # Add a separator before triggers
    table.add_section()

    # Trigger rows (all triggers grouped together)
    for schema in result.schemas:
        for tbl in schema.tables:
            for trigger in tbl.triggers:
                left_mark = "[green]✓[/green]" if trigger.in_left else ""
                right_mark = "[green]✓[/green]" if trigger.in_right else ""
                detail = _format_trigger_detail(trigger)

                table.add_row(
                    "Trigger", trigger.full_name, left_mark, right_mark, detail
                )

    # Add a separator before functions
    table.add_section()

    # Function rows (all functions grouped together)
    for schema in result.schemas:
        for func in schema.functions:
            left_mark = "[green]✓[/green]" if func.in_left else ""
            right_mark = "[green]✓[/green]" if func.in_right else ""
            detail = _format_function_detail(func)

            table.add_row("Function", func.full_name, left_mark, right_mark, detail)

    console.print(table)


def print_row_counts(db: "Database") -> int:
    """Print row count comparison table for tables before/after SQL was applied.

    Compares row counts captured before applying SQL to row counts after.
    Returns the number of tables with different row counts.
    """
    before_counts = db.row_counts_before_sql
    # Get after counts from the tables dict
    after_counts = {key: tbl.row_count or 0 for key, tbl in db.tables.items()}

    # Get all table keys (union of before and after)
    all_tables = sorted(set(before_counts.keys()) | set(after_counts.keys()))

    differences = 0

    table = Table(
        title="Row Count Comparison (Before/After SQL)",
        show_header=True,
        header_style="bold",
        show_lines=True,
    )
    table.add_column("Table", style="white", no_wrap=True)
    table.add_column("Before", justify="right", no_wrap=True)
    table.add_column("After", justify="right", no_wrap=True)
    table.add_column("Change", justify="right", no_wrap=True)
    table.add_column("Status", no_wrap=True)

    for table_key in all_tables:
        in_before = table_key in before_counts
        in_after = table_key in after_counts
        before_count = before_counts.get(table_key, 0)
        after_count = after_counts.get(table_key, 0)

        if in_before and in_after:
            # Table exists in both - show both counts and change
            if before_count != after_count:
                differences += 1
                diff = after_count - before_count
                change = f"+{diff}" if diff > 0 else str(diff)
                status = "[cyan]modified[/cyan]"
            else:
                change = ""
                status = "[green]match[/green]"
            table.add_row(
                table_key, str(before_count), str(after_count), change, status
            )
        elif in_before and not in_after:
            # Table was removed - show rows that were removed
            differences += 1
            table.add_row(
                table_key,
                str(before_count),
                "-",
                f"-{before_count}",
                "[red]removed[/red]",
            )
        else:
            # Table was added - show rows that were added
            differences += 1
            table.add_row(
                table_key,
                "-",
                str(after_count),
                f"+{after_count}",
                "[yellow]added[/yellow]",
            )

    console.print()
    console.print(table)
    console.print(f"\n[bold]Tables with different row counts: {differences}[/bold]")

    return differences


def _format_schema_action(schema: SchemaAnalysis) -> str:
    """Format detail for a schema."""
    if schema.action == Action.ADD:
        return f"[yellow]{schema.action_description}[/yellow]"
    elif schema.action == Action.REMOVE:
        return f"[red]{schema.action_description}[/red]"
    return ""


def _format_table_action(tbl: TableAnalysis) -> str:
    """Format detail for a table."""
    if tbl.action == Action.ADD:
        return f"[yellow]{tbl.action_description}[/yellow]"
    elif tbl.action == Action.REMOVE:
        return f"[red]{tbl.action_description}[/red]"
    return ""


def _format_column_detail(col: ColumnAnalysis) -> str:
    """Format detail for a column."""
    if col.action == Action.ADD:
        return f"[yellow]{col.action_description}[/yellow]"
    elif col.action == Action.REMOVE:
        return f"[red]{col.action_description}[/red]"
    elif col.action == Action.MODIFY:
        return f"[cyan]{col.modification_detail}[/cyan]"
    return ""


def _format_view_detail(view: ViewAnalysis) -> str:
    """Format detail for a view."""
    if view.action == Action.ADD:
        return f"[yellow]{view.action_description}[/yellow]"
    elif view.action == Action.REMOVE:
        return f"[red]{view.action_description}[/red]"
    elif view.action == Action.MODIFY:
        return f"[cyan]{view.modification_detail}[/cyan]"
    return ""


def _format_materialized_view_detail(matview: MaterializedViewAnalysis) -> str:
    """Format detail for a materialized view."""
    if matview.action == Action.ADD:
        return f"[yellow]{matview.action_description}[/yellow]"
    elif matview.action == Action.REMOVE:
        return f"[red]{matview.action_description}[/red]"
    elif matview.action == Action.MODIFY:
        return f"[cyan]{matview.modification_detail}[/cyan]"
    return ""


def _format_trigger_detail(trigger: TriggerAnalysis) -> str:
    """Format detail for a trigger."""
    if trigger.action == Action.ADD:
        return f"[yellow]{trigger.action_description}[/yellow]"
    elif trigger.action == Action.REMOVE:
        return f"[red]{trigger.action_description}[/red]"
    elif trigger.action == Action.MODIFY:
        return f"[cyan]{trigger.modification_detail}[/cyan]"
    return ""


def _format_index_detail(index: IndexAnalysis) -> str:
    """Format detail for an index."""
    if index.action == Action.ADD:
        return f"[yellow]{index.action_description}[/yellow]"
    elif index.action == Action.REMOVE:
        return f"[red]{index.action_description}[/red]"
    elif index.action == Action.MODIFY:
        return f"[cyan]{index.modification_detail}[/cyan]"
    return ""


def _format_constraint_detail(constraint: ConstraintAnalysis) -> str:
    """Format detail for a constraint."""
    if constraint.action == Action.ADD:
        return f"[yellow]{constraint.action_description}[/yellow]"
    elif constraint.action == Action.REMOVE:
        return f"[red]{constraint.action_description}[/red]"
    elif constraint.action == Action.MODIFY:
        return f"[cyan]{constraint.modification_detail}[/cyan]"
    return ""


def _format_function_detail(func: FunctionAnalysis) -> str:
    """Format detail for a function."""
    if func.action == Action.ADD:
        return f"[yellow]{func.action_description}[/yellow]"
    elif func.action == Action.REMOVE:
        return f"[red]{func.action_description}[/red]"
    elif func.action == Action.MODIFY:
        return f"[cyan]{func.modification_detail}[/cyan]"
    return ""


def parse_args() -> argparse.Namespace:
    """Parse command line arguments."""
    parser = argparse.ArgumentParser(
        description="Compare two PostgreSQL database schemas.  The first argument is the schema you want it to be.  The second argument is the schema you have.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
    %(prog)s "postgresql://user:pass@host1:5432/db1" "postgresql://user:pass@host2:5432/db2"
    %(prog)s --xml "postgres://localhost/db1" "postgres://localhost/db2"
        """,
    )
    parser.add_argument(
        "NEW_CONNECTION_STRING",
        help="Connection string for the left (source) database",
    )
    parser.add_argument(
        "OLD_CONNECTION_STRING",
        help="Connection string for the right (target) database",
    )
    parser.add_argument(
        "--apply-sql-file",
        type=argparse.FileType("r"),
        help="SQL file to apply to old database in a transaction before comparing (rolled back after)",
    )
    parser.add_argument(
        "--xml",
        action="store_true",
        default=False,
        help="Output results in XML format instead of table format",
    )
    return parser.parse_args()


def main() -> int:
    """Main entry point for pgcmp."""
    args = parse_args()

    # Step 1: Gather all data from both databases
    console.print("[bold]Connecting to left database...[/bold]")
    try:
        left_db = Database.from_connection_string(args.NEW_CONNECTION_STRING)
    except Exception as e:
        console.print(f"[red]Error connecting to left database: {e}[/red]")
        return 1

    console.print("[bold]Connecting to right database...[/bold]")
    try:
        apply_sql = args.apply_sql_file.read() if args.apply_sql_file else None
        right_db = Database.from_connection_string(args.OLD_CONNECTION_STRING, apply_sql=apply_sql)
    except Exception as e:
        console.print(f"[red]Error connecting to right database: {e}[/red]")
        return 1

    # Step 2 & 3: Analyze databases (schemas, then tables for matching schemas)
    console.print("[bold]Analyzing differences...[/bold]")
    result = analyze_databases(left_db, right_db)

    # Display version warning if applicable
    if result.version_warning:
        console.print(f"[yellow]{result.version_warning}[/yellow]")

    # Prepare row count data if SQL was applied
    row_counts_before = None
    row_counts_after = None
    row_count_differences = 0
    if apply_sql:
        row_counts_before = right_db.row_counts_before_sql
        row_counts_after = {key: tbl.row_count or 0 for key, tbl in right_db.tables.items()}

    # Step 4: Display results in requested format
    console.print()
    if args.xml:
        # XML output - print directly without rich formatting
        xml_output = generate_xml_report(result, row_counts_before, row_counts_after)
        print(xml_output)
    else:
        # Table output using rich
        print_comparison_table(result)

        # Show row count comparison table if SQL was applied
        if apply_sql:
            console.print()
            console.print("[bold]Comparing row counts (before/after SQL)...[/bold]")
            row_count_differences = print_row_counts(right_db)

    has_differences = result.has_differences() or row_count_differences > 0
    return 0 if not has_differences else 2


if __name__ == "__main__":
    sys.exit(main())
