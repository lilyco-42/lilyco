use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::{
    Result,
    database::{self, LocalConnection},
    runtime::ContainerRuntime,
};

const BUILD_DEBOUNCE: Duration = Duration::from_millis(250);
const POLL: Duration = Duration::from_millis(100);

pub enum WebMode {
    Off,
    Project,
    Preview,
}

pub fn dev(project: &Path, web: WebMode, runtime: ContainerRuntime) -> Result<()> {
    let project = project.canonicalize()?;
    if !project.join("Rapid.toml").is_file() {
        return Err("project directory must contain Rapid.toml".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed))?;

    let web = match web {
        WebMode::Off => None,
        WebMode::Project => WebSetup::prepare(project.join("web"))?,
        WebMode::Preview => Some(WebSetup::prepare(project.join("web-next"))?.ok_or(
            "web-next is missing; generate a preview with lilyco-rapid generate frontend",
        )?),
    };
    let database = database::dev_connection(&project, runtime)?;
    let api = Api::new(project, database);
    let (tx, rx) = mpsc::channel();
    let _watcher = watch(&api.project, tx)?;
    let server = api.build_and_start()?;
    let mut web = web.map(WebSetup::start).transpose()?;
    monitor(
        &api,
        server,
        web.as_mut(),
        WatchLoop {
            stop: &stop,
            events: &rx,
        },
    )
}

struct Api {
    project: PathBuf,
    target: PathBuf,
    executable: String,
    database: Option<LocalConnection>,
}

impl Api {
    fn new(project: PathBuf, database: Option<LocalConnection>) -> Self {
        let target = project.join("target");
        let executable = format!("rapid-server{}", std::env::consts::EXE_SUFFIX);
        Self {
            project,
            target,
            executable,
            database,
        }
    }

    fn compile(&self) -> Result<()> {
        checked(
            Command::new("cargo")
                .current_dir(&self.project)
                .args(["build", "--locked", "-p", "rapid-server", "--target-dir"])
                .arg(&self.target),
        )
    }

    fn stage(&self) -> Result<tempfile::TempDir> {
        // A running executable is locked on Windows. Run a copy so Cargo can
        // build while the previous server continues to serve requests.
        let dir = tempfile::tempdir_in(&self.target)?;
        fs::copy(
            self.target.join("debug").join(&self.executable),
            dir.path().join(&self.executable),
        )?;
        Ok(dir)
    }

    fn start(&self, staged: tempfile::TempDir) -> Result<Running> {
        let mut command = Command::new(staged.path().join(&self.executable));
        command.current_dir(&self.project);
        if let Some(database) = &self.database {
            database.apply(&mut command);
        }
        let child = command.spawn()?;
        Ok(Running {
            child,
            _staged: Some(staged),
        })
    }

    fn build_and_start(&self) -> Result<Running> {
        self.compile()?;
        self.start(self.stage()?)
    }
}

fn watch(project: &Path, tx: mpsc::Sender<()>) -> Result<RecommendedWatcher> {
    let mut watcher =
        notify::recommended_watcher(move |event: notify::Result<Event>| match event {
            Ok(event)
                if matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                ) =>
            {
                let _ = tx.send(());
            }
            Ok(_) => {}
            Err(error) => eprintln!("file watcher: {error}"),
        })?;
    for name in ["core", "server"] {
        watcher.watch(&project.join(name), RecursiveMode::Recursive)?;
    }
    Ok(watcher)
}

struct WebSetup {
    dir: PathBuf,
    install: Running,
}

impl WebSetup {
    fn prepare(dir: PathBuf) -> Result<Option<Self>> {
        if !dir.join("package.json").is_file() {
            return Ok(None);
        }
        let child = Command::new(if cfg!(windows) { "pnpm.cmd" } else { "pnpm" })
            .current_dir(&dir)
            .args(["install", "--frozen-lockfile", "--prefer-offline"])
            .spawn()?;
        let install = Running {
            child,
            _staged: None,
        };
        Ok(Some(Self { dir, install }))
    }

    fn start(mut self) -> Result<Running> {
        let status = self.install.child.wait()?;
        if !status.success() {
            return Err(format!("pnpm install exited with {status}").into());
        }
        let child = Command::new("node")
            .current_dir(&self.dir)
            .arg("node_modules/vite/bin/vite.js")
            .spawn()?;
        Ok(Running {
            child,
            _staged: None,
        })
    }
}

pub(crate) fn checked(command: &mut Command) -> Result<()> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!(
            "{} exited with {status}",
            command.get_program().to_string_lossy()
        )
        .into());
    }
    Ok(())
}

struct WatchLoop<'a> {
    stop: &'a AtomicBool,
    events: &'a Receiver<()>,
}

fn monitor(
    api: &Api,
    mut server: Running,
    mut web: Option<&mut Running>,
    watch: WatchLoop<'_>,
) -> Result<()> {
    let mut change = None;
    while !watch.stop.load(Ordering::Relaxed) {
        server.check("API server")?;
        if let Some(web) = web.as_deref_mut() {
            web.check("Vite")?;
        }
        if watch.events.recv_timeout(POLL).is_ok() {
            change = Some(Instant::now());
        }
        if change.is_some_and(|time| time.elapsed() >= BUILD_DEBOUNCE) {
            if let Err(error) = api.compile().and_then(|()| api.stage()).and_then(|staged| {
                server.stop()?;
                server = api.start(staged)?;
                Ok(())
            }) {
                eprintln!("API reload: {error}");
            }
            change = None;
        }
    }
    Ok(())
}

struct Running {
    child: Child,
    _staged: Option<tempfile::TempDir>,
}

impl Running {
    fn check(&mut self, name: &str) -> Result<()> {
        if let Some(status) = self.child.try_wait()? {
            return Err(format!("{name} exited with {status}").into());
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
            self.child.wait()?;
        }
        Ok(())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
