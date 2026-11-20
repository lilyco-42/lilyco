#![cfg(feature = "server")]

use std::io::Cursor;

use image::{ImageFormat, Rgb, RgbImage};
use lilyco_web2cli::{
    build_router, current_unix_secs, issue_license, EnterpriseLicense, GatewayConfig, GatewayState,
    PLUGIN_PROOFREAD, PLUGIN_VISION,
};
use serde_json::{json, Value};
use ureq::Agent;

#[test]
fn gateway_e2e_natural_language_image_crop_payment_gate_and_proofread() {
    let dir = tempfile::tempdir().unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://127.0.0.1:{}", addr.port());

    let cfg = GatewayConfig {
        bind_addr: addr.to_string(),
        public_base_url: base_url.clone(),
        data_dir: dir.path().to_path_buf(),
        require_payment: true,
        price_fen: 199,
        pay_url_template: None,
        require_license: false,
        license_secret: "test-secret".into(),
    };
    let state = GatewayState::new(cfg);
    // Seed a profile with 1 valid USCC + 1 invalid ID card for proofreading test
    let vault = state.vault_store();
    vault
        .set_field("default", "company", "credit_code", "91350100M000100Y43")
        .unwrap();
    vault
        .set_field("default", "legal_rep", "id_card", "110105194912310021")
        .unwrap();

    let router = build_router(state);
    rt.spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let agent = Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent();

    // 1. Create a real 600x600 PNG in memory and upload via multipart
    let img = RgbImage::from_pixel(600, 600, Rgb([40, 120, 220]));
    let mut png_buf = Cursor::new(Vec::new());
    img.write_to(&mut png_buf, ImageFormat::Png).unwrap();
    let png_bytes = png_buf.into_inner();

    let boundary = "----TestUploadBoundary123";
    let mut mp = Vec::new();
    mp.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    mp.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"portrait.png\"\r\nContent-Type: image/png\r\n\r\n",
    );
    mp.extend_from_slice(&png_bytes);
    mp.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let mut up_resp = agent
        .post(&format!("{base_url}/api/upload"))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .send(mp.as_slice())
        .unwrap();
    assert_eq!(up_resp.status().as_u16(), 200);
    let up_json: Value =
        serde_json::from_str(&up_resp.body_mut().read_to_string().unwrap()).unwrap();
    let upload_id = up_json["upload_id"].as_str().unwrap().to_string();

    // 2. Natural-language chat: crop to 1-inch ID photo (295x413)
    let mut chat_resp = agent
        .post(&format!("{base_url}/api/chat"))
        .header("Content-Type", "application/json")
        .send(
            json!({
                "message": "帮我裁剪成一寸照 295x413 并压缩给我下载链接",
                "upload_id": upload_id
            })
            .to_string(),
        )
        .unwrap();
    assert_eq!(chat_resp.status().as_u16(), 200);
    let chat_json: Value =
        serde_json::from_str(&chat_resp.body_mut().read_to_string().unwrap()).unwrap();
    let dl_url = chat_json["download_url"].as_str().unwrap();
    let pay_url = chat_json["pay_url"].as_str().unwrap();
    let order_id = chat_json["order_id"].as_str().unwrap();
    assert!(pay_url.contains(order_id));

    // 3. Before payment -> 402 Payment Required
    let blocked = agent.get(dl_url).call().unwrap();
    assert_eq!(blocked.status().as_u16(), 402);

    // 4. Confirm payment -> unlocks download
    let mut confirm_resp = agent
        .post(&format!("{base_url}/api/orders/{order_id}/confirm"))
        .send("")
        .unwrap();
    assert_eq!(confirm_resp.status().as_u16(), 200);
    let confirm_json: Value =
        serde_json::from_str(&confirm_resp.body_mut().read_to_string().unwrap()).unwrap();
    assert_eq!(confirm_json["paid"], true);

    // 5. Download real processed JPEG & verify 295x413 dimensions
    let mut dl_resp = agent.get(dl_url).call().unwrap();
    assert_eq!(dl_resp.status().as_u16(), 200);
    let dl_bytes = dl_resp.body_mut().read_to_vec().unwrap();
    let decoded = image::load_from_memory(&dl_bytes).unwrap();
    assert_eq!(decoded.width(), 295);
    assert_eq!(decoded.height(), 413);

    // 6. Natural-language proofreading catches invalid GB 11643-1999 ID card
    let mut pr_resp = agent
        .post(&format!("{base_url}/api/chat"))
        .header("Content-Type", "application/json")
        .send(json!({ "message": "帮我校对企业资料" }).to_string())
        .unwrap();
    assert_eq!(pr_resp.status().as_u16(), 200);
    let pr_json: Value =
        serde_json::from_str(&pr_resp.body_mut().read_to_string().unwrap()).unwrap();
    assert_eq!(pr_json["intent"], "proofread");
    assert_eq!(pr_json["report"]["passed"], false);
    assert_eq!(pr_json["report"]["issues"].as_array().unwrap().len(), 1);
}

