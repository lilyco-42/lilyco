//! Ultra 可视化编排一键启动：`cargo run -p lilyco-ultra-ui --example serve -- 8090`

#[tokio::main]
async fn main() {
    let port = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(8090);
    lilyco_ultra_ui::UltraUiServer::new(port).serve().await;
}
