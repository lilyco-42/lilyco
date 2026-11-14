#!/usr/bin/env sh
# 把指定仓库的 lilyco.app.json 链接进用户清单目录。
# 用法: ./install-manifests.sh /path/to/lycomc /path/to/lycodl
set -eu

target="${XDG_CONFIG_HOME:-$HOME/.config}/lilyco/apps"
mkdir -p "$target"

for repo in "$@"; do
  manifest="$repo/lilyco.app.json"
  if [ ! -f "$manifest" ]; then
    echo "skip: $repo (no lilyco.app.json)"
    continue
  fi
  id=$(sed -n 's/.*"id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)
  if [ -z "$id" ]; then
    echo "skip: $repo (cannot read id)"
    continue
  fi
  ln -sfn "$manifest" "$target/$id.json"
  echo "linked: $id -> $manifest"
done

echo "manifest dir: $target"
