use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use lilyco_web2cli::{
    find_browser, run_wizard_cdp, CdpClient, ManagedChrome, SessionManager, VaultStore, WizardPlan,
    WizardStatus, WizardStep,
};

fn spawn_iframe_gov_spa() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind spa mock");
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut s = stream;
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            if n == 0 {
                continue;
            }
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("/");

            let (status, ct, extra, body) = if path == "/" {
                let html = r#"<!doctype html>
<html>
  <head><meta charset="utf-8"><title>政务多步填报 SPA</title></head>
  <body>
    <h1>企业设立登记</h1>
    <label for="cname">企业名称</label>
    <input id="cname" name="company_name" type="text">
    <iframe src="/inner-frame" width="400" height="120"></iframe>
    <button id="submit_btn" onclick="document.getElementById('status').innerText = 'SUBMITTED:' + document.getElementById('cname').value">最终提交</button>
    <div id="status">DRAFT</div>
  </body>
</html>"#;
                (
                    "200 OK",
                    "text/html; charset=utf-8",
                    "Set-Cookie: gov_cdp_sid=cdp999; Path=/\r\n",
                    html,
                )
            } else if path == "/inner-frame" {
                let html = r#"<!doctype html>
<html>
  <head><meta charset="utf-8"></head>
  <body>
    <label for="rep">法定代表人</label>
    <input id="rep" name="legal_rep" type="text">
  </body>
</html>"#;
                ("200 OK", "text/html; charset=utf-8", "", html)
            } else {
                ("404 Not Found", "text/plain", "", "not found")
            };

            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });

    base
}

#[test]
fn live_cdp_iframe_snapshot_wizard_screenshot_gate_and_cookie_sync() {
    if find_browser().is_none() {
        eprintln!("Skipping live CDP test: no local Chrome/Chromium found");
        return;
    }

    let base = spawn_iframe_gov_spa();
    let dir = tempfile::tempdir().unwrap();

    // Prepare categorized vault profile
    let vaults = VaultStore::with_dir(dir.path().join("vault"));
    vaults
        .set_field("xinghuo", "company", "name", "星火智能科技有限公司")
        .unwrap();
    vaults
        .set_field("xinghuo", "legal_rep", "name", "李四")
        .unwrap();
    let profile = vaults.load("xinghuo").unwrap();

    // Launch real local Chromium with CDP
    let chrome = ManagedChrome::launch().expect("launch managed chromium");
    let mut client = CdpClient::connect_http(&chrome.cdp_http, None).expect("connect CDP");

    // 1. Navigate & snapshot across main frame + nested iframe
    client.navigate(&base, 300).expect("navigate");
    let snap = client.snapshot_all_frames().expect("snapshot all frames");
    assert!(snap.frame_count >= 2, "expected main frame + nested iframe");
    let company_ref = snap
        .elements
        .iter()
        .find(|e| e.name == "company_name")
        .expect("company_name in main frame")
        .ref_id
        .clone();
    let rep_ref = snap
        .elements
        .iter()
        .find(|e| e.name == "legal_rep")
        .expect("legal_rep inside nested iframe")
        .ref_id
        .clone();
    let submit_ref = snap
        .elements
        .iter()
        .find(|e| e.tag == "button")
        .expect("submit button")
        .ref_id
        .clone();

    // 2. Run multi-step WizardPlan up to HumanReviewGate
    let shot_path = dir.path().join("review-gate.png");
    let plan = WizardPlan {
        name: "gov-spa-apply".into(),
        about: "Enterprise registration multi-frame wizard".into(),
        steps: vec![
            WizardStep::Fill {
                ref_id: format!("@{company_ref}"),
                value: "@vault:company.name".into(),
            },
            WizardStep::Fill {
                ref_id: format!("@{rep_ref}"),
                value: "@vault:legal_rep.name".into(),
            },
            WizardStep::HumanReviewGate {
                message: "请人工核对截图中的企业名称与 iframe 内法定代表人信息".into(),
                screenshot_out: Some(shot_path.display().to_string()),
            },
            WizardStep::Click {
                ref_id: format!("@{submit_ref}"),
            },
        ],
    };

    let paused = run_wizard_cdp(&mut client, &plan, &profile, 0, false).expect("run until gate");
    assert_eq!(paused.status, WizardStatus::PausedForHumanReview);
    assert!(shot_path.is_file());
    let png_bytes = std::fs::read(&shot_path).unwrap();
    assert!(
        png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "valid PNG screenshot"
    );

    // Verify submit has NOT happened yet while paused at HumanReviewGate!
    let status_before = client
        .eval_js("document.getElementById('status').innerText")
        .unwrap();
    assert_eq!(status_before.as_str(), Some("DRAFT"));

    // 3. Human approves -> resume from next_step_index
    let done = run_wizard_cdp(&mut client, &plan, &profile, paused.next_step_index, true)
        .expect("resume after approval");
    assert_eq!(done.status, WizardStatus::Completed);

    let status_after = client
        .eval_js("document.getElementById('status').innerText")
        .unwrap();
    assert_eq!(
        status_after.as_str(),
        Some("SUBMITTED:星火智能科技有限公司")
    );

    // 4. Sync logged-in cookies from Chrome CDP into SessionManager
    let sess = SessionManager::with_dir("cdp-gov", dir.path().join("sessions"));
    let synced = client.sync_cookies_to_session(&sess).expect("sync cookies");
    assert!(synced >= 1);
    let snap_sess = sess.state_snapshot();
    assert_eq!(snap_sess.cookies["127.0.0.1"]["gov_cdp_sid"], "cdp999");
}

