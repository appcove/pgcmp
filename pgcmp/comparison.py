"""Comparison logic for PostgreSQL database schemas."""

from dataclasses import dataclass, field, fields
from typing import Any

from .db import Database


@dataclass
class ObjectDiff:
    """Represents a difference between two objects."""

    key: str
    object_type: str
    left_only: bool = False
    right_only: bool = False
    left_value: Any = None
    right_value: Any = None
    differences: dict[str, tuple[Any, Any]] = field(default_factory=dict)


@dataclass
class ComparisonResult:
    """Contains all differences between two databases."""

    left_name: str
    right_name: str
    schema_diffs: list[ObjectDiff] = field(default_factory=list)
    table_diffs: list[ObjectDiff] = field(default_factory=list)
    view_diffs: list[ObjectDiff] = field(default_factory=list)

    def has_differences(self) -> bool:
        """Check if there are any differences."""
        return any(
            [
                self.schema_diffs,
                self.table_diffs,
                self.view_diffs,
            ]
        )


def compare_objects(
    left_dict: dict[str, Any],
    right_dict: dict[str, Any],
    object_type: str,
    ignore_fields: set[str] | None = None,
) -> list[ObjectDiff]:
    """Compare two dictionaries of objects and return differences."""
    diffs = []
    ignore_fields = ignore_fields or set()

    left_keys = set(left_dict.keys())
    right_keys = set(right_dict.keys())

    # Objects only in left
    for key in left_keys - right_keys:
        diffs.append(
            ObjectDiff(
                key=key,
                object_type=object_type,
                left_only=True,
                left_value=left_dict[key],
            )
        )

    # Objects only in right
    for key in right_keys - left_keys:
        diffs.append(
            ObjectDiff(
                key=key,
                object_type=object_type,
                right_only=True,
                right_value=right_dict[key],
            )
        )

    # Objects in both - check for differences
    for key in left_keys & right_keys:
        left_obj = left_dict[key]
        right_obj = right_dict[key]

        differences = {}
        for fld in fields(left_obj):
            if fld.name in ignore_fields:
                continue
            left_val = getattr(left_obj, fld.name)
            right_val = getattr(right_obj, fld.name)
            if left_val != right_val:
                differences[fld.name] = (left_val, right_val)

        if differences:
            diffs.append(
                ObjectDiff(
                    key=key,
                    object_type=object_type,
                    left_value=left_obj,
                    right_value=right_obj,
                    differences=differences,
                )
            )

    return diffs


def compare_databases(
    left: Database,
    right: Database,
    compare_row_counts: bool = False,
) -> ComparisonResult:
    """Compare two databases and return all differences.

    Args:
        left: The left database to compare.
        right: The right database to compare.
        compare_row_counts: If True, include row_count differences in table comparison.
    """
    result = ComparisonResult(
        left_name=left.connection_string,
        right_name=right.connection_string,
    )

    # Ignore row_count unless explicitly requested
    table_ignore = set() if compare_row_counts else {"row_count"}

    result.schema_diffs = compare_objects(left.schemas, right.schemas, "Schema")
    result.table_diffs = compare_objects(
        left.tables, right.tables, "Table", ignore_fields=table_ignore
    )
    result.view_diffs = compare_objects(left.views, right.views, "View")

    return result
