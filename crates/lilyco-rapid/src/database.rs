use std::{fs, io::Write, net::SocketAddr, path::Path, process::Command};

use clap::ValueEnum;

use crate::{Result, runtime::ContainerRuntime};

const DATABASE_URL: &str = "RAPID_DATABASE_URL";

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum Database {
    Sqlite,
    Postgres,
    Mysql,
    Mongodb,
    Surrealdb,
    Turso,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum Driver {
    Auto,
    Rusqlite,
    Sqlx,
    Seaorm,
    Mongodb,
    Surrealdb,
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
            (Database::Mongodb, Driver::Auto | Driver::Mongodb) => Ok(()),
            (Database::Mongodb, _) | (_, Driver::Mongodb) => {
                Err("--db mongodb uses --db-driver mongodb".into())
            }
            (Database::Surrealdb, Driver::Auto | Driver::Surrealdb) => Ok(()),
            (Database::Surrealdb, _) | (_, Driver::Surrealdb) => {
                Err("--db surrealdb uses --db-driver surrealdb".into())
            }
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
            Driver::Mongodb => "mongodb",
            Driver::Surrealdb => "surrealdb",
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
            (Database::Mongodb, _) => Some("mongodb"),
            (Database::Surrealdb, _) => Some("surrealdb"),
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
            Database::Postgres => include_bytes!("../ops/postgres.yaml").as_slice(),
            Database::Mysql => include_bytes!("../ops/mysql.yaml").as_slice(),
            Database::Mongodb => include_bytes!("../ops/mongodb.yaml").as_slice(),
            Database::Surrealdb => include_bytes!("../ops/surrealdb.yaml").as_slice(),
            Database::Turso => include_bytes!("../ops/turso.yaml").as_slice(),
        };
        if matches!(self.database, Database::Postgres | Database::Mysql) {
            self.write_local_credentials(path)?;
        }
        fs::write(path.join("compose.override.yaml"), recipe)?;
        Ok(())
    }

    fn write_local_credentials(self, path: &Path) -> Result<()> {
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
        Ok(())
    }
}

impl Database {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
            Self::Mongodb => "mongodb",
            Self::Surrealdb => "surrealdb",
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
