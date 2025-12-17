"""Trigger dataclass and query for PostgreSQL trigger introspection."""

from dataclasses import dataclass

import psycopg


@dataclass(frozen=True)
class Trigger:
    """Represents a PostgreSQL trigger."""

    trigger_schema: str
    trigger_name: str
    event_manipulation: str
    event_object_schema: str
    event_object_table: str
    action_timing: str
    action_orientation: str
    action_statement: str

    @property
    def key(self) -> str:
        """Unique identifier for comparison."""
        return f"{self.trigger_schema}.{self.event_object_table}.{self.trigger_name}"

    def __str__(self) -> str:
        return f"Trigger({self.key})"


QUERY = """
SELECT
    trigger_schema,
    trigger_name,
    event_manipulation,
    event_object_schema,
    event_object_table,
    action_timing,
    action_orientation,
    action_statement
FROM information_schema.triggers
WHERE trigger_schema NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
  AND trigger_schema NOT LIKE 'pg_temp_%'
ORDER BY trigger_schema, event_object_table, trigger_name
"""


def fetch_triggers(conn: psycopg.Connection) -> dict[str, Trigger]:
    """Fetch all triggers from the database."""
    triggers = {}
    with conn.cursor() as cur:
        cur.execute(QUERY)
        for row in cur.fetchall():
            trigger = Trigger(
                trigger_schema=row[0],
                trigger_name=row[1],
                event_manipulation=row[2],
                event_object_schema=row[3],
                event_object_table=row[4],
                action_timing=row[5],
                action_orientation=row[6],
                action_statement=row[7],
            )
            triggers[trigger.key] = trigger
    return triggers
