#!/usr/bin/env bash
# Notify Bing / Yandex / Naver immediately via IndexNow protocol after deploying new release pages.
set -euo pipefail
HOST="${1:-dev.lain42.top}"
KEY="lain42indexnow2026key"
URLS=$(curl -fsSL "https://${HOST}/sitemap.xml" | grep -oE 'https://[^<]+' | jq -R . | jq -s .)
PAYLOAD=$(jq -n --arg host "$HOST" --arg key "$KEY" --arg keyLoc "https://${HOST}/lain42-indexnow-key.txt" --argjson urlList "$URLS" \
  '{host:$host, key:$key, keyLocation:$keyLoc, urlList:$urlList}')
echo "Submitting $(echo "$URLS" | jq length) URLs to IndexNow for ${HOST}..."
curl -sS -i -X POST "https://api.indexnow.org/indexnow" \
  -H "Content-Type: application/json; charset=utf-8" \
  -d "$PAYLOAD" | head -n 15
