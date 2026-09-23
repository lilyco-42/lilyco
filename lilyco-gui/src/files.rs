//! 把文件交给命令的两条路（都以「回填一个绝对路径」收场）：
//!
//! - `POST /upload` 浏览器拖拽 → base64 → 服务端临时目录副本 → 回填副本路径（受体积上限）
//! - `POST /pick`   本机原生对话框 → 回填**磁盘上的原始路径**（不复制、不设上限）
//!
//! 浏览器出于安全永远不给真实路径，所以第二条只能由服务端弹框；四端共用的 handler
//! 收的正是 `path`，两条路对命令侧完全同形。
//!
//! 安全：`/upload` 净化文件名（拒路径穿越）、粗筛 + 实筛双重体积上限；`/pick` 与
//! `/run` 同一道令牌闸（见 `security`）。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::util::generate_id;

/// 上传大小上限（base64 编码前）
const MAX_UPLOAD_BYTES: usize = 200 * 1024 * 1024;
/// 体积粗筛看的 base64 字符数上限（base64 膨胀系数 4/3，再加一点余量）
const MAX_B64_CHARS: usize = MAX_UPLOAD_BYTES / 3 * 4 + 1024;
/// `/upload` 的**传输层**上限：必须比 [`MAX_B64_CHARS`] 宽，否则超限的请求会在
/// axum 默认那道 2 MB 闸上被直接掐断，浏览器只报「Failed to fetch」，页面里那句
/// 「file too large (max 200MB)」永远显示不出来（用户实测截图就是前者）。
/// 多出来的 1 MiB 给 JSON 外壳（`name` 字段 + 括号）。
pub(crate) const MAX_JSON_BODY: usize = MAX_B64_CHARS + (1 << 20);
// 两条线一旦倒过来，超限上传就只剩一句「Failed to fetch」——编译期就把它钉住。
const _: () = assert!(
    MAX_JSON_BODY > MAX_B64_CHARS,
    "传输层上限必须比 handler 的体积粗筛更宽"
);

/// 对话框标题：既是给用户看的，也是本进程找回那个窗口的查找键
#[cfg(feature = "pick")]
const PICKER_TITLE: &str = "选择文件";

/// 手写 base64 解码（标准字母表，容忍空白与缺省 padding）——零新增依赖
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const INVALID: u8 = 0xFF;
    fn val(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => INVALID,
        }
    }
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3 + 3);
    let mut acc: u32 = 0;
    let mut nbits: u32 = 0;
    for &b in &bytes {
        let v = val(b);
        if v == INVALID {
            return None;
        }
        acc = (acc << 6) | v as u32;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((acc >> nbits) as u8);
        }
    }
    // 残余 bits 全 0 才合法（非 0 说明编码被截断/损坏）
    if nbits >= 6 && (acc & ((1 << nbits) - 1)) != 0 {
        return None;
    }
    Some(out)
}

/// 文件名净化：保留字母/数字（含中文）与 `._-`，其余替换为 '_'；拒绝路径穿越
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .take(120)
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.replace("..", "_");
    if cleaned.is_empty() || cleaned == "." {
        String::new()
    } else {
        cleaned
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct UploadRequest {
    /// 原始文件名（仅用于生成暂存文件名，会被净化）
    name: String,
    /// 文件内容（dataURL 去前缀后的 base64）。
    /// 页面发的是 camelCase `dataB64`，两种写法都得收：只认一种时另一种会整条请求 400，
    /// 而报出来的错是「missing field data_b64」，看的人根本想不到是键名对不上。
    #[serde(alias = "dataB64")]
    data_b64: String,
}

/// 文件暂存：浏览器端读文件 → base64 → 写入服务端临时目录 → 回填绝对路径。
/// 服务端与浏览器同机（回环限定），命令读到的即用户选的文件。
pub(crate) async fn upload_handler(Json(req): Json<UploadRequest>) -> Response {
    let name = sanitize_filename(&req.name);
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "invalid file name").into_response();
    }
    // base64 膨胀系数 4/3：编码长度粗筛即可挡住超大 body
    if req.data_b64.len() > MAX_B64_CHARS {
        return (StatusCode::PAYLOAD_TOO_LARGE, "file too large (max 200MB)").into_response();
    }
    let Some(bytes) = base64_decode(&req.data_b64) else {
        return (StatusCode::BAD_REQUEST, "invalid base64").into_response();
    };
    if bytes.len() > MAX_UPLOAD_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "file too large (max 200MB)").into_response();
    }

    let dir = std::env::temp_dir().join("lilyco-uploads");
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("create upload dir: {e}"),
        )
            .into_response();
    }
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let fname = format!("{}_{}.{}", ms, &generate_id()[..8], name);
    let path = dir.join(&fname);
    if let Err(e) = tokio::fs::write(&path, &bytes).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("write upload: {e}"),
        )
            .into_response();
    }

    Json(serde_json::json!({
        "path": path.to_string_lossy(),
        "name": req.name,
        "size": bytes.len(),
    }))
    .into_response()
}

