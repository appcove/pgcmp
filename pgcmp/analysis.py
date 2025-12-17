"""Analysis module for comparing PostgreSQL database schemas.

This module provides a structured approach to database comparison:
1. Gather all data from both databases
2. Analyze schemas - produce list with left/right/different/action
3. For differing schemas only, analyze tables
4. Results power the report output
"""

from dataclasses import dataclass, field
from enum import Enum
from typing import Any

from .db import Database


class Action(Enum):
    """Action needed to sync right database to left."""

    NONE = ""
    ADD = "ADD"
    REMOVE = "REMOVE"
    MODIFY = "MODIFY"


@dataclass
class AnalysisRow:
    """Base class for analysis rows with common fields."""

    name: str
    in_left: bool
    in_right: bool
    left_object: Any = None
    right_object: Any = None

    @property
    def is_different(self) -> bool:
        """Check if left and right differ (presence or implementation)."""
        if self.in_left != self.in_right:
            return True
        return self.is_modified

    def get_modifications(self) -> list[str]:
        """Get list of modification descriptions.

        Subclasses should override this to compare specific fields.
        Returns empty list if no modifications.
        """
        return []

    @property
    def is_modified(self) -> bool:
        """Check if both exist but implementations differ."""
        return len(self.get_modifications()) > 0

    @property
    def action(self) -> Action:
        """Determine action needed to sync right to left."""
        if self.in_left and not self.in_right:
            return Action.ADD
        elif self.in_right and not self.in_left:
            return Action.REMOVE
        elif self.is_modified:
            return Action.MODIFY
        return Action.NONE


@dataclass
class SchemaAnalysis(AnalysisRow):
    """Analysis result for a single schema."""

    tables: list["TableAnalysis"] = field(default_factory=list)
    views: list["ViewAnalysis"] = field(default_factory=list)
    materialized_views: list["MaterializedViewAnalysis"] = field(default_factory=list)
    functions: list["FunctionAnalysis"] = field(default_factory=list)

    @property
    def is_different(self) -> bool:
        """Check if schema or any nested objects differ."""
        if self.in_left != self.in_right:
            return True
        # Check nested objects
        if any(t.is_different for t in self.tables):
            return True
        if any(v.is_different for v in self.views):
            return True
        if any(m.is_different for m in self.materialized_views):
            return True
        if any(f.is_different for f in self.functions):
            return True
        return False

    @property
    def action_description(self) -> str:
        """Get formatted action description."""
        if self.action == Action.ADD:
            return f"create schema {self.name}"
        elif self.action == Action.REMOVE:
            return f"drop schema {self.name}"
        return ""


@dataclass
class ColumnAnalysis(AnalysisRow):
    """Analysis result for a single column (field)."""

    schema_name: str = ""
    table_name: str = ""

    @property
    def full_name(self) -> str:
        """Return schema.table.column format."""
        return f"{self.schema_name}.{self.table_name}.{self.name}"

    def _format_column_spec(self, col: Any) -> str:
        """Format a column specification for display."""
        parts = [col.data_type]
        if col.character_maximum_length:
            parts.append(f"({col.character_maximum_length})")
        elif col.numeric_precision:
            if col.numeric_scale:
                parts.append(f"({col.numeric_precision},{col.numeric_scale})")
            else:
                parts.append(f"({col.numeric_precision})")
        parts.append("null" if col.is_nullable == "YES" else "not null")
        if col.column_default:
            parts.append(f"default {col.column_default}")
        return " ".join(parts)

    def get_modifications(self) -> list[str]:
        """Get list of modification descriptions."""
        if not (self.left_object and self.right_object):
            return []
        left, right = self.left_object, self.right_object
        mods = []
        if left.data_type != right.data_type:
            mods.append(f"type: {right.data_type} -> {left.data_type}")
        if left.character_maximum_length != right.character_maximum_length:
            mods.append(
                f"max_length: {right.character_maximum_length} -> {left.character_maximum_length}"
            )
        if left.numeric_precision != right.numeric_precision:
            mods.append(
                f"precision: {right.numeric_precision} -> {left.numeric_precision}"
            )
        if left.numeric_scale != right.numeric_scale:
            mods.append(f"scale: {right.numeric_scale} -> {left.numeric_scale}")
        if left.is_nullable != right.is_nullable:
            old_null = "null" if right.is_nullable == "YES" else "not null"
            new_null = "null" if left.is_nullable == "YES" else "not null"
            mods.append(f"nullable: {old_null} -> {new_null}")
        if left.column_default != right.column_default:
            mods.append(f"default: {right.column_default} -> {left.column_default}")
        return mods

    @property
    def modification_detail(self) -> str:
        """Get detailed old/new comparison text."""
        mods = self.get_modifications()
        if not mods:
            return ""
        left, right = self.left_object, self.right_object
        lines = [f"alter column {self.full_name}"]
        lines.append(f"  old: {self._format_column_spec(right)}")
        lines.append(f"  new: {self._format_column_spec(left)}")
        return "\n".join(lines)

    @property
    def action_description(self) -> str:
        """Get formatted action description."""
        if self.action == Action.ADD:
            return f"add column {self.full_name}"
        elif self.action == Action.REMOVE:
            return f"drop column {self.full_name}"
        elif self.action == Action.MODIFY:
            return f"alter column {self.full_name}"
        return ""


