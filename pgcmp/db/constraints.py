"""Constraint dataclass and query for PostgreSQL constraint introspection."""

import re
from dataclasses import dataclass

import psycopg

# Filter out auto-generated not_null constraints (e.g., "64602_65125_1_not_null")
AUTO_GENERATED_CONSTRAINT_RE = re.compile(r"^\d+_\d+_\d+_not_null$")


def normalize_constraint_definition(definition: str | None) -> str | None:
    """Normalize constraint definition to reduce false positives from formatting differences.

    PostgreSQL's pg_get_constraintdef() can return semantically equivalent but textually
    different definitions. This function normalizes common variations:
    - Removes type casts like ::character varying, ::text, ::text[]
    - Converts non-alphanumeric characters to single space
    - Collapses multiple spaces
    """
    if definition is None:
        return None

    result = definition

    # Remove type casts: ::character varying[], ::character varying, ::text[], ::text
    result = re.sub(r"::character varying\[\]", "", result)
    result = re.sub(r"::character varying", "", result)
    result = re.sub(r"::text\[\]", "", result)
    result = re.sub(r"::text", "", result)

    # Convert any non-alphanumeric characters to a single space
    result = re.sub(r"[^a-zA-Z0-9]+", " ", result)

    # Collapse multiple spaces to single space and strip
    result = re.sub(r"\s+", " ", result).strip()

    return result


@dataclass(frozen=True)
class Constraint:
    """Represents a PostgreSQL constraint."""

    constraint_schema: str
    constraint_name: str
    table_schema: str
    table_name: str
    constraint_type: str
    constraint_definition: str | None

    @property
    def key(self) -> str:
        """Unique identifier for comparison (schema.table.constraint)."""
        return f"{self.table_schema}.{self.table_name}.{self.constraint_name}"

    def __str__(self) -> str:
        return f"Constraint({self.key})"


# Use pg_get_constraintdef() for canonical constraint definitions.
# Avoid information_schema.check_constraints.check_clause - it's not normalized
# and produces unstable output across DDL rewrites and Postgres versions.
QUERY = """
SELECT
  tc.constraint_schema,
  tc.constraint_name,
  tc.table_schema,
  tc.table_name,
  tc.constraint_type,
  pg_get_constraintdef(c.oid) AS constraint_definition
FROM information_schema.table_constraints tc
JOIN pg_namespace tn
  ON tn.nspname = tc.table_schema
JOIN pg_class t
  ON t.relnamespace = tn.oid
 AND t.relname = tc.table_name
LEFT JOIN pg_namespace cn
  ON cn.nspname = tc.constraint_schema
LEFT JOIN pg_constraint c
  ON c.connamespace = cn.oid
 AND c.conname = tc.constraint_name
 AND c.conrelid = t.oid
WHERE tc.constraint_schema NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
  AND tc.constraint_schema NOT LIKE 'pg_temp_%'
ORDER BY tc.constraint_schema, tc.table_name, tc.constraint_name
"""


def fetch_constraints(conn: psycopg.Connection) -> dict[str, Constraint]:
    """Fetch all constraints from the database."""
    constraints = {}
    with conn.cursor() as cur:
        cur.execute(QUERY)
        for row in cur.fetchall():
            constraint_name = row[1]
            # Skip auto-generated not_null constraints
            if AUTO_GENERATED_CONSTRAINT_RE.match(constraint_name):
                continue
            constraint = Constraint(
                constraint_schema=row[0],
                constraint_name=constraint_name,
                table_schema=row[2],
                table_name=row[3],
                constraint_type=row[4],
                constraint_definition=row[5],
            )
            constraints[constraint.key] = constraint
    return constraints
