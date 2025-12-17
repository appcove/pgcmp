"""Index dataclass and query for PostgreSQL index introspection."""

from dataclasses import dataclass

import psycopg


@dataclass(frozen=True)
class Index:
    """Represents a PostgreSQL index."""

    schema_name: str
    table_name: str
    index_name: str
    is_unique: bool
    is_primary: bool
    index_definition: str

    @property
    def key(self) -> str:
        """Unique identifier for comparison."""
        return f"{self.schema_name}.{self.index_name}"

    def __str__(self) -> str:
        return f"Index({self.key})"


QUERY = """
SELECT
    n.nspname AS schema_name,
    t.relname AS table_name,
    i.relname AS index_name,
    ix.indisunique AS is_unique,
    ix.indisprimary AS is_primary,
    pg_get_indexdef(ix.indexrelid) AS index_definition
FROM pg_index ix
JOIN pg_class i ON i.oid = ix.indexrelid
JOIN pg_class t ON t.oid = ix.indrelid
JOIN pg_namespace n ON n.oid = t.relnamespace
WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
  AND n.nspname NOT LIKE 'pg_temp_%'
ORDER BY n.nspname, t.relname, i.relname
"""


def fetch_indexes(conn: psycopg.Connection) -> dict[str, Index]:
    """Fetch all indexes from the database."""
    indexes = {}
    with conn.cursor() as cur:
        cur.execute(QUERY)
        for row in cur.fetchall():
            index = Index(
                schema_name=row[0],
                table_name=row[1],
                index_name=row[2],
                is_unique=row[3],
                is_primary=row[4],
                index_definition=row[5],
            )
            indexes[index.key] = index
    return indexes