@dataclass
class TriggerAnalysis(AnalysisRow):
    """Analysis result for a single trigger."""

    schema_name: str = ""
    table_name: str = ""

    @property
    def full_name(self) -> str:
        """Return schema.table.trigger format."""
        return f"{self.schema_name}.{self.table_name}.{self.name}"

    def get_modifications(self) -> list[str]:
        """Get list of modification descriptions."""
        if not (self.left_object and self.right_object):
            return []
        left, right = self.left_object, self.right_object
        mods = []
        if left.event_manipulation != right.event_manipulation:
            mods.append(
                f"event: {right.event_manipulation} -> {left.event_manipulation}"
            )
        if left.action_timing != right.action_timing:
            mods.append(f"timing: {right.action_timing} -> {left.action_timing}")
        if left.action_orientation != right.action_orientation:
            mods.append(
                f"orientation: {right.action_orientation} -> {left.action_orientation}"
            )
        if left.action_statement != right.action_statement:
            mods.append(f"statement changed")
        return mods

    @property
    def modification_detail(self) -> str:
        """Get detailed old/new comparison text."""
        mods = self.get_modifications()
        if not mods:
            return ""
        left, right = self.left_object, self.right_object
        lines = [f"replace trigger {self.full_name}"]
        lines.append(
            f"  old: {right.action_timing} {right.event_manipulation} {right.action_orientation}"
        )
        lines.append(
            f"  new: {left.action_timing} {left.event_manipulation} {left.action_orientation}"
        )
        return "\n".join(lines)

    @property
    def action_description(self) -> str:
        """Get formatted action description."""
        if self.action == Action.ADD:
            return f"create trigger {self.full_name}"
        elif self.action == Action.REMOVE:
            return f"drop trigger {self.full_name}"
        elif self.action == Action.MODIFY:
            return f"replace trigger {self.full_name}"
        return ""


@dataclass
class IndexAnalysis(AnalysisRow):
    """Analysis result for a single index."""

    schema_name: str = ""
    table_name: str = ""

    @property
    def full_name(self) -> str:
        """Return schema.index format."""
        return f"{self.schema_name}.{self.name}"

    def get_modifications(self) -> list[str]:
        """Get list of modification descriptions."""
        if not (self.left_object and self.right_object):
            return []
        left, right = self.left_object, self.right_object
        mods = []
        if left.is_unique != right.is_unique:
            mods.append(f"unique: {right.is_unique} -> {left.is_unique}")
        if left.is_primary != right.is_primary:
            mods.append(f"primary: {right.is_primary} -> {left.is_primary}")
        if left.index_definition != right.index_definition:
            mods.append("definition changed")
        return mods

    @property
    def modification_detail(self) -> str:
        """Get detailed old/new comparison text."""
        mods = self.get_modifications()
        if not mods:
            return ""
        left, right = self.left_object, self.right_object
        lines = [f"recreate index {self.full_name}"]
        lines.append(f"  old: {right.index_definition}")
        lines.append(f"  new: {left.index_definition}")
        return "\n".join(lines)

    @property
    def action_description(self) -> str:
        """Get formatted action description."""
        if self.action == Action.ADD:
            return f"create index {self.full_name}"
        elif self.action == Action.REMOVE:
            return f"drop index {self.full_name}"
        elif self.action == Action.MODIFY:
            return f"recreate index {self.full_name}"
        return ""


