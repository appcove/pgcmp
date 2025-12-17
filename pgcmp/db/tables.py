"""Table dataclass and query for PostgreSQL table introspection."""

from dataclasses import dataclass

import psycopg


@dataclass(frozen=True)
class Table:
    """Represents a PostgreSQL table."""

    table_schema: str
    table_name: str
    table_type: str
    row_count: int | None

    @property
    def key(self) -> str:
        """Unique identifier for comparison."""
        return f"{self.table_schema}.{self.table_name}"

    def __str__(self) -> str:
        return f"Table({self.key})"


QUERY = """
SELECT
    t.table_schema,
    t.table_name,
    t.table_type
FROM information_schema.tables t
WHERE t.table_schema NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
  AND t.table_schema NOT LIKE 'pg_temp_%'
  AND t.table_type = 'BASE TABLE'
ORDER BY t.table_schema, t.table_name
"""


def fetch_tables(conn: psycopg.Connection) -> dict[str, Table]:
    """Fetch all tables from the database with actual row counts."""
    tables = {}
    with conn.cursor() as cur:
        cur.execute(QUERY)
        table_rows = cur.fetchall()

    # Fetch actual row counts for each table
    for row in table_rows:
        table_schema = row[0]
        table_name = row[1]
        table_type = row[2]

        # Get actual row count using COUNT(*)
        with conn.cursor() as cur:
            # Use quote_ident equivalent for safety
            cur.execute(f'SELECT COUNT(*) FROM "{table_schema}"."{table_name}"')
            count_row = cur.fetchone()
            row_count = count_row[0] if count_row else 0

        table = Table(
            table_schema=table_schema,
            table_name=table_name,
            table_type=table_type,
            row_count=row_count,
        )
        tables[table.key] = table
    return tables


def fetch_row_counts(conn: psycopg.Connection) -> dict[str, int]:
    """Fetch row counts for all tables by querying information_schema.

    Returns:
        A dictionary mapping table keys (schema.table_name) to row counts.
    """
    row_counts = {}
    with conn.cursor() as cur:
        cur.execute(QUERY)
        table_rows = cur.fetchall()

    for row in table_rows:
        table_schema = row[0]
        table_name = row[1]
        key = f"{table_schema}.{table_name}"

        with conn.cursor() as cur:
            cur.execute(f'SELECT COUNT(*) FROM "{table_schema}"."{table_name}"')
            count_row = cur.fetchone()
            row_counts[key] = count_row[0] if count_row else 0

    return row_counts
