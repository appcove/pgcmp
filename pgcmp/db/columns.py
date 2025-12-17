"""Column dataclass and query for PostgreSQL column introspection."""

from dataclasses import dataclass

import psycopg


@dataclass(frozen=True)
class Column:
    """Represents a PostgreSQL table column."""

    table_schema: str
    table_name: str
    column_name: str
    column_default: str | None
    is_nullable: str
    data_type: str
    character_maximum_length: int | None
    numeric_precision: int | None
    numeric_scale: int | None

    @property
    def key(self) -> str:
        """Unique identifier for comparison."""
        return f"{self.table_schema}.{self.table_name}.{self.column_name}"

    def __str__(self) -> str:
        return f"Column({self.key})"


QUERY = """
SELECT
    table_schema,
    table_name,
    column_name,
    column_default,
    is_nullable,
    data_type,
    character_maximum_length,
    numeric_precision,
    numeric_scale
FROM information_schema.columns
WHERE table_schema NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
  AND table_schema NOT LIKE 'pg_temp_%'
ORDER BY table_schema, table_name, column_name
"""


def fetch_columns(conn: psycopg.Connection) -> dict[str, Column]:
    """Fetch all columns from the database."""
    columns = {}
    with conn.cursor() as cur:
        cur.execute(QUERY)
        for row in cur.fetchall():
            column = Column(
                table_schema=row[0],
                table_name=row[1],
                column_name=row[2],
                column_default=row[3],
                is_nullable=row[4],
                data_type=row[5],
                character_maximum_length=row[6],
                numeric_precision=row[7],
                numeric_scale=row[8],
            )
            columns[column.key] = column
    return columns