@dataclass
class ConstraintAnalysis(AnalysisRow):
    """Analysis result for a single constraint."""

    schema_name: str = ""
    table_name: str = ""

    @property
    def full_name(self) -> str:
        """Return schema.table.constraint format."""
        return f"{self.schema_name}.{self.table_name}.{self.name}"

    def get_modifications(self) -> list[str]:
        """Get list of modification descriptions.

        Uses pg_get_constraintdef() output for comparison, which provides
        canonical, normalized constraint definitions.
        """
        from pgcmp.db.constraints import normalize_constraint_definition

        if not (self.left_object and self.right_object):
            return []
        left, right = self.left_object, self.right_object
        mods = []
        if left.constraint_type != right.constraint_type:
            mods.append(f"type: {right.constraint_type} -> {left.constraint_type}")
        # Normalize definitions before comparison to reduce false positives
        left_def = normalize_constraint_definition(left.constraint_definition)
        right_def = normalize_constraint_definition(right.constraint_definition)
        if left_def != right_def:
            mods.append("definition changed")
        return mods

    @property
    def modification_detail(self) -> str:
        """Get detailed old/new comparison text."""
        mods = self.get_modifications()
        if not mods:
            return ""
        left, right = self.left_object, self.right_object
        lines = [f"replace constraint {self.full_name}"]
        lines.append(f"  old: {right.constraint_definition}")
        lines.append(f"  new: {left.constraint_definition}")
        return "\n".join(lines)

    @property
    def action_description(self) -> str:
        """Get formatted action description."""
        if self.action == Action.ADD:
            return f"add constraint {self.full_name}"
        elif self.action == Action.REMOVE:
            return f"drop constraint {self.full_name}"
        elif self.action == Action.MODIFY:
            return f"replace constraint {self.full_name}"
        return ""


@dataclass
class FunctionAnalysis(AnalysisRow):
    """Analysis result for a single function."""

    schema_name: str = ""

    @property
    def full_name(self) -> str:
        """Return schema.function(args) format."""
        return f"{self.schema_name}.{self.name}"

    def get_modifications(self) -> list[str]:
        """Get list of modification descriptions."""
        if not (self.left_object and self.right_object):
            return []
        left, right = self.left_object, self.right_object
        mods = []
        if left.return_type != right.return_type:
            mods.append(f"return_type: {right.return_type} -> {left.return_type}")
        if left.language != right.language:
            mods.append(f"language: {right.language} -> {left.language}")
        if left.is_strict != right.is_strict:
            mods.append(f"strict: {right.is_strict} -> {left.is_strict}")
        if left.volatility != right.volatility:
            mods.append(f"volatility: {right.volatility} -> {left.volatility}")
        if left.function_definition != right.function_definition:
            mods.append("body changed")
        return mods

    @property
    def modification_detail(self) -> str:
        """Get detailed old/new comparison text."""
        mods = self.get_modifications()
        if not mods:
            return ""
        lines = [f"replace function {self.full_name}"]
        for mod in mods:
            lines.append(f"  {mod}")
        return "\n".join(lines)

    @property
    def action_description(self) -> str:
        """Get formatted action description."""
        if self.action == Action.ADD:
            return f"create function {self.full_name}"
        elif self.action == Action.REMOVE:
            return f"drop function {self.full_name}"
        elif self.action == Action.MODIFY:
            return f"replace function {self.full_name}"
        return ""


