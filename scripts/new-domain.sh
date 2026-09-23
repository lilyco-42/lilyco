#!/usr/bin/env bash
# 从 scripts/domain-template/ 生成一个新域 crate（「一域一二进制 × 四端」的骨架）。
#
# 用法：bash scripts/new-domain.sh <域名>        # 例：bash scripts/new-domain.sh archive
# 结果：新建 lilyco-<域名>/（二进制名 l<域名>），并把该 crate 挂进根 Cargo.toml 的 members。
#
# 只生成不删：已存在同名 crate 时直接退出，不覆盖任何手写代码。
set -euo pipefail

DOMAIN="${1:-}"
if [[ ! "$DOMAIN" =~ ^[a-z][a-z0-9]*$ ]]; then
  echo "用法：bash scripts/new-domain.sh <域名>（小写字母开头，仅小写字母与数字），如 archive" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE="lilyco-$DOMAIN"
BIN="l$DOMAIN"
SRC="$ROOT/scripts/domain-template"
DST="$ROOT/$CRATE"

if [ -e "$DST" ]; then
  echo "$CRATE 已存在，不动它" >&2
  exit 1
fi

mkdir -p "$DST/src"
cp "$SRC/Cargo.toml" "$DST/Cargo.toml"
for f in "$SRC"/src/*.rs; do
  cp "$f" "$DST/src/"
done

# 代换模板标记 @CRATE@ / @BIN@
sed -i.bak "s|@CRATE@|$CRATE|g; s|@BIN@|$BIN|g" "$DST/Cargo.toml" "$DST"/src/*.rs
rm -f "$DST/Cargo.toml.bak" "$DST"/src/*.bak

# 挂进 workspace members（幂等）
if ! grep -q "\"$CRATE\"," "$ROOT/Cargo.toml"; then
  awk -v c="\"$CRATE\"," '
    BEGIN { done = 0 }
    {
      print
      if (!done && /^members = \[$/) { print "    " c; done = 1 }
    }
  ' "$ROOT/Cargo.toml" > "$ROOT/Cargo.toml.new"
  mv "$ROOT/Cargo.toml.new" "$ROOT/Cargo.toml"
fi

echo "已生成 $CRATE（二进制名 $BIN），并挂进 workspace members"
cat <<EOF

下一步见 docs/INTEGRATION.md §2（3→12 步）：
  1. src/show.rs 换成你自己的命令；src/main.rs 的 registry 装配与两条断言跟着改
  2. .github/workflows/ci.yml 的 apps job 两处枚举都要加 -p $CRATE（clippy + test）
     —— 漏了等于该 crate 在 CI 上不存在，这一条真栽过一次
  3. docs/$DOMAIN.md + readme.md 小节 + docs/CODEGRAPH.md（§1 版本 / §3 符号 / §9 测试地图）
  4. cp scripts/acceptance/binfmt_probe.py scripts/acceptance/${DOMAIN}_probe.py：四端逐字比对
  5. cargo fmt -p $CRATE && cargo clippy -p $CRATE --all-targets && cargo test -p $CRATE
EOF