/// 提醒线程的句柄；非 Windows 版本给 None，调用点两边写法一致
#[cfg(feature = "pick")]
struct Raiser(Option<std::thread::JoinHandle<()>>);

#[cfg(feature = "pick")]
impl Raiser {
    fn join(self) {
        if let Some(handle) = self.0 {
            let _ = handle.join();
        }
    }
}

/// Windows 的前台锁定不让后台进程抢焦点，所以不硬抢：按标题轮询到那个顶层窗口后
/// `SetForegroundWindow` 试一次 + `FlashWindowEx` 让任务栏闪。页面提示是主力，闪烁是加成。
#[cfg(all(windows, feature = "pick"))]
fn flash_picker_when_ready(title: &'static str) -> Raiser {
    Raiser(Some(std::thread::spawn(move || {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, FlashWindowEx, SetForegroundWindow, FLASHWINFO, FLASHW_ALL,
            FLASHW_TIMERNOFG,
        };
        let needle: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        // 窗口是另一个线程慢慢建出来的，所以要等一会；建好后闪一次就收工
        for _ in 0..50 {
            // windows-sys 里 PCWSTR 就是裸的 *const u16，结尾 0 由 needle 自己带上
            let hwnd = unsafe { FindWindowW(std::ptr::null(), needle.as_ptr()) };
            if !hwnd.is_null() {
                unsafe {
                    SetForegroundWindow(hwnd);
                    let info = FLASHWINFO {
                        cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                        hwnd,
                        dwFlags: FLASHW_ALL | FLASHW_TIMERNOFG,
                        uCount: 5,
                        dwTimeout: 0,
                    };
                    FlashWindowEx(&info);
                }
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
    })))
}

/// 非 Windows：没有可闪的任务栏格子，页面那条提示仍然管用
#[cfg(all(feature = "pick", not(windows)))]
fn flash_picker_when_ready(_title: &'static str) -> Raiser {
    Raiser(None)
}

/// 让本机进程弹一次系统文件选择框，把**磁盘上的原始路径**回填到页面输入框。
///
/// 与 `/upload` 的分工：拖拽上传走浏览器（拿到的是服务端暂存副本的路径，受 200 MB 上限），
/// 这里走本机对话框（拿到文件本来的路径，不复制、不设体积上限）。浏览器出于安全永远不给
/// 真实路径，所以这个框只能由服务端弹；而四端共用的 handler 收的正是 `path`。
pub(crate) async fn pick_handler() -> Response {
    #[cfg(feature = "pick")]
    {
        // 对话框是阻塞调用：放 blocking 线程里等，弹着的时候页面其他请求照常跑
        let picked = tokio::task::spawn_blocking(|| {
            let raiser = flash_picker_when_ready(PICKER_TITLE);
            let chosen = rfd::FileDialog::new().set_title(PICKER_TITLE).pick_file();
            raiser.join();
            chosen
        })
        .await;
        match picked {
            // 取消不是错误：path 给 null，页面留着原值不动
            Ok(Some(path)) => Json(serde_json::json!({ "path": path.display().to_string() })),
            Ok(None) => Json(serde_json::json!({ "path": serde_json::Value::Null })),
            Err(error) => Json(serde_json::json!({
                "path": serde_json::Value::Null,
                "error": error.to_string(),
            })),
        }
        .into_response()
    }
    #[cfg(not(feature = "pick"))]
    {
        (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "path": serde_json::Value::Null,
                "error": "这个构建没开 pick 特性：请手填路径，或把文件拖进虚线框上传",
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 页面发 camelCase、结构体字段是 snake_case：两种拼写都得收。
    /// 只认一种时拖拽上传会整条 400，而报出来的错是「missing field data_b64」，
    /// 完全看不出是键名对不上（这个坑是实跑截图抓到的）。
    #[test]
    fn upload_request_accepts_both_key_spellings() {
        let camel: UploadRequest =
            serde_json::from_str(r#"{"name":"a.bin","dataB64":"AAA"}"#).expect("camelCase 要能收");
        assert_eq!(camel.data_b64, "AAA");
        let snake: UploadRequest = serde_json::from_str(r#"{"name":"a.bin","data_b64":"BBB"}"#)
            .expect("snake_case 要能收");
        assert_eq!(snake.data_b64, "BBB");
    }

    #[test]
    fn base64_known_vectors() {
        // RFC 4648 已知向量
        assert_eq!(base64_decode("aGVsbG8="), Some(b"hello".to_vec()));
        assert_eq!(base64_decode("aGVsbG8"), Some(b"hello".to_vec())); // 无 padding
        assert_eq!(base64_decode("5Lit5paH"), Some("中文".as_bytes().to_vec()));
        assert_eq!(base64_decode("!!!"), None);
        assert_eq!(base64_decode(""), Some(Vec::new()));
    }

    #[test]
    fn sanitize_filename_blocks_traversal() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "____etc_passwd");
        assert_eq!(sanitize_filename("报告 最终.pdf"), "报告_最终.pdf");
        assert_eq!(sanitize_filename(""), "");
        assert_eq!(sanitize_filename(".."), "_");
    }
}
