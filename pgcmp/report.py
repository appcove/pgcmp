"""Report generation for database schema comparison."""

from .comparison import ComparisonResult, ObjectDiff


def format_diff(diff: ObjectDiff, indent: str = "  ") -> list[str]:
    """Format a single object difference."""
    lines = []

    if diff.left_only:
        lines.append(f"{indent}← LEFT ONLY: {diff.key}")
    elif diff.right_only:
        lines.append(f"{indent}→ RIGHT ONLY: {diff.key}")
    else:
        lines.append(f"{indent}≠ DIFFERENT: {diff.key}")
        for field_name, (left_val, right_val) in diff.differences.items():
            lines.append(f"{indent}    {field_name}:")
            lines.append(f"{indent}      ← {left_val!r}")
            lines.append(f"{indent}      → {right_val!r}")

    return lines


def format_section(title: str, diffs: list[ObjectDiff]) -> list[str]:
    """Format a section of the report."""
    if not diffs:
        return []

    lines = [
        "",
        "=" * 60,
        f" {title} ({len(diffs)} difference(s))",
        "=" * 60,
    ]

    for diff in sorted(diffs, key=lambda d: d.key):
        lines.extend(format_diff(diff))

    return lines


def generate_report(result: ComparisonResult) -> str:
    """Generate a human-readable comparison report."""
    lines = [
        "PostgreSQL Schema Comparison Report",
        "=" * 60,
        f"Left:  {result.left_name}",
        f"Right: {result.right_name}",
    ]

    if not result.has_differences():
        lines.append("")
        lines.append("✓ No differences found. Schemas are identical.")
        return "\n".join(lines)

    lines.extend(format_section("SCHEMAS", result.schema_diffs))
    lines.extend(format_section("TABLES", result.table_diffs))
    lines.extend(format_section("VIEWS", result.view_diffs))

    # Summary
    lines.append("")
    lines.append("=" * 60)
    lines.append(" SUMMARY")
    lines.append("=" * 60)

    total = 0
    for name, diffs in [
        ("Schemas", result.schema_diffs),
        ("Tables", result.table_diffs),
        ("Views", result.view_diffs),
    ]:
        if diffs:
            left_only = sum(1 for d in diffs if d.left_only)
            right_only = sum(1 for d in diffs if d.right_only)
            different = sum(1 for d in diffs if not d.left_only and not d.right_only)
            parts = []
            if left_only:
                parts.append(f"{left_only} left-only")
            if right_only:
                parts.append(f"{right_only} right-only")
            if different:
                parts.append(f"{different} different")
            lines.append(f"  {name}: {', '.join(parts)}")
            total += len(diffs)

    lines.append(f"  Total differences: {total}")

    return "\n".join(lines)
