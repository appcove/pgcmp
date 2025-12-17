"""Schema dataclass and query for PostgreSQL schema introspection."""

from dataclasses import dataclass

import psycopg


@dataclass(frozen=True)
class Schema:
    """Represents a PostgreSQL schema."""

    schema_name: str
    schema_owner: str

    @property
    def key(self) -> str:
        """Unique identifier for comparison."""
        return self.schema_name

    def __str__(self) -> str:
        return f"Schema({self.schema_name})"


QUERY = """
SELECT
    schema_name,
    schema_owner
FROM information_schema.schemata
WHERE schema_name NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
  AND schema_name NOT LIKE 'pg_temp_%'
  AND schema_name NOT LIKE 'pg_toast_temp_%'
ORDER BY schema_name
"""


def fetch_schemas(conn: psycopg.Connection) -> dict[str, Schema]:
    """Fetch all schemas from the database."""
    schemas = {}
    with conn.cursor() as cur:
        cur.execute(QUERY)
        for row in cur.fetchall():
            schema = Schema(
                schema_name=row[0],
                schema_owner=row[1],
            )
            schemas[schema.key] = schema
    return schemas
