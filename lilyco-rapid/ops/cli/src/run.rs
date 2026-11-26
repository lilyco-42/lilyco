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

use crate::Result;

const BUILD_DEBOUNCE: Duration = Duration::from_millis(250);
const POLL: Duration = Duration::from_millis(100);

pub fn dev(project: &Path) -> Result<()> {
    let project = project.canonicalize()?;
    if !project.join("Rapid.toml").is_file() {
        return Err("project directory must contain Rapid.toml".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed))?;

    let api = Api::new(project);
    let (tx, rx) = mpsc::channel();
    let _watcher = watch(&api.project, tx)?;
    let server = api.build_and_start()?;
    let mut web = start_web(&api.project.join("web"))?;
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
}

impl Api {
    fn new(project: PathBuf) -> Self {
        let target = project.join("target");
        let executable = format!("rapid-server{}", std::env::consts::EXE_SUFFIX);
        Self {
            project,
            target,
            executable,
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
        let child = Command::new(staged.path().join(&self.executable))
            .current_dir(&self.project)
            .spawn()?;
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

fn start_web(web: &Path) -> Result<Option<Running>> {
    if !web.join("package.json").is_file() {
        return Ok(None);
    }
    checked(
        Command::new(if cfg!(windows) { "pnpm.cmd" } else { "pnpm" })
            .current_dir(web)
            .args(["install", "--frozen-lockfile"]),
    )?;
    let child = Command::new("node")
        .current_dir(web)
        .arg("node_modules/vite/bin/vite.js")
        .spawn()?;
    Ok(Some(Running {
        child,
        _staged: None,
    }))
}

fn checked(command: &mut Command) -> Result<()> {
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
