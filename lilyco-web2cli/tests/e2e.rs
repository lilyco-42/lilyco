use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

use lilyco::__core::context::Context;
use lilyco::__core::safety::Interactive;
use lilyco_web2cli::{
    discover_url, distill_response, DraftStatus, DraftStore, SessionManager, VaultStore,
};
use serde_json::{json, Map, Value};

fn spawn_mock_site() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().unwrap();
    let base = format!("http://127.0.0.1:{}", addr.port());

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut s = stream;
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            let mut header_end = None;
            let mut content_len = 0usize;
            loop {
                let n = s.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                if header_end.is_none() {
                    if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(pos + 4);
                        let hdrs = String::from_utf8_lossy(&raw[..pos]);
                        for line in hdrs.lines() {
                            if let Some((k, v)) = line.split_once(':') {
                                if k.trim().eq_ignore_ascii_case("content-length") {
                                    content_len = v.trim().parse().unwrap_or(0);
                                }
                            }
                        }
                    }
                }
                if let Some(he) = header_end {
                    if raw.len() >= he + content_len {
                        break;
                    }
                }
            }
            if raw.is_empty() {
                continue;
            }
            let req = String::from_utf8_lossy(&raw).to_string();
            let first_line = req.lines().next().unwrap_or("");
            let mut parts = first_line.split_whitespace();
            let method = parts.next().unwrap_or("GET");
            let path = parts.next().unwrap_or("/");

            let (status, ct, extra_hdr, body) = if method == "GET" && path == "/" {
                let html = r#"<!doctype html>
<html>
  <head><title>Mock Portal</title></head>
  <body>
    <h1>Mock Portal</h1>
    <p>Welcome to the mock portal used for web2cli end-to-end testing.</p>
    <a href="/docs">Documentation</a>
    <form id="search" action="/search" method="get">
      <label for="q">Search Query</label>
      <input id="q" type="text" name="q" required>
      <select name="sort">
        <option value="relevance" selected>Relevance</option>
        <option value="date">Date</option>
      </select>
      <input type="hidden" name="fmt" value="json">
    </form>
    <form id="login" action="/login" method="post">
      <input type="text" name="user" required>
    </form>
    <table>
      <tr><th>Package</th><th>Version</th></tr>
      <tr><td>lilyco</td><td>0.3.0</td></tr>
    </table>
    <script>fetch('/api/v1/items/{id}');</script>
  </body>
</html>"#;
                ("200 OK", "text/html; charset=utf-8", "", html.to_string())
            } else if method == "GET" && path.starts_with("/search") {
                let resp_json = json!({
                    "query_path": path,
                    "results": [{"title": "Lilyco Web2CLI", "score": 100}]
                })
                .to_string();
                ("200 OK", "application/json", "", resp_json)
            } else if method == "POST" && path == "/login" {
                let body_part = req.split("\r\n\r\n").nth(1).unwrap_or("");
                let resp_json = json!({
                    "logged_in": true,
                    "form_body": body_part
                })
                .to_string();
                (
                    "200 OK",
                    "application/json",
                    "Set-Cookie: sid=abc123; Path=/\r\n",
                    resp_json,
                )
            } else if method == "GET" && path == "/api/v1/items/42" {
                let has_cookie = req.to_ascii_lowercase().contains("sid=abc123");
                let resp_json = json!({
                    "id": 42,
                    "cookie_present": has_cookie
                })
                .to_string();
                ("200 OK", "application/json", "", resp_json)
            } else {
                ("404 Not Found", "text/plain", "", "not found".to_string())
            };

            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n{extra_hdr}\r\n{body}",
                body.len()
            );
            let _ = s.write_all(response.as_bytes());
        }
    });

    base
}