#[test]
fn live_cdp_verification_handoff_batch_fill_dropdown_and_file_upload() {
    if find_browser().is_none() {
        eprintln!("Skipping live CDP test: no local Chrome/Chromium found");
        return;
    }

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind verification mock");
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut s = stream;
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            if n == 0 {
                continue;
            }
            let html = r#"<!doctype html>
<html>
  <head><meta charset="utf-8"><title>企业登记安全验证页</title></head>
  <body>
    <div id="verify_banner">安全验证：请先完成短信验证码核验</div>
    <input id="f_name" name="ent_name" type="text">
    <input id="f_code" name="uscc" type="text">
    <button id="el_trigger" type="button" onclick="document.getElementById('el_menu').style.display='block'">选择企业类型</button>
    <ul id="el_menu" style="display:none">
      <li class="el-select-dropdown__item" onclick="document.getElementById('chosen_type').innerText='有限责任公司'">有限责任公司</li>
      <li class="el-select-dropdown__item" onclick="document.getElementById('chosen_type').innerText='股份有限公司'">股份有限公司</li>
    </ul>
    <div id="chosen_type">未选</div>
    <input id="f_pdf" name="license_pdf" type="file">
  </body>
</html>"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
                html.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let pdf = dir.path().join("biz-license.pdf");
    std::fs::write(&pdf, b"%PDF-1.4 TEST").unwrap();

    let chrome = ManagedChrome::launch().expect("launch chromium");
    let mut client = CdpClient::connect_http(&chrome.cdp_http, None).expect("connect cdp");
    client.navigate(&base, 250).expect("navigate");

    let snap1 = client.snapshot_all_frames().expect("snap1");
    let name_ref = snap1
        .elements
        .iter()
        .find(|e| e.name == "ent_name")
        .unwrap()
        .ref_id
        .clone();
    let code_ref = snap1
        .elements
        .iter()
        .find(|e| e.name == "uscc")
        .unwrap()
        .ref_id
        .clone();
    let trigger_ref = snap1
        .elements
        .iter()
        .find(|e| e.tag == "button")
        .unwrap()
        .ref_id
        .clone();
    let file_ref = snap1
        .elements
        .iter()
        .find(|e| e.input_type == "file")
        .unwrap()
        .ref_id
        .clone();

    // 1. Wizard MUST pause cleanly when verification prompt is present (never bypass!)
    let plan = WizardPlan {
        name: "verify-handoff".into(),
        about: "Verify human handoff".into(),
        steps: vec![WizardStep::Fill {
            ref_id: format!("@{name_ref}"),
            value: "星火科技".into(),
        }],
    };
    let empty_profile = lilyco_web2cli::VaultProfile::default();
    let paused = run_wizard_cdp(&mut client, &plan, &empty_profile, 0, false).unwrap();
    assert_eq!(paused.status, WizardStatus::PausedForHumanVerification);
    assert_eq!(paused.executed_steps, 0);

    // 2. Simulate human completing verification in the visible browser window
    client
        .eval_js(
            "document.getElementById('verify_banner').remove(); document.title = '企业登记表';",
        )
        .unwrap();
    let resumed = run_wizard_cdp(
        &mut client,
        &plan,
        &empty_profile,
        paused.next_step_index,
        false,
    )
    .unwrap();
    assert_eq!(resumed.status, WizardStatus::Completed);

    // 3. Test fill_batch + ElementUI custom dropdown + DOM.setFileInputFiles + delta_snapshot
    let filled = client
        .fill_batch(&[
            (name_ref.clone(), "星火智能有限公司".into()),
            (code_ref.clone(), "91350100M000100Y43".into()),
        ])
        .unwrap();
    assert_eq!(filled, 2);

    assert!(client
        .select_custom_dropdown(&trigger_ref, "有限责任公司")
        .unwrap());
    let chosen = client
        .eval_js("document.getElementById('chosen_type').innerText")
        .unwrap();
    assert_eq!(chosen.as_str(), Some("有限责任公司"));

    assert!(client
        .set_file_input_files(&file_ref, &[pdf.display().to_string()])
        .unwrap());
    let uploaded_name = client
        .eval_js("document.getElementById('f_pdf').files[0].name")
        .unwrap();
    assert_eq!(uploaded_name.as_str(), Some("biz-license.pdf"));

    let snap2 = client.snapshot_all_frames().unwrap();
    let delta = CdpClient::delta_snapshot(&snap1, &snap2);
    assert!(
        delta
            .iter()
            .any(|e| e.name == "ent_name" && e.value == "星火智能有限公司"),
        "delta snapshot should capture updated form value"
    );
}