@dataclass
class TableAnalysis(AnalysisRow):
    """Analysis result for a single table."""

    schema_name: str = ""
    columns: list["ColumnAnalysis"] = field(default_factory=list)
    triggers: list["TriggerAnalysis"] = field(default_factory=list)
    indexes: list["IndexAnalysis"] = field(default_factory=list)
    constraints: list["ConstraintAnalysis"] = field(default_factory=list)

    @property
    def full_name(self) -> str:
        """Return schema.table format."""
        return f"{self.schema_name}.{self.name}"

    @property
    def left_row_count(self) -> int | None:
        """Get row count from left database."""
        if self.left_object:
            return self.left_object.row_count
        return None

    @property
    def right_row_count(self) -> int | None:
        """Get row count from right database."""
        if self.right_object:
            return self.right_object.row_count
        return None

    @property
    def row_count_differs(self) -> bool:
        """Check if row counts differ between databases."""
        if not (self.in_left and self.in_right):
            return False
        return self.left_row_count != self.right_row_count

    @property
    def is_different(self) -> bool:
        """Check if table or any nested objects differ.

        Note: Row count differences are checked separately via --row-counts mode.
        """
        if self.in_left != self.in_right:
            return True
        # Check nested objects
        if any(c.is_different for c in self.columns):
            return True
        if any(t.is_different for t in self.triggers):
            return True
        if any(i.is_different for i in self.indexes):
            return True
        if any(c.is_different for c in self.constraints):
            return True
        return False

    @property
    def action_description(self) -> str:
        """Get formatted action description."""
        if self.action == Action.ADD:
            return f"create table {self.full_name}"
        elif self.action == Action.REMOVE:
            return f"drop table {self.full_name}"
        return ""


@dataclass
class ViewAnalysis(AnalysisRow):
    """Analysis result for a single view."""

    schema_name: str = ""

    @property
    def full_name(self) -> str:
        """Return schema.view format."""
        return f"{self.schema_name}.{self.name}"

    def get_modifications(self) -> list[str]:
        """Get list of modification descriptions."""
        if not (self.left_object and self.right_object):
            return []
        left, right = self.left_object, self.right_object
        mods = []
        if left.is_updatable != right.is_updatable:
            mods.append(f"updatable: {right.is_updatable} -> {left.is_updatable}")
        if left.is_insertable_into != right.is_insertable_into:
            mods.append(
                f"insertable: {right.is_insertable_into} -> {left.is_insertable_into}"
            )
        if left.view_definition != right.view_definition:
            mods.append("definition changed")
        return mods

    @property
    def modification_detail(self) -> str:
        """Get detailed old/new comparison text."""
        mods = self.get_modifications()
        if not mods:
            return ""
        left, right = self.left_object, self.right_object
        lines = [f"replace view {self.full_name}"]
        lines.append(f"  old: {right.view_definition}")
        lines.append(f"  new: {left.view_definition}")
        return "\n".join(lines)

    @property
    def action_description(self) -> str:
        """Get formatted action description."""
        if self.action == Action.ADD:
            return f"create view {self.full_name}"
        elif self.action == Action.REMOVE:
            return f"drop view {self.full_name}"
        elif self.action == Action.MODIFY:
            return f"replace view {self.full_name}"
        return ""


@dataclass
class MaterializedViewAnalysis(AnalysisRow):
    """Analysis result for a single materialized view."""

    schema_name: str = ""

    @property
    def full_name(self) -> str:
        """Return schema.matview format."""
        return f"{self.schema_name}.{self.name}"

    def get_modifications(self) -> list[str]:
        """Get list of modification descriptions."""
        if not (self.left_object and self.right_object):
            return []
        left, right = self.left_object, self.right_object
        mods = []
        if left.definition != right.definition:
            mods.append("definition changed")
        return mods

    @property
    def modification_detail(self) -> str:
        """Get detailed old/new comparison text."""
        mods = self.get_modifications()
        if not mods:
            return ""
        left, right = self.left_object, self.right_object
        lines = [f"replace materialized view {self.full_name}"]
        lines.append(f"  old: {right.definition}")
        lines.append(f"  new: {left.definition}")
        return "\n".join(lines)

    @property
    def action_description(self) -> str:
        """Get formatted action description."""
        if self.action == Action.ADD:
            return f"create materialized view {self.full_name}"
        elif self.action == Action.REMOVE:
            return f"drop materialized view {self.full_name}"
        elif self.action == Action.MODIFY:
            return f"replace materialized view {self.full_name}"
        return ""


