"""View dataclass and query for PostgreSQL view introspection."""

from dataclasses import dataclass

import psycopg


@dataclass(frozen=True)
class View:
    """Represents a PostgreSQL view."""

    table_schema: str
    table_name: str
    view_definition: str | None
    is_updatable: str
    is_insertable_into: str

    @property
    def key(self) -> str:
        """Unique identifier for comparison."""
        return f"{self.table_schema}.{self.table_name}"

    def __str__(self) -> str:
        return f"View({self.key})"


QUERY = """
SELECT
    table_schema,
    table_name,
    view_definition,
    is_updatable,
    is_insertable_into
FROM information_schema.views
WHERE table_schema NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
  AND table_schema NOT LIKE 'pg_temp_%'
ORDER BY table_schema, table_name
"""


def fetch_views(conn: psycopg.Connection) -> dict[str, View]:
    """Fetch all views from the database."""
    views = {}
    with conn.cursor() as cur:
        cur.execute(QUERY)
        for row in cur.fetchall():
            view = View(
                table_schema=row[0],
                table_name=row[1],
                view_definition=row[2],
                is_updatable=row[3],
                is_insertable_into=row[4],
            )
            views[view.key] = view
    return views
