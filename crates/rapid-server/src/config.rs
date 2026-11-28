use std::{
    env,
    net::SocketAddr,
    num::{NonZeroU16, NonZeroU32, NonZeroUsize},
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub project: Project,
    pub http: Http,
    pub storage: Storage,
    pub resources: Resources,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Http {
    pub address: SocketAddr,
    pub max_body_bytes: NonZeroUsize,
    pub max_in_flight: NonZeroUsize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    pub path: PathBuf,
    pub busy_timeout_ms: NonZeroU32,
    #[serde(default)]
    pub engine: EngineKind,
    #[serde(default)]
    pub connection: Connection,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    #[default]
    Sqlite,
    Postgres,
    Mysql,
    Mongodb,
    Surrealdb,
    Turso,
}

impl std::fmt::Display for EngineKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
            Self::Mongodb => "mongodb",
            Self::Surrealdb => "surrealdb",
            Self::Turso => "turso",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Driver {
    #[default]
    Auto,
    Rusqlite,
    Sqlx,
    Seaorm,
    Mongodb,
    Surrealdb,
    Libsql,
}

impl Storage {
    pub fn driver(&self) -> Driver {
        match (self.engine, self.connection.driver) {
            (EngineKind::Sqlite, Driver::Auto) => Driver::Rusqlite,
            (EngineKind::Mongodb, Driver::Auto) => Driver::Mongodb,
            (EngineKind::Surrealdb, Driver::Auto) => Driver::Surrealdb,
            (EngineKind::Turso, Driver::Auto) => Driver::Libsql,
            (_, Driver::Auto) => Driver::Sqlx,
            (_, driver) => driver,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Connection {
    pub driver: Driver,
    pub url_env: String,
    pub max_connections: NonZeroU32,
    pub token_env: String,
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            driver: Driver::Auto,
            url_env: "RAPID_DATABASE_URL".to_owned(),
            max_connections: NonZeroU32::MIN.saturating_add(7),
            token_env: "RAPID_DATABASE_TOKEN".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    pub max_text_bytes: NonZeroU16,
    pub page_size: NonZeroU16,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("read configuration at {0}: {1}")]
    Read(PathBuf, #[source] std::io::Error),
    #[error("invalid TOML at {0}: {1}")]
    Parse(PathBuf, #[source] toml::de::Error),
    #[error("invalid JSON at {0}: {1}")]
    Json(PathBuf, #[source] serde_json::Error),
    #[error("RAPID_BIND must be a socket address: {0}")]
    Address(#[from] std::net::AddrParseError),
    #[error("cannot read RAPID_BIND: {0}")]
    Environment(#[from] env::VarError),
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content =
            std::fs::read_to_string(path).map_err(|err| ConfigError::Read(path.to_owned(), err))?;
        let mut config: Self = match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => serde_json::from_str(&content)
                .map_err(|err| ConfigError::Json(path.to_owned(), err))?,
            _ => {
                toml::from_str(&content).map_err(|err| ConfigError::Parse(path.to_owned(), err))?
            }
        };
        match env::var("RAPID_BIND") {
            Ok(address) => config.http.address = address.parse()?,
            Err(env::VarError::NotPresent) => {}
            Err(error) => return Err(error.into()),
        }
        if config.storage.path.is_relative() {
            config.storage.path = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&config.storage.path);
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(name: &str, content: &str) -> PathBuf {
        let path = env::temp_dir().join(format!("rapid-config-{}-{name}", std::process::id()));
        std::fs::write(&path, content).expect("write configuration");
        path
    }

    #[test]
    fn json_configuration_loads_and_resolves_relative_paths() {
        let path = write(
            "demo.json",
            r#"{
              "project": {"name": "json-demo"},
              "http": {"address": "127.0.0.1:8421", "max_body_bytes": 16384, "max_in_flight": 32},
              "storage": {
                "path": "data/demo.sqlite3",
                "busy_timeout_ms": 2500,
                "engine": "postgres",
                "connection": {"driver": "seaorm", "url_env": "DEMO_DATABASE_URL", "max_connections": 8}
              },
              "resources": {"max_text_bytes": 200, "page_size": 40}
            }"#,
        );
        let config = Config::load(&path).expect("load JSON configuration");
        let _ = std::fs::remove_file(&path);
        assert_eq!(config.project.name, "json-demo");
        assert_eq!(config.storage.engine, EngineKind::Postgres);
        assert_eq!(config.storage.connection.driver, Driver::Seaorm);
        assert_eq!(config.storage.connection.url_env, "DEMO_DATABASE_URL");
        assert!(config.storage.path.is_absolute());
        assert!(config.storage.path.ends_with("data/demo.sqlite3"));
    }

    #[test]
    fn toml_configuration_still_loads() {
        let path = write(
            "demo.toml",
            r#"
[project]
name = "toml-demo"

[http]
address = "127.0.0.1:8421"
max_body_bytes = 16384
max_in_flight = 32

[storage]
path = "data/demo.sqlite3"
busy_timeout_ms = 2500

[resources]
max_text_bytes = 200
page_size = 40
"#,
        );
        let config = Config::load(&path).expect("load TOML configuration");
        let _ = std::fs::remove_file(&path);
        assert_eq!(config.project.name, "toml-demo");
        assert_eq!(config.storage.engine, EngineKind::Sqlite);
        assert!(config.storage.path.is_absolute());
    }
}