#[test]
fn gateway_enforces_enterprise_plugin_licenses() {
    let dir = tempfile::tempdir().unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://127.0.0.1:{}", addr.port());
    let secret = "enterprise-hmac-secret-2026";

    let cfg = GatewayConfig {
        bind_addr: addr.to_string(),
        public_base_url: base_url.clone(),
        data_dir: dir.path().to_path_buf(),
        require_payment: false,
        price_fen: 199,
        pay_url_template: None,
        require_license: true,
        license_secret: secret.to_string(),
    };
    let router = build_router(GatewayState::new(cfg));
    rt.spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let agent = Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent();

    // 1. No license -> 402 Payment Required for `proofread` plugin
    let blocked = agent
        .post(&format!("{base_url}/api/chat"))
        .header("Content-Type", "application/json")
        .send(json!({ "message": "校对企业资料" }).to_string())
        .unwrap();
    assert_eq!(blocked.status().as_u16(), 402);

    // 2. License with only `vision` still blocks `proofread`
    let now = current_unix_secs();
    let vision_only = issue_license(
        secret,
        &EnterpriseLicense {
            tenant_id: "ent-a".into(),
            company_name: "甲公司".into(),
            enabled_plugins: vec![PLUGIN_VISION.into()],
            issued_unix: now,
            expires_unix: now + 3600,
        },
    )
    .unwrap();
    let still_blocked = agent
        .post(&format!("{base_url}/api/chat"))
        .header("Content-Type", "application/json")
        .header("X-Lain-License", &vision_only)
        .send(json!({ "message": "校对企业资料" }).to_string())
        .unwrap();
    assert_eq!(still_blocked.status().as_u16(), 402);

    // 3. License with `proofread` succeeds (200 OK)
    let with_proofread = issue_license(
        secret,
        &EnterpriseLicense {
            tenant_id: "ent-b".into(),
            company_name: "乙公司".into(),
            enabled_plugins: vec![PLUGIN_PROOFREAD.into()],
            issued_unix: now,
            expires_unix: now + 3600,
        },
    )
    .unwrap();
    let allowed = agent
        .post(&format!("{base_url}/api/chat"))
        .header("Content-Type", "application/json")
        .header("X-Lain-License", &with_proofread)
        .send(json!({ "message": "校对企业资料" }).to_string())
        .unwrap();
    assert_eq!(allowed.status().as_u16(), 200);

    // 4. `/api/plugins` reports accurate unlock flags for the license
    let mut plugins_resp = agent
        .get(&format!("{base_url}/api/plugins"))
        .header("X-Lain-License", &with_proofread)
        .call()
        .unwrap();
    assert_eq!(plugins_resp.status().as_u16(), 200);
    let pj: Value =
        serde_json::from_str(&plugins_resp.body_mut().read_to_string().unwrap()).unwrap();
    let plugins = pj["plugins"].as_array().unwrap();
    assert_eq!(
        plugins.iter().find(|p| p["id"] == "proofread").unwrap()["unlocked"],
        true
    );
    assert_eq!(
        plugins.iter().find(|p| p["id"] == "vision").unwrap()["unlocked"],
        false
    );

    // 5. Admin license issue API (requires X-Admin-Secret)
    let unauth_issue = agent
        .post(&format!("{base_url}/api/admin/license/issue"))
        .header("Content-Type", "application/json")
        .send(
            json!({
                "tenant_id": "ent-c",
                "company_name": "丙公司",
                "enabled_plugins": ["*"],
                "days": 90
            })
            .to_string(),
        )
        .unwrap();
    assert_eq!(unauth_issue.status().as_u16(), 401);

    let mut auth_issue = agent
        .post(&format!("{base_url}/api/admin/license/issue"))
        .header("Content-Type", "application/json")
        .header("X-Admin-Secret", secret)
        .send(
            json!({
                "tenant_id": "ent-c",
                "company_name": "丙公司",
                "enabled_plugins": ["*"],
                "days": 90
            })
            .to_string(),
        )
        .unwrap();
    assert_eq!(auth_issue.status().as_u16(), 200);
    let issued: Value =
        serde_json::from_str(&auth_issue.body_mut().read_to_string().unwrap()).unwrap();
    let issued_token = issued["token"].as_str().unwrap();

    let mut verify_resp = agent
        .post(&format!("{base_url}/api/license/verify"))
        .header("Content-Type", "application/json")
        .send(json!({ "token": issued_token }).to_string())
        .unwrap();
    assert_eq!(verify_resp.status().as_u16(), 200);
    let vj: Value =
        serde_json::from_str(&verify_resp.body_mut().read_to_string().unwrap()).unwrap();
    assert_eq!(vj["license"]["company_name"], "丙公司");

    // 6. Multi-theme API & 10-year UI Protocol capabilities
    let mut themes_resp = agent.get(&format!("{base_url}/api/themes")).call().unwrap();
    assert_eq!(themes_resp.status().as_u16(), 200);
    let tj: Value =
        serde_json::from_str(&themes_resp.body_mut().read_to_string().unwrap()).unwrap();
    assert_eq!(tj["themes"].as_array().unwrap().len(), 10);

    let mut css_resp = agent
        .get(&format!("{base_url}/api/themes/ant_design.css"))
        .call()
        .unwrap();
    assert_eq!(css_resp.status().as_u16(), 200);
    let css_text = css_resp.body_mut().read_to_string().unwrap();
    assert!(css_text.contains("--primary: #1677ff;"));

    let mut caps_resp = agent
        .get(&format!("{base_url}/api/capabilities"))
        .call()
        .unwrap();
    assert_eq!(caps_resp.status().as_u16(), 200);
    let cj: Value = serde_json::from_str(&caps_resp.body_mut().read_to_string().unwrap()).unwrap();
    assert_eq!(cj["capabilities"]["protocol_version"], "1.0");

    // 7. AI-Native Web endpoints: /llms.txt, /.well-known/agent.json, /openapi.json, POST /mcp
    let mut llms = agent.get(&format!("{base_url}/llms.txt")).call().unwrap();
    assert_eq!(llms.status().as_u16(), 200);
    assert!(llms.body_mut().read_to_string().unwrap().contains("POST"));

    let mut card = agent
        .get(&format!("{base_url}/.well-known/agent.json"))
        .call()
        .unwrap();
    assert_eq!(card.status().as_u16(), 200);
    let card_j: Value = serde_json::from_str(&card.body_mut().read_to_string().unwrap()).unwrap();
    assert!(card_j["endpoints"]["mcp_http"]
        .as_str()
        .unwrap()
        .ends_with("/mcp"));

    let mut oa = agent
        .get(&format!("{base_url}/openapi.json"))
        .call()
        .unwrap();
    assert_eq!(oa.status().as_u16(), 200);
    let oa_j: Value = serde_json::from_str(&oa.body_mut().read_to_string().unwrap()).unwrap();
    assert_eq!(oa_j["openapi"], "3.1.0");

    let mut mcp_init = agent
        .post(&format!("{base_url}/mcp"))
        .header("Content-Type", "application/json")
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}).to_string())
        .unwrap();
    assert_eq!(mcp_init.status().as_u16(), 200);
    let mj: Value = serde_json::from_str(&mcp_init.body_mut().read_to_string().unwrap()).unwrap();
    assert_eq!(mj["result"]["protocolVersion"], "2024-11-05");

    let mut mcp_call = agent
        .post(&format!("{base_url}/mcp"))
        .header("Content-Type", "application/json")
        .header("X-Lain-License", &with_proofread)
        .send(
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": { "name": "proofread_profile", "arguments": { "profile": "default" } }
            })
            .to_string(),
        )
        .unwrap();
    assert_eq!(mcp_call.status().as_u16(), 200);
    let mcj: Value = serde_json::from_str(&mcp_call.body_mut().read_to_string().unwrap()).unwrap();
    assert!(mcj["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("passed"));
}
