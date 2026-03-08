use std::time::Duration;
use tokio_postgres::{Client, NoTls};

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
