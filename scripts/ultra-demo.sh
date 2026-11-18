#!/usr/bin/env bash
# Ultra 可视化编排一键启动
#
# 用法:
#   bash scripts/ultra-demo.sh              # 默认 8090
#   LILYCO_PORT=8190 bash scripts/ultra-demo.sh
#
# 打开后：
#   http://127.0.0.1:8090/designer   Axure 式原型设计器
#   http://127.0.0.1:8090/blocks     Scratch 式积木编程
#   http://127.0.0.1:8090/graph      节点图（LiteGraph）
set -euo pipefail

cd "$(dirname "$0")/.."
PORT="${LILYCO_PORT:-8090}"

echo "[ultra] 端口 $PORT"
echo "[ultra] designer  http://127.0.0.1:$PORT/designer"
echo "[ultra] blocks    http://127.0.0.1:$PORT/blocks"
echo "[ultra] graph     http://127.0.0.1:$PORT/graph"

exec cargo run -q -p lilyco-ultra-ui --example serve -- "$PORT"