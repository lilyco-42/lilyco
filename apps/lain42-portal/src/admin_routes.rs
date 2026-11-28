use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    db::admin_analytics::{Activity, Commerce, Period, Visit, STARTED},
    routes::{check_admin_access, AppState},
};

pub(crate) const CLASSIC_CATALOG: &str = "https://lain42.top/studio/api/products";
pub(crate) const CLASSIC_PROXY_QUALITY: &str = "https://lain42.top/studio/api/proxy/quality";

type ApiError = (StatusCode, &'static str);

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/admin/overview", get(overview))
        .route("/api/admin/catalog-comparison", get(catalog_comparison))
        .route("/api/analytics/visit", post(visit))
}

pub(crate) fn is_admin_name(username: &str) -> bool {
    let admins =
        std::env::var("LAIN_ADMIN_USERS").unwrap_or_else(|_| "lilyco42ovo,admin".to_string());
    admins
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .any(|name| username.eq_ignore_ascii_case(name))
}

#[derive(Deserialize)]
struct OverviewQuery {
    days: Option<i64>,
}

#[derive(Serialize)]
struct Overview {
    site: String,
    period: Period,
    commerce: Commerce,
    activity: Activity,
    system: RuntimeInfo,
}

#[derive(Serialize)]
struct RuntimeInfo {
    environment: String,
    version: &'static str,
    platform: String,
    uptime_secs: u64,
    db_latency_ms: f64,
    region_enabled: bool,
}

async fn overview(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<OverviewQuery>,
) -> Result<Json<Overview>, (StatusCode, String)> {
    check_admin_access(&st, &headers)?;
    let days = query.days.unwrap_or(30);
    if !(1..=90).contains(&days) {
        return Err((StatusCode::BAD_REQUEST, "统计范围应为 1–90 天".into()));
    }
    let period = Period::ending_today(days, lilyco_license::current_unix_secs() as i64);
    let (commerce, activity, latency) = tokio::try_join!(
        st.db.admin_commerce(period),
        st.db.admin_activity(period),
        st.db.admin_db_latency(),
    )
    .map_err(|err| {
        eprintln!("运营统计读取失败: {err}");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "运营统计暂时不可用，请重试".into(),
        )
    })?;
    let site = std::env::var("LAIN_ANALYTICS_SITE").unwrap_or_else(|_| "dev.lain42.top".into());
    Ok(Json(Overview {
        site,
        period,
        commerce,
        activity,
        system: RuntimeInfo {
            environment: st.env_name.clone(),
            version: env!("CARGO_PKG_VERSION"),
            platform: format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
            uptime_secs: STARTED.elapsed().as_secs(),
            db_latency_ms: latency,
            region_enabled: crate::visitor_region::enabled(),
        },
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VisitRequest {
    page: PublicPage,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum PublicPage {
    Home,
    Vpn,
    Shop,
    Downloads,
    Models,
    AccountPool,
    MyAgent,
    ApiKeys,
    Account,
    Tool,
    Campus,
}

impl PublicPage {
    fn slug(&self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Vpn => "vpn",
            Self::Shop => "shop",
            Self::Downloads => "downloads",
            Self::Models => "models",
            Self::AccountPool => "account-pool",
            Self::MyAgent => "my-agent",
            Self::ApiKeys => "api-keys",
            Self::Account => "account",
            Self::Tool => "tool",
            Self::Campus => "campus",
        }
    }
}

fn operating_system(headers: &HeaderMap) -> &'static str {
    let ua = headers
        .get("user-agent")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let hint = headers
        .get("sec-ch-ua-platform")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .trim_matches('"')
        .to_ascii_lowercase();
    if ua.contains("android") || hint == "android" {
        "Android"
    } else if ua.contains("iphone") || ua.contains("ipad") || hint == "ios" {
        "iOS"
    } else if ua.contains("windows") || hint == "windows" {
        "Windows"
    } else if ua.contains("macintosh") || hint == "macos" {
        "macOS"
    } else if ua.contains("cros") || hint == "chrome os" {
        "ChromeOS"
    } else if ua.contains("linux") || hint == "linux" {
        "Linux"
    } else {
        "unknown"
    }
}

async fn visit(
    State(st): State<Arc<AppState>>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(req): Json<VisitRequest>,
) -> Result<StatusCode, ApiError> {
    let username = crate::studio_api::extract_username(&headers, &st.license_secret).ok();
    let country =
        peer.and_then(|Extension(ConnectInfo(addr))| crate::visitor_region::lookup(addr.ip()));
    st.db
        .record_portal_visit(Visit {
            page: req.page.slug(),
            username: username.as_deref(),
            os: operating_system(&headers),
            country: country.as_deref(),
        })
        .await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "访问记录暂时不可用"))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Catalog {
    products: Vec<CatalogProduct>,
}

#[derive(Deserialize, Serialize, PartialEq)]
struct CatalogProduct {
    code: String,
    name: String,
    points_price: i64,
    price_cny: Option<f64>,
    sale_open: Option<bool>,
    duration_days: Option<i64>,
    traffic_gb: Option<i64>,
}

#[derive(Serialize)]
struct CatalogRow {
    code: String,
    dev: Option<CatalogProduct>,
    classic: Option<CatalogProduct>,
    state: &'static str,
}

#[derive(Serialize)]
struct CatalogComparison {
    fetched_unix: u64,
    classic_available: bool,
    rows: Vec<CatalogRow>,
}

pub(crate) async fn public_json<T: serde::de::DeserializeOwned + Send + 'static>(
    url: &'static str,
) -> Option<T> {
    tokio::task::spawn_blocking(move || {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .max_redirects(0)
            .build()
            .new_agent();
        let mut res = agent
            .get(url)
            .header("Accept", "application/json")
            .call()
            .ok()?;
        res.body_mut()
            .with_config()
            .limit(512 * 1024)
            .read_json()
            .ok()
    })
    .await
    .ok()
    .flatten()
}

fn compare_catalog(dev: Catalog, classic: Option<Catalog>) -> CatalogComparison {
    let classic_available = classic.is_some();
    let mut rows: BTreeMap<String, (Option<CatalogProduct>, Option<CatalogProduct>)> =
        BTreeMap::new();
    for product in dev.products {
        let code = product.code.clone();
        rows.entry(code).or_default().0 = Some(product);
    }
    if let Some(classic) = classic {
        for product in classic.products {
            let code = product.code.clone();
            rows.entry(code).or_default().1 = Some(product);
        }
    }
    CatalogComparison {
        fetched_unix: lilyco_license::current_unix_secs(),
        classic_available,
        rows: rows
            .into_iter()
            .map(|(code, (dev, classic))| {
                let state = match (&dev, &classic) {
                    _ if !classic_available => "unverified",
                    (Some(a), Some(b)) if a == b => "matched",
                    (Some(_), Some(_)) => "different",
                    (Some(_), None) => "dev_only",
                    _ => "classic_only",
                };
                CatalogRow {
                    code,
                    dev,
                    classic,
                    state,
                }
            })
            .collect(),
    }
}

async fn catalog_comparison(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<CatalogComparison>, (StatusCode, String)> {
    check_admin_access(&st, &headers)?;
    let dev = serde_json::from_value(crate::studio_api::catalog_json()).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "无法读取 dev 商品目录".into(),
        )
    })?;
    let classic = public_json(CLASSIC_CATALOG).await;
    Ok(Json(compare_catalog(dev, classic)))
}
