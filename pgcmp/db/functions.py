"""Function/procedure dataclass and query for PostgreSQL introspection."""

from dataclasses import dataclass

import psycopg


@dataclass(frozen=True)
class Function:
    """Represents a PostgreSQL function or procedure."""

    schema_name: str
    function_name: str
    function_type: str  # 'function' or 'procedure'
    argument_types: str
    return_type: str | None
    function_definition: str | None
    language: str
    is_strict: bool
    volatility: str

    @property
    def key(self) -> str:
        """Unique identifier for comparison."""
        return f"{self.schema_name}.{self.function_name}({self.argument_types})"

    def __str__(self) -> str:
        return f"Function({self.key})"


QUERY = """
SELECT
    n.nspname AS schema_name,
    p.proname AS function_name,
    CASE p.prokind
        WHEN 'f' THEN 'function'
        WHEN 'p' THEN 'procedure'
        WHEN 'a' THEN 'aggregate'
        WHEN 'w' THEN 'window'
    END AS function_type,
    pg_get_function_arguments(p.oid) AS argument_types,
    pg_get_function_result(p.oid) AS return_type,
    pg_get_functiondef(p.oid) AS function_definition,
    l.lanname AS language,
    p.proisstrict AS is_strict,
    CASE p.provolatile
        WHEN 'i' THEN 'immutable'
        WHEN 's' THEN 'stable'
        WHEN 'v' THEN 'volatile'
    END AS volatility
FROM pg_proc p
JOIN pg_namespace n ON n.oid = p.pronamespace
JOIN pg_language l ON l.oid = p.prolang
WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
  AND n.nspname NOT LIKE 'pg_temp_%'
  AND p.prokind IN ('f', 'p')
ORDER BY n.nspname, p.proname, pg_get_function_arguments(p.oid)
"""


def fetch_functions(conn: psycopg.Connection) -> dict[str, Function]:
    """Fetch all functions and procedures from the database."""
    functions = {}
    with conn.cursor() as cur:
        cur.execute(QUERY)
        for row in cur.fetchall():
            func = Function(
                schema_name=row[0],
                function_name=row[1],
                function_type=row[2],
                argument_types=row[3],
                return_type=row[4],
                function_definition=row[5],
                language=row[6],
                is_strict=row[7],
                volatility=row[8],
            )
            functions[func.key] = func
    return functions
