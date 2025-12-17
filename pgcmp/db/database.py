"""Main Database class that collects all schema objects."""

import re
from dataclasses import dataclass, field

import psycopg

from .schemas import Schema, fetch_schemas
from .tables import Table, fetch_tables, fetch_row_counts
from .columns import Column, fetch_columns
from .indexes import Index, fetch_indexes
from .constraints import Constraint, fetch_constraints
from .views import View, fetch_views
from .triggers import Trigger, fetch_triggers
from .functions import Function, fetch_functions
from .materialized_views import MaterializedView, fetch_materialized_views
from .sequences import Sequence, fetch_sequences


def _fetch_postgres_version(conn: psycopg.Connection) -> str:
    """Fetch PostgreSQL version string from the database."""
    with conn.cursor() as cur:
        cur.execute("SELECT version()")
        row = cur.fetchone()
        return row[0] if row else ""


def _extract_major_version(version_string: str) -> str:
    """Extract major version (e.g., '16' or '15.4') from version string."""
    # Match patterns like "PostgreSQL 16.1" or "PostgreSQL 15.4"
    match = re.search(r"PostgreSQL (\d+(?:\.\d+)?)", version_string)
    return match.group(1) if match else ""


@dataclass
class Database:
    """Represents a complete PostgreSQL database schema snapshot."""

    connection_string: str
    postgres_version: str = ""
    schemas: dict[str, Schema] = field(default_factory=dict)
    tables: dict[str, Table] = field(default_factory=dict)
    columns: dict[str, Column] = field(default_factory=dict)
    indexes: dict[str, Index] = field(default_factory=dict)
    constraints: dict[str, Constraint] = field(default_factory=dict)
    views: dict[str, View] = field(default_factory=dict)
    triggers: dict[str, Trigger] = field(default_factory=dict)
    functions: dict[str, Function] = field(default_factory=dict)
    materialized_views: dict[str, MaterializedView] = field(default_factory=dict)
    sequences: dict[str, Sequence] = field(default_factory=dict)
    # Row counts before SQL was applied (only populated when apply_sql is used)
    row_counts_before_sql: dict[str, int] = field(default_factory=dict)

    @property
    def major_version(self) -> str:
        """Extract major version number from full version string."""
        return _extract_major_version(self.postgres_version)

    @classmethod
    def from_connection_string(
        cls, connection_string: str, apply_sql: str | None = None
    ) -> "Database":
        """Create a Database snapshot by connecting and fetching all objects.

        Args:
            connection_string: PostgreSQL connection string.
            apply_sql: Optional SQL to execute before fetching schema info.
                       The SQL is executed in a transaction that is rolled back
                       after fetching, so changes are not persisted.
        """
        db = cls(connection_string=connection_string)
        db.fetch_all(apply_sql=apply_sql)
        return db

    def fetch_all(self, apply_sql: str | None = None) -> None:
        """Fetch all schema objects from the database.

        Args:
            apply_sql: Optional SQL to execute before fetching schema info.
                       The SQL is executed in a transaction that is rolled back
                       after fetching, so changes are not persisted.
                       When apply_sql is provided, row counts are captured both
                       before and after applying the SQL for comparison.
        """
        with psycopg.connect(self.connection_string, autocommit=True) as conn:
            try:
                with conn.transaction():
                    if apply_sql:
                        # Capture row counts BEFORE applying SQL
                        self.row_counts_before_sql = fetch_row_counts(conn)
                        with conn.cursor() as cur:
                            cur.execute(apply_sql)  # type: ignore[arg-type]

                    self.postgres_version = _fetch_postgres_version(conn)
                    self.schemas = fetch_schemas(conn)
                    self.tables = fetch_tables(conn)
                    self.columns = fetch_columns(conn)
                    self.indexes = fetch_indexes(conn)
                    self.constraints = fetch_constraints(conn)
                    self.views = fetch_views(conn)
                    self.triggers = fetch_triggers(conn)
                    self.functions = fetch_functions(conn)
                    self.materialized_views = fetch_materialized_views(conn)
                    self.sequences = fetch_sequences(conn)

                    raise psycopg.Rollback()
            except psycopg.Rollback:
                pass

    def summary(self) -> str:
        """Return a summary of the database contents."""
        return (
            f"Database Summary:\n"
            f"  Schemas: {len(self.schemas)}\n"
            f"  Tables: {len(self.tables)}\n"
            f"  Columns: {len(self.columns)}\n"
            f"  Indexes: {len(self.indexes)}\n"
            f"  Constraints: {len(self.constraints)}\n"
            f"  Views: {len(self.views)}\n"
            f"  Triggers: {len(self.triggers)}\n"
            f"  Functions: {len(self.functions)}\n"
            f"  Materialized Views: {len(self.materialized_views)}\n"
            f"  Sequences: {len(self.sequences)}"
        )
