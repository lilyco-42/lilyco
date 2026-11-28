//! 云枢智创 · lain42 全 Rust 门户入口（环境隔离：dev / test 独立端口与 Turso 数据库文件）。

mod admin_routes;
mod db;
mod mirror;
mod proxy_health;
mod routes;
mod studio_api;
mod visitor_region;

use std::sync::Arc;

use db::PortalDb;
use routes::{build_router, AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::sync::LazyLock::force(&db::admin_analytics::STARTED);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("serve");

    let env_name = arg_val(&args, "--env")
        .or_else(|| std::env::var("LAIN_ENV").ok())
        .unwrap_or_else(|| "dev".to_string());
    let db_path = arg_val(&args, "--db")
        .or_else(|| std::env::var("LAIN_DB").ok())
        .unwrap_or_else(|| format!("./data/lain42-{env_name}.db"));

    let db = PortalDb::open(&db_path).await?;

    match cmd {
        "db-status" => {
            let health = db.health().await?;
            println!("{}", serde_json::to_string_pretty(&health)?);
        }
        "list-models" => {
            let models = db.list_ai_models().await?;
            println!("{}", serde_json::to_string_pretty(&models)?);
        }
        "add-announcement" => {
            let date = arg_val(&args, "--date").unwrap_or_else(|| "2026-11-20".to_string());
            let title = arg_val(&args, "--title").unwrap_or_else(|| "新公告".to_string());
            let summary = arg_val(&args, "--summary").unwrap_or_default();
            let link = arg_val(&args, "--link").unwrap_or_else(|| "/shop".to_string());
            let pinned = args.iter().any(|a| a == "--pinned");
            db.add_announcement(&date, &title, &summary, &link, pinned)
                .await?;
            println!("已写入 Turso 公告：{title}");
        }
        _ => {
            let bind = arg_val(&args, "--bind")
                .or_else(|| std::env::var("LAIN_BIND").ok())
                .unwrap_or_else(|| {
                    if env_name == "test" {
                        "0.0.0.0:443".to_string()
                    } else {
                        "0.0.0.0:80".to_string()
                    }
                });
            let license_secret = arg_val(&args, "--license-secret")
                .or_else(|| std::env::var("LAIN_LICENSE_SECRET").ok())
                .unwrap_or_else(|| "lain42-dev-secret-change-in-prod".to_string());
            let static_dir =
                arg_val(&args, "--static-dir").or_else(|| std::env::var("LAIN_STATIC_DIR").ok());
            let pay_config_dir = arg_val(&args, "--pay-config-dir")
                .or_else(|| std::env::var("LAIN_PAY_CONFIG_DIR").ok());

            let state = Arc::new(AppState {
                db,
                env_name: env_name.clone(),
                license_secret,
                admin_secret: std::env::var("LAIN_ADMIN_SECRET")
                    .ok()
                    .filter(|key| key.len() >= 32),
                static_dir: static_dir.clone(),
                pay_config_dir: pay_config_dir.clone(),
            });
            let app = build_router(state);
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            println!(
                "lain42-portal ({env_name}) listening on http://{bind} [db={db_path}, static={:?}, pay_dir={:?}, engine=Turso Pure-Rust]",
                static_dir, pay_config_dir
            );
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await?;
        }
    }
    Ok(())
}

