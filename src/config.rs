use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const CONFIG_FILENAME: &str = "CONFIG.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    #[default]
    Disable,
    Require,
}

impl TlsMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            TlsMode::Disable => "disable",
            TlsMode::Require => "require",
        }
    }

    pub fn display_str(&self) -> &'static str {
        match self {
            TlsMode::Disable => "No TLS",
            TlsMode::Require => "TLS",
        }
    }

    pub fn toggle(&self) -> Self {
        match self {
            TlsMode::Disable => TlsMode::Require,
            TlsMode::Require => TlsMode::Disable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DbConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<TlsMode>,
}

impl DbConfig {
    pub fn host(&self) -> &str {
        self.host.as_deref().unwrap_or("localhost")
    }

    pub fn port(&self) -> u16 {
        self.port.unwrap_or(5432)
    }

    pub fn user(&self) -> &str {
        self.user.as_deref().unwrap_or("")
    }

    pub fn password(&self) -> &str {
        self.password.as_deref().unwrap_or("")
    }

    pub fn database(&self) -> &str {
        self.database.as_deref().unwrap_or("")
    }

    pub fn tls(&self) -> TlsMode {
        self.tls.unwrap_or_default()
    }

    pub fn connection_string(&self) -> String {
        let password = self.password();
        let base = if password.is_empty() {
            format!(
                "postgresql://{}@{}:{}/{}",
                self.user(),
                self.host(),
                self.port(),
                self.database()
            )
        } else {
            format!(
                "postgresql://{}:{}@{}:{}/{}",
                self.user(),
                password,
                self.host(),
                self.port(),
                self.database()
            )
        };
        format!("{}?sslmode={}", base, self.tls().as_str())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<DbConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<DbConfig>,
}

impl Config {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let path = dir.join(CONFIG_FILENAME);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(config)
    }

    /// Load config, returning default if file doesn't exist or is empty/invalid
    pub fn load_or_default(dir: &Path) -> Self {
        let path = dir.join(CONFIG_FILENAME);
        match fs::read_to_string(&path) {
            Ok(content) if !content.trim().is_empty() => {
                toml::from_str(&content).unwrap_or_default()
            }
            _ => Self::default(),
        }
    }

    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        let path = dir.join(CONFIG_FILENAME);
        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }
}
