use futures_util::future::join_all;
use std::ops::Deref;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio_postgres::{Client, NoTls};

/// Connections opened per database for parallel catalog queries.
///
/// PostgreSQL runs one query at a time per connection, so fetching object
/// types (and counting rows) concurrently needs several of them.
pub const POOL_SIZE: usize = 8;

pub struct DbConnection {
    client: Client,
}

impl DbConnection {
    pub async fn connect(connection_string: &str) -> anyhow::Result<Self> {
        let connect_future = tokio_postgres::connect(connection_string, NoTls);

        let (client, connection) = tokio::time::timeout(Duration::from_secs(2), connect_future)
            .await
            .map_err(|_| anyhow::anyhow!("Connection timed out (2s)"))??;

        // Spawn connection handler
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("Database connection error: {}", e);
            }
        });

        Ok(Self { client })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}

/// A fixed set of connections to one database, checked out one task at a time.
///
/// Work fanned out over a pool runs with as much parallelism as the pool has
/// connections. A pool of one (`DbPool::single`) serializes the same work on
/// that connection, which is how `pgcmp test` keeps every query inside the
/// migration's open transaction.
pub struct DbPool {
    connections: Vec<DbConnection>,
    /// Indexes into `connections` that are not checked out.
    idle: Mutex<Vec<usize>>,
    /// One permit per connection, so `get` waits instead of finding none idle.
    permits: Semaphore,
}

impl DbPool {
    /// Open up to `POOL_SIZE` connections concurrently.
    ///
    /// Fails only if no connection can be opened. A server that refuses some
    /// of them (e.g. a connection limit) yields a smaller pool.
    pub async fn connect(connection_string: &str) -> anyhow::Result<Self> {
        let attempts =
            join_all((0..POOL_SIZE).map(|_| DbConnection::connect(connection_string))).await;

        let mut connections = Vec::new();
        let mut first_error = None;
        for attempt in attempts {
            match attempt {
                Ok(conn) => connections.push(conn),
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }

        match first_error {
            Some(e) if connections.is_empty() => Err(e),
            _ => Ok(Self::from_connections(connections)),
        }
    }

    /// A pool over one existing connection: all work runs on it, in turn.
    pub fn single(conn: DbConnection) -> Self {
        Self::from_connections(vec![conn])
    }

    fn from_connections(connections: Vec<DbConnection>) -> Self {
        Self {
            idle: Mutex::new((0..connections.len()).collect()),
            permits: Semaphore::new(connections.len()),
            connections,
        }
    }

    /// Check out a connection, waiting until one is idle. It returns to the
    /// pool when the guard is dropped.
    pub async fn get(&self) -> anyhow::Result<PooledConnection<'_>> {
        let permit = self.permits.acquire().await?;
        let index = self
            .idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop();
        let conn = index
            .and_then(|index| Some((index, self.connections.get(index)?)))
            .map(|(index, conn)| PooledConnection {
                pool: self,
                conn,
                index,
                _permit: permit,
            });
        conn.ok_or_else(|| anyhow::anyhow!("Connection pool has no idle connection"))
    }
}

/// A connection checked out of a `DbPool`.
pub struct PooledConnection<'a> {
    pool: &'a DbPool,
    conn: &'a DbConnection,
    index: usize,
    _permit: SemaphorePermit<'a>,
}

impl Deref for PooledConnection<'_> {
    type Target = DbConnection;

    fn deref(&self) -> &DbConnection {
        self.conn
    }
}

impl Drop for PooledConnection<'_> {
    fn drop(&mut self) {
        // Runs before `_permit` is released, so the index is back in `idle`
        // by the time another task is allowed to pop one.
        self.pool
            .idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(self.index);
    }
}
