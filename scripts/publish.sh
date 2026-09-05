#!/usr/bin/env bash
# Lilyco crates.io 发布脚本 —— 按依赖顺序全链发布
#
# 前置（一次性）：cargo login（token: https://crates.io/settings/tokens）
#
# 为什么用临时 CARGO_HOME：
#   本机 cargo 配置了 rsproxy.cn 镜像替换 crates-io，镜像同步有滞后 ——
#   发布 gui/mcp 等依赖刚发布的 core 时，验证构建会解析到旧版而编译失败。
#   临时 CARGO_HOME 只带 credentials、不带镜像配置 → 验证走 crates.io 真实索引。
#
# 用法：bash scripts/publish.sh
# 顺序：core → macros → cli → tui → gui → mcp → facade（facade 依赖其余全部）
set -euo pipefail

ORDER=(lilyco-core lilyco-macros lilyco-cli lilyco-tui lilyco-gui lilyco-mcp lilyco)

# 干净的 cargo home：仅 credentials，无镜像替换
WORK_HOME="${TMPDIR:-/tmp}/lilyco-publish-home"
mkdir -p "$WORK_HOME"
if [ -f "${CARGO_HOME:-$HOME/.cargo}/credentials.toml" ]; then
  cp "${CARGO_HOME:-$HOME/.cargo}/credentials.toml" "$WORK_HOME/"
fi

for crate in "${ORDER[@]}"; do
  echo "==> publishing $crate"
  CARGO_HOME="$WORK_HOME" cargo publish -p "$crate"
  # sparse 索引 CDN 传播有延迟：等新版本对下游可见，避免依赖解析到旧版
  sleep 25
done

echo "==> 全部发布完成，验证（crates.io API）："
for crate in "${ORDER[@]}"; do
  curl -s -A "lilyco-release" "https://crates.io/api/v1/crates/$crate" |
    python -c "import json,sys; d=json.load(sys.stdin); print(f\"  {d['crate']['name']} {d['crate']['max_version']}\")"
done