@dataclass
class SummaryRow:
    """Summary counts for an object type."""

    object_type: str
    left_count: int
    right_count: int

    @property
    def is_different(self) -> bool:
        """Check if counts differ."""
        return self.left_count != self.right_count


@dataclass
class AnalysisResult:
    """Complete analysis result for database comparison."""

    left_db: Database
    right_db: Database
    summary: list[SummaryRow] = field(default_factory=list)
    schemas: list[SchemaAnalysis] = field(default_factory=list)

    def has_differences(self) -> bool:
        """Check if there are any differences."""
        return any(s.is_different for s in self.schemas)

    def count_differences(self) -> int:
        """Count total number of differences across all objects."""
        count = 0
        for schema in self.schemas:
            if schema.in_left != schema.in_right:
                count += 1
            for table in schema.tables:
                if table.in_left != table.in_right:
                    count += 1
                for col in table.columns:
                    if col.is_different:
                        count += 1
                for idx in table.indexes:
                    if idx.is_different:
                        count += 1
                for constraint in table.constraints:
                    if constraint.is_different:
                        count += 1
                for trigger in table.triggers:
                    if trigger.is_different:
                        count += 1
            for view in schema.views:
                if view.is_different:
                    count += 1
            for matview in schema.materialized_views:
                if matview.is_different:
                    count += 1
            for func in schema.functions:
                if func.is_different:
                    count += 1
        return count

    @property
    def versions_differ(self) -> bool:
        """Check if PostgreSQL major versions differ between databases."""
        return self.left_db.major_version != self.right_db.major_version

    @property
    def version_warning(self) -> str | None:
        """Return warning message if versions differ, None otherwise."""
        if not self.versions_differ:
            return None
        return (
            f"Warning: PostgreSQL versions differ "
            f"(left: {self.left_db.major_version}, right: {self.right_db.major_version}). "
            f"Some differences may be due to version-specific output formatting."
        )


def analyze_databases(left_db: Database, right_db: Database) -> AnalysisResult:
    """Analyze two databases and produce structured comparison results.

    Steps:
    1. Gather all data (already done via Database objects)
    2. Produce summary counts
    3. Analyze schemas with left/right/different/action
    4. For schemas present in both, analyze tables

    Args:
        left_db: The left (source) database.
        right_db: The right (target) database.

    Returns:
        AnalysisResult containing all comparison data.
    """
    result = AnalysisResult(left_db=left_db, right_db=right_db)

    # Step 1: Build summary rows
    result.summary = _build_summary(left_db, right_db)

    # Step 2: Analyze schemas
    result.schemas = _analyze_schemas(left_db, right_db)

    # Step 3: For schemas in both, analyze tables, views, materialized views, and functions
    for schema in result.schemas:
        if schema.in_left and schema.in_right:
            schema.tables = _analyze_tables_for_schema(left_db, right_db, schema.name)
            schema.views = _analyze_views_for_schema(left_db, right_db, schema.name)
            schema.materialized_views = _analyze_materialized_views_for_schema(
                left_db, right_db, schema.name
            )
            schema.functions = _analyze_functions_for_schema(
                left_db, right_db, schema.name
            )

    return result


def _build_summary(left_db: Database, right_db: Database) -> list[SummaryRow]:
    """Build summary counts for all object types."""
    return [
        SummaryRow("Schemas", len(left_db.schemas), len(right_db.schemas)),
        SummaryRow("Tables", len(left_db.tables), len(right_db.tables)),
        SummaryRow("Columns", len(left_db.columns), len(right_db.columns)),
        SummaryRow("Indexes", len(left_db.indexes), len(right_db.indexes)),
        SummaryRow("Constraints", len(left_db.constraints), len(right_db.constraints)),
        SummaryRow("Views", len(left_db.views), len(right_db.views)),
        SummaryRow("Triggers", len(left_db.triggers), len(right_db.triggers)),
        SummaryRow("Functions", len(left_db.functions), len(right_db.functions)),
        SummaryRow(
            "Materialized Views",
            len(left_db.materialized_views),
            len(right_db.materialized_views),
        ),
        SummaryRow("Sequences", len(left_db.sequences), len(right_db.sequences)),
    ]


