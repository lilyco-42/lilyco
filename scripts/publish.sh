#!/usr/bin/env bash
# Lilyco crates.io 发布脚本 —— 按依赖顺序全链发布
#
# 前置（一次性）：
#   cargo login --registry crates-io <token>     # token: https://crates.io/settings/tokens
#
# 为什么带 --registry crates-io：
#   本机 ~/.cargo/config.toml 用 rsproxy.cn 镜像替换了 crates-io，
#   cargo publish 拒绝走非远程 registry；显式指定真实源即可。
#
# 用法：bash scripts/publish.sh
# 顺序：core → macros → cli → tui → gui → mcp → facade（facade 依赖其余全部）
set -euo pipefail

ORDER=(lilyco-core lilyco-macros lilyco-cli lilyco-tui lilyco-gui lilyco-mcp lilyco)

for crate in "${ORDER[@]}"; do
  echo "==> publishing $crate"
  cargo publish -p "$crate" --registry crates-io
  # sparse 索引刷新有延迟：等新版本对下游可见，避免依赖解析失败
  sleep 20
done

echo "==> 全部发布完成，验证："
for crate in "${ORDER[@]}"; do
  curl -s "https://crates.io/api/v1/crates/$crate" | python -c "import json,sys; d=json.load(sys.stdin); print(f\"  {d['crate']['name']} {d['crate']['max_version']}\")"
done