#[test]
fn end_to_end_discover_compile_execute_and_persist_cookies() {
    let base = spawn_mock_site();
    let dir = tempfile::tempdir().unwrap();
    let session = Arc::new(SessionManager::with_dir("e2e", dir.path().to_path_buf()));

    // 1. Discover any webpage -> WebCliSpec
    let spec = discover_url(&session, &base, false).expect("discover mock site");
    assert!(spec.commands.iter().any(|c| c.name == "search"));
    assert!(spec.commands.iter().any(|c| c.name == "login"));
    assert!(spec.commands.iter().any(|c| c.name == "tables"));
    assert!(spec
        .commands
        .iter()
        .any(|c| c.name.contains("api-v1-items")));

    // 2. Compile WebCliSpec -> Lilyco Registry
    let reg = spec
        .to_registry(Arc::clone(&session), Some(Arc::new(Interactive)))
        .expect("compile registry");

    let (tx, _rx) = std::sync::mpsc::channel();
    let ctx = Context::new_test(tx);

    // 3. Execute GET form command (`search`)
    let search_cmd = reg.get("search").expect("search cmd");
    search_cmd
        .schema
        .validate_args(&json!({"q": "rust", "sort": "date"}))
        .expect("validate search args");
    let search_out =
        (search_cmd.handler.as_ref().unwrap())(&ctx, &json!({"q": "rust", "sort": "date"}))
            .expect("run search");
    let q_path = search_out["data"]["query_path"].as_str().unwrap();
    assert!(q_path.contains("q=rust"));
    assert!(q_path.contains("sort=date"));
    assert!(q_path.contains("fmt=json"));

    // 4. Execute POST form command (`login`) -> sets cookie `sid=abc123`
    let login_cmd = reg.get("login").expect("login cmd");
    let login_out =
        (login_cmd.handler.as_ref().unwrap())(&ctx, &json!({"user": "alice"})).expect("run login");
    assert_eq!(login_out["data"]["logged_in"], true);
    assert_eq!(login_out["data"]["form_body"], "user=alice");

    // 5. Execute discovered API endpoint with path param `{id}` + verify cookie persisted!
    let api_cmd = reg
        .iter()
        .find(|c| c.name.contains("api-v1-items"))
        .expect("api cmd");
    let api_out = (api_cmd.handler.as_ref().unwrap())(&ctx, &json!({"id": "42"})).expect("run api");
    assert_eq!(api_out["data"]["id"], 42);
    assert_eq!(api_out["data"]["cookie_present"], true);

    // 6. Execute `tables` command
    let tables_cmd = reg.get("tables").expect("tables cmd");
    let tables_out = (tables_cmd.handler.as_ref().unwrap())(&ctx, &json!({})).expect("run tables");
    assert_eq!(tables_out["tables"][0]["rows"][0][0], "lilyco");

    // 7. Distill check
    let resp = session
        .request("GET", &base, &Default::default(), None, false)
        .unwrap();
    let distilled = distill_response(&resp.final_url, &resp.content_type, &resp.body, 4000);
    assert_eq!(distilled.title, "Mock Portal");
    assert!(!distilled.actions.is_empty());
}

fn spawn_mock_gov_portal() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind gov mock server");
    let addr = listener.local_addr().unwrap();
    let base = format!("http://127.0.0.1:{}", addr.port());

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut s = stream;
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            let mut header_end = None;
            let mut content_len = 0usize;
            loop {
                let n = s.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                if header_end.is_none() {
                    if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(pos + 4);
                        let hdrs = String::from_utf8_lossy(&raw[..pos]);
                        for line in hdrs.lines() {
                            if let Some((k, v)) = line.split_once(':') {
                                if k.trim().eq_ignore_ascii_case("content-length") {
                                    content_len = v.trim().parse().unwrap_or(0);
                                }
                            }
                        }
                    }
                }
                if let Some(he) = header_end {
                    if raw.len() >= he + content_len {
                        break;
                    }
                }
            }
            if raw.is_empty() {
                continue;
            }
            let req = String::from_utf8_lossy(&raw).to_string();
            let first_line = req.lines().next().unwrap_or("");
            let mut parts = first_line.split_whitespace();
            let method = parts.next().unwrap_or("GET");
            let path = parts.next().unwrap_or("/");

            let (status, ct, body) = if method == "GET" && path == "/register" {
                let html = r#"<!doctype html>
<html>
  <head><title>企业设立登记一网通办</title></head>
  <body>
    <form id="enterprise_apply" action="/api/apply" method="post" enctype="multipart/form-data">
      <label for="ent">企业名称</label>
      <input id="ent" type="text" name="entName" required>
      <label for="rep">法定代表人</label>
      <input id="rep" type="text" name="lerep" required>
      <label for="lic">营业执照</label>
      <input id="lic" type="file" name="licenseFile" required>
      <label for="memo">设立备注</label>
      <input id="memo" type="text" name="applyMemo" required>
      <input type="hidden" name="bizType" value="NEW_COMPANY">
    </form>
  </body>
</html>"#;
                ("200 OK", "text/html; charset=utf-8", html.to_string())
            } else if method == "POST" && path == "/api/apply" {
                let has_auth_cookie = req.to_ascii_lowercase().contains("gov_token=real_sso_888");
                let is_multipart = req
                    .to_ascii_lowercase()
                    .contains("content-type: multipart/form-data");
                let has_company = req.contains("星火科技有限公司");
                let has_pdf = req.contains("%PDF-1.4 MOCK_LICENSE");
                let has_biz_type = req.contains("NEW_COMPANY");
                let resp_json = json!({
                    "receipt_no": "GOV-2026-0001",
                    "auth_ok": has_auth_cookie,
                    "multipart_ok": is_multipart,
                    "company_ok": has_company,
                    "pdf_uploaded": has_pdf,
                    "hidden_field_ok": has_biz_type
                })
                .to_string();
                ("200 OK", "application/json", resp_json)
            } else {
                ("404 Not Found", "text/plain", "not found".to_string())
            };

            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(response.as_bytes());
        }
    });

    base
}

