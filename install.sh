#!/usr/bin/env bash
# lilyco 一键安装：下载 lbrush / lvision 二进制并接入 DSH（dsh-mcp-client stdio）。
#
# 用法:
#   bash install.sh                     # 默认 profile: web；安装到 ~/.lilyco/bin
#   LILYCO_PROFILE=foo bash install.sh  # 指定 DSH profile
#   bash install.sh --dry-run           # 只打印将执行的动作，不改任何文件
#
# 幂等：重复运行不会重复下载/追加条目。完成后重启 dsh web 生效。
set -euo pipefail

REPO="lilyco-42/lilyco"
BIN_DIR="${LILYCO_HOME:-$HOME/.lilyco}/bin"
PROFILE="${LILYCO_PROFILE:-}"
DRY_RUN=0
[ "${1:-}" = "--dry-run" ] && DRY_RUN=1

say()  { printf '\033[1;32m[lilyco]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[lilyco]\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31m[lilyco]\033[0m %s\n' "$*" >&2; exit 1; }
run()  { say "+ $*"; [ "$DRY_RUN" = 1 ] || "$@"; }

# 转成 Windows 正斜杠路径（YAML 友好）；非 Windows 平台原样返回
winpath() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -m "$1"
  else
    printf '%s' "$1"
  fi
}

# 平台 → 资产名
detect_platform() {
  local os arch
  os=$(uname -s 2>/dev/null || echo unknown)
  arch=$(uname -m 2>/dev/null || echo unknown)
  case "$os" in
    MINGW* | MSYS* | CYGWIN*)
      echo "windows" ;;
    Linux)
      if [ -n "${TERMUX_VERSION:-}" ] || [ "${PREFIX:-}" = "/data/data/com.termux/files/usr" ]; then
        case "$arch" in
          aarch64 | arm64) echo "android-arm64" ;;
          *) die "Termux 仅支持 aarch64（当前 $arch）" ;;
        esac
      else
        echo "linux-desktop"
      fi ;;
    *) echo "unsupported" ;;
  esac
}

# 从 latest release 取指定资产的下载 URL（免 jq，仅 grep/sed）
asset_url() { # $1 = 资产名子串
  curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
    | grep -o "\"browser_download_url\": *\"[^\"]*${1}[^\"]*\"" \
    | head -n1 | sed 's/.*"browser_download_url": *"//; s/"$//'
}

print_path_hint() {
  say "二进制目录: $BIN_DIR"
  say "如需命令行使用，把 $BIN_DIR 加入 PATH"
}

main() {
  local plat bin1 bin2
  plat=$(detect_platform)
  case "$plat" in
    windows)       bin1="lbrush-windows.exe";    bin2="lvision-windows.exe" ;;
    android-arm64) bin1="lbrush-android-arm64";  bin2="lvision-android-arm64" ;;
    linux-desktop)
      warn "暂无 Linux 桌面预编译二进制；请源码构建：cargo build --release -p lilyco-brush -p lilyco-vision"
      exit 0 ;;
    *) die "不支持的平台: $(uname -s)/$(uname -m)" ;;
  esac

  say "平台: $plat → 资产: $bin1, $bin2"
  mkdir -p "$BIN_DIR"

  for b in "$bin1" "$bin2"; do
    local url
    url=$(asset_url "$b")
    if [ -z "$url" ]; then
      die "找不到资产 $b（检查 https://github.com/$REPO/releases/latest）"
    fi
    say "下载 $b"
    if [ "$DRY_RUN" = 0 ]; then
      # 临时名下载 + rename 覆盖：绕开 Windows 实时扫描对刚下载 exe 的瞬时写锁
      local part="$BIN_DIR/.$b.part"
      local ok=0
      for attempt in 1 2 3; do
        if curl -fL --retry 2 -sS -o "$part" "$url" && mv -f "$part" "$BIN_DIR/$b"; then
          ok=1
          break
        fi
        warn "写入被锁（Windows 实时扫描常见），重试 $attempt/3…"
        sleep 2
      done
      [ "$ok" = 1 ] || die "下载失败: $b"
      chmod +x "$BIN_DIR/$b" 2>/dev/null || true
    fi
  done

  # ── DSH 接入 ──
  if ! command -v dsh >/dev/null 2>&1; then
    warn "未找到 dsh 命令，跳过 DSH 配置；二进制已装到 $BIN_DIR"
    print_path_hint
    return 0
  fi

  if [ -z "$PROFILE" ]; then
    if [ -f "$HOME/.dsh/profiles/web/cordis.patch.yml" ]; then
      PROFILE="web"
    else
      PROFILE=$(basename "$(ls -d "$HOME"/.dsh/profiles/*/ 2>/dev/null | head -n1)")
      PROFILE="${PROFILE:-web}"
    fi
  fi
  local patch="$HOME/.dsh/profiles/$PROFILE/cordis.patch.yml"
  [ -f "$patch" ] || die "profile $PROFILE 的 patch 不存在: $patch"
  say "DSH profile: $PROFILE"

  # 1) 确保 dsh-mcp-client 插件已安装（幂等）
  if grep -q '"@deepseek-ai/dsh-mcp-client"' "$HOME/.dsh/profiles/$PROFILE/package.json" 2>/dev/null; then
    say "dsh-mcp-client 已安装，跳过"
  else
    say "安装 @deepseek-ai/dsh-mcp-client（dsh plugin add，等价 pnpm add）"
    run dsh plugin --profile "$PROFILE" add '@deepseek-ai/dsh-mcp-client'
  fi

  # 2) patch 追加 mcp 条目（按 id 去重，幂等）
  append_entry() { # $1=id  $2=serverName  $3=二进制绝对路径  $4=toolCallTimeoutMs
    local id="$1" abs
    if grep -q "id: $id" "$patch"; then
      say "$id 已在 patch 中，跳过"
      return
    fi
    abs=$(winpath "$3")
    say "追加 $id → $abs"
    if [ "$DRY_RUN" = 0 ]; then
      cat >>"$patch" <<EOF

# lilyco $id（install.sh 自动生成）
- insert:
    - id: $id
      name: '@deepseek-ai/dsh-mcp-client'
      config:
        serverName: $2
        transport: stdio
        command: $abs
        args: ['--mcp']
        toolCallTimeoutMs: $4
EOF
    fi
  }

  append_entry mcp-lbrush lbrush "$BIN_DIR/$bin1" 180000
  append_entry mcp-lvision lvision "$BIN_DIR/$bin2" 120000

  say "完成！重启 dsh web 后模型将看到 mcp__lbrush__Brush + mcp__lvision__*（8 个视觉工具）"
  print_path_hint
}

main "$@"
