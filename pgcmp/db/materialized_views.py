"""Materialized view dataclass and query for PostgreSQL introspection."""

from dataclasses import dataclass

import psycopg


@dataclass(frozen=True)
class MaterializedView:
    """Represents a PostgreSQL materialized view."""

    schema_name: str
    matview_name: str
    definition: str
    has_indexes: bool
    is_populated: bool

    @property
    def key(self) -> str:
        """Unique identifier for comparison."""
        return f"{self.schema_name}.{self.matview_name}"

    def __str__(self) -> str:
        return f"MaterializedView({self.key})"


QUERY = """
SELECT
    n.nspname AS schema_name,
    c.relname AS matview_name,
    pg_get_viewdef(c.oid) AS definition,
    EXISTS(SELECT 1 FROM pg_index WHERE indrelid = c.oid) AS has_indexes,
    c.relispopulated AS is_populated
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE c.relkind = 'm'
  AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
  AND n.nspname NOT LIKE 'pg_temp_%'
ORDER BY n.nspname, c.relname
"""


def fetch_materialized_views(conn: psycopg.Connection) -> dict[str, MaterializedView]:
    """Fetch all materialized views from the database."""
    matviews = {}
    with conn.cursor() as cur:
        cur.execute(QUERY)
        for row in cur.fetchall():
            matview = MaterializedView(
                schema_name=row[0],
                matview_name=row[1],
                definition=row[2],
                has_indexes=row[3],
                is_populated=row[4],
            )
            matviews[matview.key] = matview
    return matviews
