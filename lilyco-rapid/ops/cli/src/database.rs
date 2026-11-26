use std::{fs, io::Write, net::SocketAddr, path::Path, process::Command};

use clap::ValueEnum;

use crate::{Result, runtime::ContainerRuntime};

const DATABASE_URL: &str = "RAPID_DATABASE_URL";

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum Database {
    Sqlite,
    Postgres,
    Mysql,
    Turso,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum Driver {
    Auto,
    Rusqlite,
    Sqlx,
    Seaorm,
    Libsql,
}

#[derive(Clone, Copy)]
pub struct StorageChoice {
    pub database: Database,
    pub driver: Driver,
}

impl StorageChoice {
    pub fn check(self) -> Result<()> {
        match (self.database, self.driver) {
            (Database::Turso, Driver::Auto | Driver::Libsql) => Ok(()),
            (Database::Turso, _) | (_, Driver::Libsql) => {
                Err("--db turso uses --db-driver libsql".into())
            }
            (Database::Sqlite, Driver::Rusqlite) => Ok(()),
            (_, Driver::Rusqlite) => {
                Err("--db-driver rusqlite can only be used with --db sqlite".into())
            }
            _ => Ok(()),
        }
    }

    pub fn config(self, settings: &str) -> String {
        let driver = match self.driver {
            Driver::Auto => "auto",
            Driver::Rusqlite => "rusqlite",
            Driver::Sqlx => "sqlx",
            Driver::Seaorm => "seaorm",
            Driver::Libsql => "libsql",
        };
        settings.replace(
            "[storage]\n",
            &format!(
                "[storage]\nengine = \"{}\"\nconnection = {{ driver = \"{driver}\" }}\n",
                self.database.as_str()
            ),
        )
    }

    pub fn write(self, path: &Path) -> Result<()> {
        let feature = match (self.database, self.driver) {
            (Database::Turso, _) => Some("turso"),
            (_, Driver::Seaorm) => Some("seaorm"),
            (Database::Sqlite, Driver::Auto | Driver::Rusqlite) => None,
            _ => Some("sqlx-server"),
        };
        if let Some(feature) = feature {
            let manifest = path.join("server/Cargo.toml");
            let content = fs::read_to_string(&manifest)?;
            fs::write(
                manifest,
                content.replace("default = []", &format!("default = [\"{feature}\"]")),
            )?;
        }
        let recipe = match self.database {
            Database::Sqlite => return Ok(()),
            Database::Postgres => include_bytes!("../../../ops/postgres.yaml").as_slice(),
            Database::Mysql => include_bytes!("../../../ops/mysql.yaml").as_slice(),
            Database::Turso => {
                return Ok(fs::write(
                    path.join("compose.override.yaml"),
                    include_bytes!("../../../ops/turso.yaml"),
                )?);
            }
        };

        let mut settings = fs::OpenOptions::new();
        settings.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            settings.mode(0o600);
        }
        let mut settings = settings.open(path.join(".env"))?;
        writeln!(settings, "RAPID_LOCAL_DB={}", self.database.as_str())?;
        writeln!(settings, "RAPID_DB_PASSWORD={}", secret()?)?;
        if self.database == Database::Mysql {
            writeln!(settings, "RAPID_DB_ROOT_PASSWORD={}", secret()?)?;
        }
        fs::write(path.join("compose.override.yaml"), recipe)?;
        Ok(())
    }
}

impl Database {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
            Self::Turso => "turso",
        }
    }
}

pub struct LocalConnection(String);

impl LocalConnection {
    pub fn apply(&self, command: &mut Command) {
        command.env(DATABASE_URL, &self.0);
    }
}

pub fn dev_connection(
    project: &Path,
    runtime: ContainerRuntime,
) -> Result<Option<LocalConnection>> {
    let env = project.join(".env");
    if std::env::var_os(DATABASE_URL).is_some() || !env.is_file() {
        return Ok(None);
    }
    let values = dotenvy::from_path_iter(env)?
        .collect::<std::result::Result<std::collections::BTreeMap<_, _>, _>>()?;
    let Some(engine) = values.get("RAPID_LOCAL_DB") else {
        return Ok(None);
    };
    let port = match engine.as_str() {
        "postgres" => "5432",
        "mysql" => "3306",
        _ => return Err("expected RAPID_LOCAL_DB=postgres or mysql".into()),
    };
    let password = values
        .get("RAPID_DB_PASSWORD")
        .filter(|password| {
            !password.is_empty() && password.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .ok_or("invalid RAPID_DB_PASSWORD in generated .env")?;
    let program = runtime.executable();
    let status = runtime
        .compose(project)
        .args(["up", "-d", "--wait", "database"])
        .status()
        .map_err(|error| format!("could not run {program} compose: {error}"))?;
    if !status.success() {
        return Err(format!(
            "could not start local database: {program} compose exited with {status}"
        )
        .into());
    }
    let output = runtime
        .compose(project)
        .args(["port", "database", port])
        .output()?;
    if !output.status.success() {
        return Err("could not determine the local database port".into());
    }
    let address: SocketAddr = std::str::from_utf8(&output.stdout)?.trim().parse()?;
    if !address.ip().is_loopback() {
        return Err("local database port must bind to loopback".into());
    }
    let tls = if engine == "mysql" {
        "?ssl-mode=required"
    } else {
        ""
    };
    Ok(Some(LocalConnection(format!(
        "{engine}://rapid:{password}@{address}/rapid{tls}"
    ))))
}

fn secret() -> Result<String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes)
        .map_err(|_| "could not get OS randomness for database credentials")?;
    Ok(hex::encode(bytes))
}
