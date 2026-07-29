//! Lilyco Ultra UI 示例
//!
//! 启动 Ultra UI 服务器，在浏览器中打开 Excel 风格的 JSON 编辑器。
//!
//! ```text
//! cargo run -p lilyco-ultra-ui-example
//! ```
//!
//! 然后在浏览器中编辑 JSON，实时预览生成的 React UI。

use lilyco_ultra_ui::UltraUiServer;

#[tokio::main]
async fn main() {
    let port = 9090;
    eprintln!("┌─────────────────────────────────────────────┐");
    eprintln!("│  Lilyco Ultra UI — Excel JSON → React       │");
    eprintln!("│  浏览器: http://localhost:{port:<5}             │", port = port);
    eprintln!("│  Ctrl+C 退出                                │");
    eprintln!("└─────────────────────────────────────────────┘");
    UltraUiServer::new(port).serve().await;
}
