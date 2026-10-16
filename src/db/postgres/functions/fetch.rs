use tokio_postgres::Client;

/// Metadata for a function
#[derive(Debug)]
pub struct FunctionInfo {
    pub schema_name: String,
    pub function_name: String,
    /// Argument types that distinguish overloads, e.g. `integer, text`
    pub identity_args: String,
    pub definition: String,
}

impl FunctionInfo {
    /// Identity for matching functions across databases. Overloads share a
    /// name, so the identity arguments are part of the key.
    pub fn key(&self) -> (&str, &str, &str) {
        (&self.schema_name, &self.function_name, &self.identity_args)
    }

    /// Qualified name with identity arguments, e.g. `public.add(integer, integer)`
    pub fn signature(&self) -> String {
        format!(
            "{}.{}({})",
            self.schema_name, self.function_name, self.identity_args
        )
    }
}

/// Fetch all user functions from the database
pub async fn fetch_functions(client: &Client) -> anyhow::Result<Vec<FunctionInfo>> {
    let rows = client
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                p.proname AS function_name,
                pg_get_function_identity_arguments(p.oid) AS identity_args,
                pg_get_functiondef(p.oid) AS definition
            FROM pg_proc p
            JOIN pg_namespace n ON n.oid = p.pronamespace
            WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
              AND left(n.nspname, 2) <> 'x_'
              AND p.prokind IN ('f', 'p')  -- functions and procedures
              AND NOT EXISTS (
                  SELECT 1 FROM pg_depend d
                  WHERE d.classid = 'pg_proc'::regclass
                    AND d.objid = p.oid AND d.deptype = 'e'
              )
            ORDER BY n.nspname, p.proname, identity_args
            "#,
            &[],
        )
        .await?;

    let mut functions = Vec::new();

    for row in rows {
        let definition: Option<String> = row.get("definition");
        if let Some(def) = definition {
            functions.push(FunctionInfo {
                schema_name: row.get("schema_name"),
                function_name: row.get("function_name"),
                identity_args: row.get("identity_args"),
                definition: def,
            });
        }
    }

    Ok(functions)
}