#[test]
fn gov_enterprise_registration_workflow_with_cookies_vault_and_human_review() {
    let base = spawn_mock_gov_portal();
    let dir = tempfile::tempdir().unwrap();

    // 1. 导入浏览器 Cookies（模拟政务网扫码登录后导出的 JSON cookies）
    let session = SessionManager::with_dir("gov", dir.path().join("sessions"));
    let cookie_json = r#"[{"domain": "127.0.0.1", "name": "gov_token", "value": "real_sso_888"}]"#;
    let imported = session.import_cookies_from_str(cookie_json, None).unwrap();
    assert_eq!(imported, 1);

    // 2. 分类资料库管理（企业信息 + 法人信息 + 营业执照附件）
    let vault = VaultStore::with_dir(dir.path().join("vault"));
    let pdf_path = dir.path().join("license.pdf");
    std::fs::write(&pdf_path, b"%PDF-1.4 MOCK_LICENSE").unwrap();
    vault
        .set_field("xinghuo", "company", "name", "星火科技有限公司")
        .unwrap();
    vault
        .set_field("xinghuo", "legal_rep", "name", "张三")
        .unwrap();
    vault
        .add_attachment("xinghuo", "证照", "business_license", "营业执照", &pdf_path)
        .unwrap();
    let profile = vault.load("xinghuo").unwrap();

    // 3. AI 自动发现表单并生成 PendingReview 草稿（不发包）
    let spec = discover_url(&session, &format!("{base}/register"), false).unwrap();
    let cmd = spec
        .commands
        .iter()
        .find(|c| c.name == "enterprise-apply")
        .unwrap()
        .clone();
    let drafts = DraftStore::with_dir(dir.path().join("drafts"));
    let draft = drafts
        .prepare_draft(&profile, "gov", &spec.base_url, cmd, &Map::new())
        .unwrap();
    assert_eq!(draft.status, DraftStatus::PendingReview);
    assert_eq!(draft.args["entname"], "星火科技有限公司");
    assert_eq!(draft.args["lerep"], "张三");
    assert_eq!(draft.missing_required.len(), 1); // applymemo 尚未填写

    // 4. 安全闸门：未经人类批准，严禁直接 commit
    let (tx, _rx) = std::sync::mpsc::channel();
    let ctx = Context::new_test(tx);
    assert!(drafts.commit_draft(&draft.id, &ctx, &session).is_err());

    // 5. 人类核对、补齐缺失字段并批准
    let patched = drafts
        .patch_arg(
            &draft.id,
            "applymemo",
            Value::String("新设有限责任公司".into()),
        )
        .unwrap();
    assert!(patched.missing_required.is_empty());
    let approved = drafts
        .approve(&draft.id, Some("法人已核对全部信息与执照附件".into()))
        .unwrap();
    assert_eq!(approved.status, DraftStatus::Approved);

    // 6. 最终提交（带 multipart/form-data 营业执照 PDF + 导入的政务登录 Cookie）并留存回执
    let submitted = drafts.commit_draft(&draft.id, &ctx, &session).unwrap();
    assert_eq!(submitted.status, DraftStatus::Submitted);
    let receipt = submitted.receipt.as_ref().unwrap();
    assert_eq!(receipt["data"]["receipt_no"], "GOV-2026-0001");
    assert_eq!(receipt["data"]["auth_ok"], true);
    assert_eq!(receipt["data"]["multipart_ok"], true);
    assert_eq!(receipt["data"]["company_ok"], true);
    assert_eq!(receipt["data"]["pdf_uploaded"], true);
    assert_eq!(receipt["data"]["hidden_field_ok"], true);
}
