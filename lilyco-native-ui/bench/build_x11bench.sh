#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$HERE/bin"
gcc -O2 -o "$HERE/bin/x11bench" "$HERE/x11bench.c" -lX11 -lXtst
echo "X11BENCH $HERE/bin/x11bench"
