use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Child;
use tokio::sync::Mutex;

const MAX_LOG_LINES: usize = 500;

#[derive(Clone, Default)]
pub struct Supervisor {
    inner: Arc<Mutex<HashMap<String, Managed>>>,
}

struct Managed {
    child: Child,
    pid: u32,
    started_at: u64,
    logs: Arc<Mutex<VecDeque<String>>>,
    exit_code: Option<i32>,
}

pub enum StartError {
    AlreadyRunning(u32),
    Spawn(String),
}

pub struct AppStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub started_at: Option<u64>,
    pub exit_code: Option<i32>,
    pub log_lines: usize,
}

impl Supervisor {
    pub async fn start(
        &self,
        id: &str,
        program: PathBuf,
        args: &[String],
        env: &std::collections::BTreeMap<String, String>,
        cwd: Option<&Path>,
    ) -> Result<u32, StartError> {
        let mut guard = self.inner.lock().await;
        if let Some(existing) = guard.get_mut(id) {
            if existing.exit_code.is_none() && existing.child.try_wait().ok().flatten().is_none() {
                return Err(StartError::AlreadyRunning(existing.pid));
            }
            guard.remove(id);
        }

        let mut command = tokio::process::Command::new(program);
        command.args(args).envs(env);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| StartError::Spawn(format!("启动失败: {err}")))?;
        let pid = child.id().unwrap_or(0);

        let logs = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(pump(stdout, logs.clone()));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(pump(stderr, logs.clone()));
        }

        guard.insert(
            id.to_string(),
            Managed {
                child,
                pid,
                started_at: now_secs(),
                logs,
                exit_code: None,
            },
        );
        Ok(pid)
    }

    pub async fn stop(&self, id: &str) -> Result<i32, String> {
        let mut guard = self.inner.lock().await;
        let managed = guard
            .get_mut(id)
            .ok_or_else(|| format!("应用 {id} 未由管理器启动"))?;
        if managed.exit_code.is_some() {
            return Ok(managed.exit_code.unwrap_or(-1));
        }
        let code = terminate(&mut managed.child, managed.pid).await?;
        managed.exit_code = Some(code);
        Ok(code)
    }

    pub async fn status(&self, id: &str) -> Option<AppStatus> {
        let mut guard = self.inner.lock().await;
        let managed = guard.get_mut(id)?;
        if managed.exit_code.is_none() {
            if let Ok(Some(status)) = managed.child.try_wait() {
                managed.exit_code = Some(status.code().unwrap_or(-1));
            }
        }
        let log_lines = managed.logs.lock().await.len();
        Some(AppStatus {
            running: managed.exit_code.is_none(),
            pid: Some(managed.pid),
            started_at: Some(managed.started_at),
            exit_code: managed.exit_code,
            log_lines,
        })
    }

    pub async fn logs(&self, id: &str, limit: usize) -> Vec<String> {
        let guard = self.inner.lock().await;
        let Some(managed) = guard.get(id) else {
            return Vec::new();
        };
        let logs = managed.logs.lock().await;
        let skip = logs.len().saturating_sub(limit.min(MAX_LOG_LINES));
        logs.iter().skip(skip).cloned().collect()
    }
}

/// 先请求优雅退出（Unix SIGTERM），3 秒未退出再强杀。
async fn terminate(child: &mut Child, pid: u32) -> Result<i32, String> {
    #[cfg(unix)]
    {
        // SAFETY: pid 来自本进程启动的子进程；kill 失败时退回强杀。
        let sent = unsafe { libc::kill(pid as i32, libc::SIGTERM) } == 0;
        if sent {
            if let Ok(result) = tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
                let status = result.map_err(|err| format!("停止失败: {err}"))?;
                return Ok(status.code().unwrap_or(-1));
            }
        }
    }
    child
        .start_kill()
        .map_err(|err| format!("停止失败: {err}"))?;
    let status = tokio::time::timeout(Duration::from_secs(3), child.wait())
        .await
        .map_err(|_| "停止超时".to_string())?
        .map_err(|err| format!("停止失败: {err}"))?;
    Ok(status.code().unwrap_or(-1))
}

async fn pump<R: AsyncRead + Unpin>(reader: R, logs: Arc<Mutex<VecDeque<String>>>) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut logs = logs.lock().await;
        if logs.len() == MAX_LOG_LINES {
            logs.pop_front();
        }
        logs.push_back(line);
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stop_unknown_app_is_rejected() {
        let supervisor = Supervisor::default();
        assert!(supervisor.stop("nope").await.is_err());
        assert!(supervisor.status("nope").await.is_none());
        assert!(supervisor.logs("nope", 50).await.is_empty());
    }
}
