use crate::manifest::Manifest;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::Mutex;

const MAX_LOG_LINES: usize = 500;
const MAX_JOBS: usize = 32;

/// 开发任务执行器。任务 argv 只来自清单，HTTP 只负责选择任务名。
#[derive(Clone, Default)]
pub struct DevRunner {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    jobs: VecDeque<Job>,
    seq: u64,
}

struct Job {
    id: String,
    app: String,
    task: String,
    started_at: u64,
    state: Arc<Mutex<JobState>>,
}

#[derive(Default)]
struct JobState {
    running: bool,
    exit_code: Option<i32>,
    finished_at: Option<u64>,
    lines: VecDeque<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobSnapshot {
    pub id: String,
    pub app: String,
    pub task: String,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub lines: Vec<String>,
}

pub enum DevError {
    UnknownTask,
    AlreadyRunning,
    NoSource,
    Spawn(String),
}

impl DevRunner {
    pub async fn start(&self, manifest: &Manifest, task: &str) -> Result<String, DevError> {
        let spec = manifest.dev.as_ref().ok_or(DevError::UnknownTask)?;
        let argv = spec.tasks.get(task).ok_or(DevError::UnknownTask)?;
        let cwd = manifest.source_dir.as_ref().ok_or(DevError::NoSource)?;
        if !cwd.is_dir() {
            return Err(DevError::NoSource);
        }

        let mut inner = self.inner.lock().await;
        for job in inner.jobs.iter() {
            if job.app == manifest.id && job.task == task {
                let state = job.state.lock().await;
                if state.running {
                    return Err(DevError::AlreadyRunning);
                }
            }
        }

        let id = format!("{}-{}", manifest.id, inner.seq + 1);
        inner.seq += 1;
        let started_at = now_secs();
        let state = Arc::new(Mutex::new(JobState {
            running: true,
            ..JobState::default()
        }));

        let mut command = tokio::process::Command::new(&argv[0]);
        command
            .args(&argv[1..])
            .current_dir(cwd)
            .envs(&spec.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|err| DevError::Spawn(format!("启动任务失败: {err}")))?;

        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(pump(stdout, state.clone()));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(pump(stderr, state.clone()));
        }
        let waiter_state = state.clone();
        tokio::spawn(async move {
            let code = child.wait().await.ok().and_then(|status| status.code());
            let mut state = waiter_state.lock().await;
            state.running = false;
            state.exit_code = Some(code.unwrap_or(-1));
            state.finished_at = Some(now_secs());
        });

        inner.jobs.push_back(Job {
            id: id.clone(),
            app: manifest.id.clone(),
            task: task.to_string(),
            started_at,
            state,
        });
        while inner.jobs.len() > MAX_JOBS {
            let Some(index) = inner.jobs.iter().position(|job| {
                job.state
                    .try_lock()
                    .map(|state| !state.running)
                    .unwrap_or(false)
            }) else {
                break;
            };
            inner.jobs.remove(index);
        }
        Ok(id)
    }

    pub async fn list(&self, app: Option<&str>) -> Vec<JobSnapshot> {
        let inner = self.inner.lock().await;
        let mut result = Vec::new();
        for job in inner.jobs.iter().rev() {
            if app.is_some_and(|id| id != job.app) {
                continue;
            }
            let state = job.state.lock().await;
            result.push(snapshot(job, &state));
        }
        result
    }

    pub async fn logs(&self, id: &str, limit: usize) -> Option<JobSnapshot> {
        let inner = self.inner.lock().await;
        let job = inner.jobs.iter().find(|job| job.id == id)?;
        let state = job.state.lock().await;
        let mut snapshot = snapshot(job, &state);
        let skip = snapshot.lines.len().saturating_sub(limit);
        snapshot.lines = snapshot.lines.split_off(skip);
        Some(snapshot)
    }
}

fn snapshot(job: &Job, state: &JobState) -> JobSnapshot {
    let lines = state.lines.iter().cloned().collect();
    JobSnapshot {
        id: job.id.clone(),
        app: job.app.clone(),
        task: job.task.clone(),
        running: state.running,
        exit_code: state.exit_code,
        started_at: job.started_at,
        finished_at: state.finished_at,
        lines,
    }
}

async fn pump<R: AsyncRead + Unpin>(reader: R, state: Arc<Mutex<JobState>>) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut state = state.lock().await;
        if state.lines.len() == MAX_LOG_LINES {
            state.lines.pop_front();
        }
        state.lines.push_back(line);
    }
}

pub fn task_list(manifest: &Manifest) -> BTreeMap<String, Vec<String>> {
    manifest
        .dev
        .as_ref()
        .map(|dev| dev.tasks.clone())
        .unwrap_or_default()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
