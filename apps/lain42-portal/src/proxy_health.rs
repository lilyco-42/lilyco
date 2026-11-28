//! Shares only the public health projection of the production gateway.
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::admin_routes::{public_json, CLASSIC_PROXY_QUALITY};

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct HealthNode {
    name: String,
    region: String,
    protocol: String,
    state: NodeState,
    latency_ms: Option<f64>,
    jitter_ms: Option<f64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum NodeState {
    Reachable,
    Unreachable,
    Unverified,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Deserialize)]
struct SourceReport {
    ok: bool,
    measurement: Option<String>,
    tested_at: Option<String>,
    nodes: Vec<HealthNode>,
}

#[derive(Serialize)]
pub(crate) struct HealthReport {
    source: &'static str,
    tested_at: Option<String>,
    measurement: Option<String>,
    stale: bool,
    nodes: Vec<HealthNode>,
}

#[derive(Default)]
struct Cache {
    attempted: Option<Instant>,
    last: Option<SourceReport>,
    fresh: bool,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(Cache::default()));

#[derive(Deserialize, Serialize)]
pub(crate) struct AuditRow {
    region: String,
    protocol: String,
    state: AuditState,
    elapsed_ms: Option<u32>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum AuditState {
    Approved,
    HttpsFailed,
    Slow,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct OfficialAudit {
    checked_unix: u64,
    rows: Vec<AuditRow>,
}

pub(crate) async fn official_audit() -> Result<Json<OfficialAudit>, (StatusCode, &'static str)> {
    let doc = tokio::task::spawn_blocking(|| {
        let path = std::env::var_os("LAIN_PROXY_AUDIT_FILE")?;
        let file = std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() > 64 * 1024 {
            return None;
        }
        let report: OfficialAudit = serde_json::from_reader(file).ok()?;
        let age = lilyco_license::current_unix_secs().checked_sub(report.checked_unix)?;
        (age <= 90 * 60).then_some(report)
    })
    .await
    .ok()
    .flatten();
    doc.map(Json).ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "主站 HTTPS 审核暂无有效快照",
    ))
}

pub(crate) async fn report() -> Result<Json<HealthReport>, (StatusCode, &'static str)> {
    let mut cache = CACHE.lock().await;
    let ttl = if cache.fresh {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(15)
    };
    if cache.attempted.is_none_or(|when| when.elapsed() >= ttl) {
        cache.attempted = Some(Instant::now());
        let incoming = public_json::<SourceReport>(CLASSIC_PROXY_QUALITY)
            .await
            .filter(|report| report.ok);
        cache.fresh = incoming.is_some();
        if let Some(report) = incoming {
            cache.last = Some(report);
        }
    }
    let data = cache
        .last
        .as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "主站节点监测暂时不可用"))?;
    Ok(Json(HealthReport {
        source: "lain42.top",
        tested_at: data.tested_at.clone(),
        measurement: data.measurement.clone(),
        stale: !cache.fresh,
        nodes: data.nodes.clone(),
    }))
}