fn arg_val(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn test_app() -> axum::Router {
        let db = PortalDb::open(":memory:").await.unwrap();
        let state = Arc::new(AppState {
            db,
            env_name: "test".to_string(),
            license_secret: "unit-test-secret".to_string(),
            admin_secret: Some("unit-test-admin-secret".to_string()),
            static_dir: None,
            pay_config_dir: None,
        });
        build_router(state)
    }

    #[tokio::test]
    async fn operations_overview_counts_only_accepted_visits_and_requires_admin() {
        let app = test_app().await;
        let request = |path, body| Request::builder().uri(path).body(body).unwrap();
        let access = app
            .clone()
            .oneshot(request("/api/admin/overview", Body::empty()))
            .await
            .unwrap();
        assert_eq!(access.status(), StatusCode::FORBIDDEN);

        let register = app.clone().oneshot(Request::post("/studio/api/register")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"username":"ops_fixture","password":"fixturepass","email":"ops@example.test"}"#)).unwrap()).await.unwrap();
        assert_eq!(register.status(), StatusCode::OK);
        let registration: serde_json::Value =
            serde_json::from_slice(&register.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let authorization = format!("Bearer {}", registration["token"].as_str().unwrap());

        for (body, expected) in [
            (r#"{"page":"tool"}"#, StatusCode::NO_CONTENT),
            (r#"{"page":"vpn"}"#, StatusCode::NO_CONTENT),
            (
                r#"{"page":"tool","country":"US"}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (r#"{"page":"unknown"}"#, StatusCode::UNPROCESSABLE_ENTITY),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::post("/api/analytics/visit")
                        .header("content-type", "application/json")
                        .header("authorization", &authorization)
                        .header("user-agent", "Mozilla/5.0 (Linux; Android 16)")
                        .header("cf-ipcountry", "US")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
        }

        let campus_purchase = app
            .clone()
            .oneshot(
                Request::post("/studio/api/points/purchase")
                    .header("content-type", "application/json")
                    .header("authorization", &authorization)
                    .body(Body::from(
                        r#"{"product":"paodou_6m","reference":"fixture"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(campus_purchase.status(), StatusCode::CONFLICT);

        let admin_request = |path| {
            Request::get(path)
                .header("x-admin-secret", "unit-test-admin-secret")
                .body(Body::empty())
                .unwrap()
        };
        let invalid_period = app
            .clone()
            .oneshot(admin_request("/api/admin/overview?days=91"))
            .await
            .unwrap();
        assert_eq!(invalid_period.status(), StatusCode::BAD_REQUEST);

        let response = app
            .oneshot(admin_request("/api/admin/overview?days=7"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let report: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let timeline = report["activity"]["timeline"].as_array().unwrap();
        assert_eq!(timeline.len(), 7);
        assert_eq!(
            timeline
                .iter()
                .map(|day| day["views"].as_i64().unwrap())
                .sum::<i64>(),
            2
        );
        assert_eq!(report["commerce"]["users"]["registered"], 1);
        assert_eq!(report["commerce"]["users"]["active"], 1);
        assert_eq!(report["commerce"]["orders"], 0);
        assert_eq!(report["activity"]["systems"][0]["key"], "Android");
        assert_eq!(report["activity"]["regions"][0]["key"], "unknown");
    }

    #[tokio::test]
    async fn admin_access_uses_a_separate_session() {
        let db = PortalDb::open(":memory:").await.unwrap();
        let state = Arc::new(AppState {
            db,
            env_name: "test".into(),
            license_secret: "unit-test-secret".into(),
            admin_secret: Some("unit-test-admin-secret".into()),
            static_dir: None,
            pay_config_dir: None,
        });
        let password =
            studio_api::hash_password(&state.license_secret, "lilyco42ovo", "fixturepass");
        state
            .db
            .studio_register("lilyco42ovo", &password, "admin@example.test")
            .await
            .unwrap();
        let app = build_router(state);
        let login = app
            .clone()
            .oneshot(
                Request::post("/studio/api/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"username":"lilyco42ovo","password":"fixturepass"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&login.into_body().collect().await.unwrap().to_bytes()).unwrap();
        let authorization = format!("Bearer {}", body["token"].as_str().unwrap());
        let privileged = body["admin_token"].as_str().unwrap();
        for (session, expected) in [("", StatusCode::FORBIDDEN), (privileged, StatusCode::OK)] {
            let response = app
                .clone()
                .oneshot(
                    Request::get("/api/admin/overview?days=1")
                        .header("authorization", &authorization)
                        .header("x-admin-session", session)
                        .header("x-admin-secret", "unit-test-secret")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
        }
    }

    #[tokio::test]
    async fn studio_billing_register_afdian_alipay_purchase_and_redeem_e2e() {
        let app = test_app().await;

        // 1. Register user via /studio/api/register
        let reg = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/studio/api/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"username":"lain_tester","password":"password123","email":"t@lain42.top"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reg.status(), StatusCode::OK);
        let reg_j: serde_json::Value =
            serde_json::from_slice(&reg.into_body().collect().await.unwrap().to_bytes()).unwrap();
        let token = reg_j["token"].as_str().unwrap().to_string();

        // 2. Create Afdian intent & verify automatic_checkout_url + check settlement
        let af = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/studio/api/payments/afdian/intents")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"product":"points_cny_30","reference":"ref1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(af.status(), StatusCode::OK);
        let af_j: serde_json::Value =
            serde_json::from_slice(&af.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(af_j["automatic_binding"], true);
        let intent_id = af_j["intent_id"].as_str().unwrap().to_string();

        let chk = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/studio/api/payments/afdian/intents/{intent_id}/check"
                    ))
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(chk.status(), StatusCode::OK);

        // 3. Purchase proxy_1m_220g (300 points, 220GB) using credited 300 paid points
        let buy = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/studio/api/points/purchase")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"product":"proxy_1m_220g","reference":"ref2"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(buy.status(), StatusCode::OK);

        // 4. Redeem LAIN42-TEST-500 card (free promo bonus points) -> verify it CANNOT buy proxy_1m (150 pts, non-replicable)
        //    but CAN buy rembg_creator (replicable software tool)
        let rdm = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/studio/api/redeem")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"code":"LAIN42-TEST-500"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rdm.status(), StatusCode::OK);

        let buy_proxy_with_bonus = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/studio/api/points/purchase")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"product":"proxy_1m","reference":"ref_deny_bonus"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(buy_proxy_with_bonus.status(), StatusCode::BAD_REQUEST);

        let buy_tool_with_bonus = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/studio/api/points/purchase")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"product":"rembg_creator","reference":"ref_allow_bonus"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(buy_tool_with_bonus.status(), StatusCode::OK);

        let me = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/studio/api/me")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let me_j: serde_json::Value =
            serde_json::from_slice(&me.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(me_j["username"], "lain_tester");
        assert_eq!(me_j["points"]["paid_balance"], 0);
        assert_eq!(me_j["points"]["bonus_balance"], 458);
        assert_eq!(me_j["points"]["balance"], 458);
        assert_eq!(me_j["entitlements"].as_array().unwrap().len(), 2);
        let lkey = me_j["entitlements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["product"] == "proxy_1m_220g")
            .unwrap()["license_key"]
            .as_str()
            .unwrap()
            .to_string();

        // 5. Verify Clash subscription delivery, client receipt verify, and SEO endpoints
        let sub = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/studio/api/sub/{lkey}/clash"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(sub.status(), StatusCode::OK);
        let userinfo = sub
            .headers()
            .get("subscription-userinfo")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            userinfo.contains("total=236223201280"),
            "220GB plan must advertise 236223201280 bytes in Subscription-Userinfo, got {userinfo}"
        );
        let sub_yaml =
            String::from_utf8(sub.into_body().collect().await.unwrap().to_bytes().to_vec())
                .unwrap();
        assert!(sub_yaml.contains("LYCO-Auto"));
        assert!(sub_yaml.contains("🇦🇺 澳大利亚悉尼 · VLESS 专线"));
        assert!(sub_yaml.contains("🇰🇷 韩国二 · VLESS 中转"));

        let ver = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/studio/api/verify")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"license_key":"{lkey}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ver.status(), StatusCode::OK);

        let rob = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/robots.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rob.status(), StatusCode::OK);

        let sm = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/sitemap.xml")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(sm.status(), StatusCode::OK);
        let sm_body =
            String::from_utf8(sm.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
        assert!(sm_body.contains("/downloads/clash-verge-rev"));

        let dls = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/downloads")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(dls.status(), StatusCode::OK);

        let dl_detail = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/downloads/clash-verge-rev")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(dl_detail.status(), StatusCode::OK);

        let go_dl = app
            .oneshot(
                Request::builder()
                    .uri("/go/download/clash-verge-rev")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(go_dl.status(), StatusCode::TEMPORARY_REDIRECT);
    }

    #[tokio::test]
    async fn full_portal_pages_orders_credits_and_mcp_e2e() {
        let app = test_app().await;

        // 1. All 6 SSR pages render HTTP 200 with expected content
        for (path, needle) in [
            ("/", "把想法做成能直接使用的产品"),
            ("/models", "wb-cn:glm-5.3"),
            ("/shop", "商城与全球积分收银台"),
            ("/account", "我的账户控制台"),
            ("/docs", "ANTHROPIC_BASE_URL"),
            ("/tool", "SHA-256"),
            ("/vpn", "LYCO Connect"),
        ] {
            let resp = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "path={path}");
            let body = String::from_utf8(
                resp.into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .to_vec(),
            )
            .unwrap();
            assert!(body.contains(needle), "missing `{needle}` in {path}");
        }

        // 2. Create credits order -> confirm payment -> verify credits added & lkr_ key issued
        let order_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/orders")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"product_id":"credits-100","buyer_email":"alice@lain42.top"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(order_resp.status(), StatusCode::CREATED);
        let order_json: serde_json::Value =
            serde_json::from_slice(&order_resp.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let order_id = order_json["order"]["id"].as_str().unwrap().to_string();

        let confirm_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/orders/{order_id}/confirm"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(confirm_resp.status(), StatusCode::OK);
        let confirm_json: serde_json::Value =
            serde_json::from_slice(&confirm_resp.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(confirm_json["order"]["status"], "paid");
        // Initial 10 welcome credits + 100 purchased = 110
        assert_eq!(confirm_json["account"]["credits"], 110);
        assert!(confirm_json["account"]["api_key"]
            .as_str()
            .unwrap()
            .starts_with("lkr_"));

        // 3. Publish announcement via API and verify persistence
        let ann_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/announcements")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"published_date":"2026-11-20","title":"全 Rust 重写上线","summary":"Turso 实测通过","pinned":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ann_resp.status(), StatusCode::CREATED);

        // 4. Verify health & MCP tool call
        let health_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let health: serde_json::Value =
            serde_json::from_slice(&health_resp.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(health["db"]["orders_count"], 1);
        assert_eq!(health["db"]["accounts_count"], 1);
        assert_eq!(health["db"]["announcements_count"], 3);

        let mcp_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_models"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(mcp_resp.status(), StatusCode::OK);
        let mcp_body = String::from_utf8(
            mcp_resp
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(mcp_body.contains("wb-cn:deepseek-v3.2"));
    }
}
