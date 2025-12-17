"""Sequence dataclass and query for PostgreSQL sequence introspection."""

from dataclasses import dataclass

import psycopg


@dataclass(frozen=True)
class Sequence:
    """Represents a PostgreSQL sequence."""

    sequence_schema: str
    sequence_name: str
    data_type: str
    start_value: int
    minimum_value: int
    maximum_value: int
    increment: int
    cycle_option: str

    @property
    def key(self) -> str:
        """Unique identifier for comparison."""
        return f"{self.sequence_schema}.{self.sequence_name}"

    def __str__(self) -> str:
        return f"Sequence({self.key})"


QUERY = """
SELECT
    sequence_schema,
    sequence_name,
    data_type,
    start_value::bigint,
    minimum_value::bigint,
    maximum_value::bigint,
    increment::bigint,
    cycle_option
FROM information_schema.sequences
WHERE sequence_schema NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
  AND sequence_schema NOT LIKE 'pg_temp_%'
ORDER BY sequence_schema, sequence_name
"""


def fetch_sequences(conn: psycopg.Connection) -> dict[str, Sequence]:
    """Fetch all sequences from the database."""
    sequences = {}
    with conn.cursor() as cur:
        cur.execute(QUERY)
        for row in cur.fetchall():
            sequence = Sequence(
                sequence_schema=row[0],
                sequence_name=row[1],
                data_type=row[2],
                start_value=row[3],
                minimum_value=row[4],
                maximum_value=row[5],
                increment=row[6],
                cycle_option=row[7],
            )
            sequences[sequence.key] = sequence
    return sequences