def _analyze_schemas(left_db: Database, right_db: Database) -> list[SchemaAnalysis]:
    """Analyze all schemas from both databases."""
    all_schema_names = sorted(
        set(left_db.schemas.keys()) | set(right_db.schemas.keys())
    )

    return [
        SchemaAnalysis(
            name=name,
            in_left=name in left_db.schemas,
            in_right=name in right_db.schemas,
        )
        for name in all_schema_names
    ]


def _analyze_tables_for_schema(
    left_db: Database,
    right_db: Database,
    schema_name: str,
) -> list[TableAnalysis]:
    """Analyze tables for a specific schema present in both databases."""
    # Get tables for this schema from each database
    left_tables = {
        key: tbl
        for key, tbl in left_db.tables.items()
        if tbl.table_schema == schema_name
    }
    right_tables = {
        key: tbl
        for key, tbl in right_db.tables.items()
        if tbl.table_schema == schema_name
    }

    all_table_names = sorted(set(left_tables.keys()) | set(right_tables.keys()))

    result = []
    for key in all_table_names:
        table_name = key.split(".", 1)[1] if "." in key else key
        in_left = key in left_tables
        in_right = key in right_tables

        table_analysis = TableAnalysis(
            name=table_name,
            schema_name=schema_name,
            in_left=in_left,
            in_right=in_right,
            left_object=left_tables.get(key),
            right_object=right_tables.get(key),
        )

        # Analyze columns, triggers, indexes, and constraints for tables that exist in both databases
        if in_left and in_right:
            table_analysis.columns = _analyze_columns_for_table(
                left_db, right_db, schema_name, table_name
            )
            table_analysis.triggers = _analyze_triggers_for_table(
                left_db, right_db, schema_name, table_name
            )
            table_analysis.indexes = _analyze_indexes_for_table(
                left_db, right_db, schema_name, table_name
            )
            table_analysis.constraints = _analyze_constraints_for_table(
                left_db, right_db, schema_name, table_name
            )

        result.append(table_analysis)

    return result


def _analyze_columns_for_table(
    left_db: Database,
    right_db: Database,
    schema_name: str,
    table_name: str,
) -> list[ColumnAnalysis]:
    """Analyze columns for a specific table present in both databases."""
    table_key_prefix = f"{schema_name}.{table_name}."

    left_columns = {
        key: col
        for key, col in left_db.columns.items()
        if key.startswith(table_key_prefix)
    }
    right_columns = {
        key: col
        for key, col in right_db.columns.items()
        if key.startswith(table_key_prefix)
    }

    all_column_keys = sorted(set(left_columns.keys()) | set(right_columns.keys()))

    return [
        ColumnAnalysis(
            name=key.split(".")[-1],
            schema_name=schema_name,
            table_name=table_name,
            in_left=key in left_columns,
            in_right=key in right_columns,
            left_object=left_columns.get(key),
            right_object=right_columns.get(key),
        )
        for key in all_column_keys
    ]


def _analyze_views_for_schema(
    left_db: Database,
    right_db: Database,
    schema_name: str,
) -> list[ViewAnalysis]:
    """Analyze views for a specific schema present in both databases."""
    left_views = {
        key: v for key, v in left_db.views.items() if v.table_schema == schema_name
    }
    right_views = {
        key: v for key, v in right_db.views.items() if v.table_schema == schema_name
    }

    all_view_names = sorted(set(left_views.keys()) | set(right_views.keys()))

    return [
        ViewAnalysis(
            name=key.split(".", 1)[1] if "." in key else key,
            schema_name=schema_name,
            in_left=key in left_views,
            in_right=key in right_views,
            left_object=left_views.get(key),
            right_object=right_views.get(key),
        )
        for key in all_view_names
    ]


def _analyze_materialized_views_for_schema(
    left_db: Database,
    right_db: Database,
    schema_name: str,
) -> list[MaterializedViewAnalysis]:
    """Analyze materialized views for a specific schema present in both databases."""
    left_matviews = {
        key: mv
        for key, mv in left_db.materialized_views.items()
        if mv.schema_name == schema_name
    }
    right_matviews = {
        key: mv
        for key, mv in right_db.materialized_views.items()
        if mv.schema_name == schema_name
    }

    all_matview_names = sorted(set(left_matviews.keys()) | set(right_matviews.keys()))

    return [
        MaterializedViewAnalysis(
            name=key.split(".", 1)[1] if "." in key else key,
            schema_name=schema_name,
            in_left=key in left_matviews,
            in_right=key in right_matviews,
            left_object=left_matviews.get(key),
            right_object=right_matviews.get(key),
        )
        for key in all_matview_names
    ]


