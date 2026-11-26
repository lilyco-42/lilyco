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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    pub path: PathBuf,
    pub busy_timeout_ms: NonZeroU32,
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
    #[error("RAPID_BIND must be a socket address: {0}")]
    Address(#[from] std::net::AddrParseError),
    #[error("cannot read RAPID_BIND: {0}")]
    Environment(#[from] env::VarError),
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content =
            std::fs::read_to_string(path).map_err(|err| ConfigError::Read(path.to_owned(), err))?;
        let mut config: Self =
            toml::from_str(&content).map_err(|err| ConfigError::Parse(path.to_owned(), err))?;
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