def _analyze_triggers_for_table(
    left_db: Database,
    right_db: Database,
    schema_name: str,
    table_name: str,
) -> list[TriggerAnalysis]:
    """Analyze triggers for a specific table present in both databases."""
    left_triggers = {
        key: trig
        for key, trig in left_db.triggers.items()
        if trig.event_object_schema == schema_name
        and trig.event_object_table == table_name
    }
    right_triggers = {
        key: trig
        for key, trig in right_db.triggers.items()
        if trig.event_object_schema == schema_name
        and trig.event_object_table == table_name
    }

    all_trigger_keys = sorted(set(left_triggers.keys()) | set(right_triggers.keys()))

    return [
        TriggerAnalysis(
            name=key.split(".")[-1],
            schema_name=schema_name,
            table_name=table_name,
            in_left=key in left_triggers,
            in_right=key in right_triggers,
            left_object=left_triggers.get(key),
            right_object=right_triggers.get(key),
        )
        for key in all_trigger_keys
    ]


def _analyze_indexes_for_table(
    left_db: Database,
    right_db: Database,
    schema_name: str,
    table_name: str,
) -> list[IndexAnalysis]:
    """Analyze indexes for a specific table present in both databases."""
    left_indexes = {
        key: idx
        for key, idx in left_db.indexes.items()
        if idx.schema_name == schema_name and idx.table_name == table_name
    }
    right_indexes = {
        key: idx
        for key, idx in right_db.indexes.items()
        if idx.schema_name == schema_name and idx.table_name == table_name
    }

    all_index_keys = sorted(set(left_indexes.keys()) | set(right_indexes.keys()))

    return [
        IndexAnalysis(
            name=key.split(".", 1)[1] if "." in key else key,
            schema_name=schema_name,
            table_name=table_name,
            in_left=key in left_indexes,
            in_right=key in right_indexes,
            left_object=left_indexes.get(key),
            right_object=right_indexes.get(key),
        )
        for key in all_index_keys
    ]


def _analyze_constraints_for_table(
    left_db: Database,
    right_db: Database,
    schema_name: str,
    table_name: str,
) -> list[ConstraintAnalysis]:
    """Analyze constraints for a specific table present in both databases."""
    left_constraints = {
        key: c
        for key, c in left_db.constraints.items()
        if c.table_schema == schema_name and c.table_name == table_name
    }
    right_constraints = {
        key: c
        for key, c in right_db.constraints.items()
        if c.table_schema == schema_name and c.table_name == table_name
    }

    all_constraint_keys = sorted(
        set(left_constraints.keys()) | set(right_constraints.keys())
    )

    return [
        ConstraintAnalysis(
            name=key.split(".")[-1],  # key is schema.table.constraint
            schema_name=schema_name,
            table_name=table_name,
            in_left=key in left_constraints,
            in_right=key in right_constraints,
            left_object=left_constraints.get(key),
            right_object=right_constraints.get(key),
        )
        for key in all_constraint_keys
    ]


def _analyze_functions_for_schema(
    left_db: Database,
    right_db: Database,
    schema_name: str,
) -> list[FunctionAnalysis]:
    """Analyze functions for a specific schema present in both databases."""
    left_funcs = {
        key: f for key, f in left_db.functions.items() if f.schema_name == schema_name
    }
    right_funcs = {
        key: f for key, f in right_db.functions.items() if f.schema_name == schema_name
    }

    all_func_keys = sorted(set(left_funcs.keys()) | set(right_funcs.keys()))

    return [
        FunctionAnalysis(
            name=key.split(".", 1)[1] if "." in key else key,
            schema_name=schema_name,
            in_left=key in left_funcs,
            in_right=key in right_funcs,
            left_object=left_funcs.get(key),
            right_object=right_funcs.get(key),
        )
        for key in all_func_keys
    ]
