#!/usr/bin/env python3
"""
High-Star GitHub Release Discovery & Quark Drive Mirror Pipeline (400+ projects).

Categories (target >= 100 qualified projects with real Quark Drive share links per category):
  - game  (游戏: 启动器/模拟器/开源游戏/串流/MOD工具)
  - video (影视: 播放器/视频下载/剪辑转码/字幕/媒体服务器)
  - music (音乐: 播放器/音源/DAW编曲/音频处理/标签歌词)
  - ebook (电子书: 阅读器/漫画阅读器/PDF工具/书库管理/排版写作)

Features:
  - Resumable discovery cache (`data/discovered_400.json`)
  - Incremental checkpointing to `data/downloads.json` (preserves existing `self` & `oss` entries)
  - Concurrent `gh api` discovery + controlled concurrent `curl` -> SHA-256 -> `kuake upload` -> `kuake share 0 false`
  - Auto-retries on transient network/Quark errors
"""

from __future__ import annotations

import concurrent.futures
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Any

DATA_DIR = Path(__file__).resolve().parent.parent / "data"
DISCOVERED_PATH = DATA_DIR / "discovered_400.json"
DOWNLOADS_PATH = DATA_DIR / "downloads.json"

TARGET_PER_CAT = 100
CANDIDATE_BUFFER = 150

CATEGORY_QUERIES: dict[str, list[str]] = {
    "game": [
        "topic:game-launcher stars:>300 archived:false",
        "topic:emulator stars:>500 archived:false",
        "topic:minecraft-launcher stars:>200 archived:false",
        "topic:game-engine stars:>1000 archived:false",
        "topic:retro-gaming stars:>300 archived:false",
        "topic:steam stars:>500 archived:false",
        "topic:game-modding stars:>300 archived:false",
        "topic:open-source-game stars:>500 archived:false",
        "topic:minecraft stars:>1000 archived:false",
        "topic:gaming stars:>800 archived:false",
        "game launcher OR emulator OR moonlight OR playnite stars:>500 archived:false",
        "switch emulator OR ps2 emulator OR gba emulator OR nintendo emulator stars:>300 archived:false",
    ],
    "video": [
        "topic:video-player stars:>400 archived:false",
        "topic:video-downloader stars:>500 archived:false",
        "topic:video-editor stars:>400 archived:false",
        "topic:media-server stars:>500 archived:false",
        "topic:subtitle stars:>400 archived:false",
        "topic:bilibili stars:>500 archived:false",
        "topic:anime stars:>600 archived:false",
        "topic:iptv stars:>500 archived:false",
        "topic:screen-recorder stars:>400 archived:false",
        "topic:streaming stars:>800 archived:false",
        "topic:ffmpeg stars:>800 archived:false",
        "video downloader OR m3u8 OR subtitle OR media player stars:>500 archived:false",
    ],
    "music": [
        "topic:music-player stars:>400 archived:false",
        "topic:audio-player stars:>300 archived:false",
        "topic:audio-editor stars:>300 archived:false",
        "topic:synthesizer stars:>400 archived:false",
        "topic:daw stars:>300 archived:false",
        "topic:spotify stars:>600 archived:false",
        "topic:netease-cloud-music stars:>200 archived:false",
        "topic:lyrics stars:>300 archived:false",
        "topic:subsonic stars:>200 archived:false",
        "topic:music-downloader stars:>300 archived:false",
        "topic:audio stars:>1000 archived:false",
        "music player OR audio converter OR vocal remover OR equalizer stars:>400 archived:false",
    ],
    "ebook": [
        "topic:ebook-reader stars:>200 archived:false",
        "topic:ebook stars:>400 archived:false",
        "topic:epub stars:>300 archived:false",
        "topic:manga stars:>400 archived:false",
        "topic:manga-reader stars:>200 archived:false",
        "topic:pdf-reader stars:>200 archived:false",
        "topic:pdf-editor stars:>200 archived:false",
        "topic:pdf stars:>1000 archived:false",
        "topic:kindle stars:>300 archived:false",
        "topic:calibre stars:>200 archived:false",
        "topic:comic stars:>300 archived:false",
        "topic:note-taking stars:>1500 archived:false",
        "ebook OR epub OR comic reader OR novel reader OR pdf viewer stars:>400 archived:false",
    ],
}

CTA_BY_CAT = {
    "game": "proxy",
    "video": "proxy",
    "music": "music",
    "ebook": "ai",
}

CAT_LABEL_ZH = {
    "game": "游戏工具与开源游戏",
    "video": "影视播放与视频工具",
    "music": "音乐播放与音频创作",
    "ebook": "电子书漫画与 PDF 阅读",
}

ASSET_EXT_RE = re.compile(
    r"\.(exe|msi|zip|7z|apk|dmg|AppImage|deb|tar\.gz|tar\.xz|jar)$", re.IGNORECASE
)
BAD_ASSET_RE = re.compile(
    r"(\.sig|\.sha256|\.sha512|\.asc|\.blockmap|\.yml|\.yaml|\.pdb|\.dSYM|debug|symbols|source|src\.|armhf|riscv|ppc64|s390x|freebsd|openbsd)",
    re.IGNORECASE,
)

PREFER_PATTERNS = [
    re.compile(r"(win|windows).*(x64|x86_64|amd64|64).*\.(exe|msi|zip|7z)$", re.I),
    re.compile(r"(x64|x86_64|amd64).*(setup|installer|portable)?\.(exe|msi)$", re.I),
    re.compile(r"\.(exe|msi)$", re.I),
    re.compile(r"(universal|arm64-v8a|arm64).*\.apk$", re.I),
    re.compile(r"\.apk$", re.I),
    re.compile(r"(win|windows).*\.(zip|7z)$", re.I),
    re.compile(r"(linux|x86_64|amd64).*\.(AppImage|deb|tar\.gz|zip)$", re.I),
    re.compile(r"\.(zip|7z|jar|tar\.gz|dmg|AppImage)$", re.I),
]

ENV = os.environ.copy()
ENV["HOME"] = "/home/lilyco"

LOCK = threading.Lock()
DIR_CACHE: set[str] = set()


def run_json(cmd: list[str], timeout: int = 120) -> dict[str, Any]:
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, env=ENV, timeout=timeout)
    except subprocess.TimeoutExpired:
        return {"success": False, "error": "timeout"}
    out = p.stdout.strip()
    if not out:
        return {"success": False, "error": p.stderr.strip()[:300], "rc": p.returncode}
    try:
        return json.loads(out)
    except Exception:
        return {"success": False, "raw": out[:300], "error": p.stderr.strip()[:200], "rc": p.returncode}


def slugify(repo_full: str) -> str:
    name = repo_full.split("/")[-1].lower()
    s = re.sub(r"[^a-z0-9]+", "-", name).strip("-")
    return s or "tool"


def pick_best_asset(assets: list[dict[str, Any]], max_mb: float = 180.0) -> dict[str, Any] | None:
    valid = []
    for a in assets:
        name = a.get("name") or ""
        size = int(a.get("size") or 0)
        if size < 10_000 or size > int(max_mb * 1_048_576):
            continue
        if not ASSET_EXT_RE.search(name):
            continue
        if BAD_ASSET_RE.search(name):
            continue
        valid.append(a)
    if not valid:
        return None
    for pat in PREFER_PATTERNS:
        for a in valid:
            if pat.search(a["name"]):
                return a
    valid.sort(key=lambda x: x.get("size", 0))
    return valid[0]


def infer_platform(asset_name: str) -> str:
    low = asset_name.lower()
    if low.endswith((".exe", ".msi")) or "win" in low:
        return "Windows"
    if low.endswith(".apk") or "android" in low:
        return "Android"
    if low.endswith(".dmg") or "mac" in low or "darwin" in low:
        return "macOS"
    if low.endswith((".appimage", ".deb")) or "linux" in low:
        return "Linux"
    if low.endswith(".jar"):
        return "Windows / macOS / Linux (Java)"
    return "Windows / macOS / Linux"


def discover_category(cat: str, seen_repos: set[str], seen_slugs: set[str]) -> list[dict[str, Any]]:
    print(f"\n=== Discovering [{cat}] (target {CANDIDATE_BUFFER}) ===", flush=True)
    raw_candidates: list[dict[str, Any]] = []
    local_seen: set[str] = set()

    for q in CATEGORY_QUERIES[cat]:
        if len(raw_candidates) >= 260:
            break
        res = run_json(
            ["gh", "api", "-X", "GET", "search/repositories", "-f", f"q={q}", "-f", "sort=stars", "-f", "order=desc", "-f", "per_page=50"],
            timeout=30,
        )
        items = res.get("items") or []
        for it in items:
            full = it.get("full_name") or ""
            if not full or full in seen_repos or full in local_seen:
                continue
            if it.get("archived") or it.get("disabled"):
                continue
            local_seen.add(full)
            raw_candidates.append({
                "repo": full,
                "stars": int(it.get("stargazers_count") or 0),
                "description": (it.get("description") or "").strip(),
                "topics": it.get("topics") or [],
            })
        time.sleep(0.25)

    # Sort all discovered candidates by stars descending
    raw_candidates.sort(key=lambda x: x["stars"], reverse=True)
    print(f"[{cat}] Collected {len(raw_candidates)} unique candidates, probing latest releases...", flush=True)

    qualified: list[dict[str, Any]] = []

    def probe(cand: dict[str, Any]) -> dict[str, Any] | None:
        repo = cand["repo"]
        rel = run_json(["gh", "api", f"repos/{repo}/releases/latest"], timeout=20)
        tag = rel.get("tag_name")
        if not tag:
            return None
        assets = rel.get("assets") or []
        asset = pick_best_asset(assets)
        if not asset:
            return None
        return {
            "repo": repo,
            "stars": cand["stars"],
            "description": cand["description"],
            "topics": cand["topics"],
            "version": tag,
            "published_at": (rel.get("published_at") or "")[:10],
            "github_release_url": rel.get("html_url") or f"https://github.com/{repo}/releases",
            "asset_name": asset["name"],
            "asset_size_bytes": int(asset["size"]),
            "github_download_url": asset["browser_download_url"],
        }

    with concurrent.futures.ThreadPoolExecutor(max_workers=10) as ex:
        # Process in batches of 30 to avoid wasting API calls once we hit CANDIDATE_BUFFER
        idx = 0
        while idx < len(raw_candidates) and len(qualified) < CANDIDATE_BUFFER:
            batch = raw_candidates[idx : idx + 35]
            idx += 35
            for res in ex.map(probe, batch):
                if res is None:
                    continue
                slug = slugify(res["repo"])
                if slug in seen_slugs:
                    slug = slugify(res["repo"].replace("/", "-"))
                if slug in seen_slugs:
                    continue
                seen_slugs.add(slug)
                seen_repos.add(res["repo"])

                short_name = res["repo"].split("/")[-1]
                desc = res["description"] or f"{short_name} 开源项目最新正式版安装包"
                topics_str = ",".join(res["topics"][:5])
                qualified.append({
                    "slug": slug,
                    "name": f"{short_name} ({CAT_LABEL_ZH[cat]})",
                    "repo": res["repo"],
                    "category": cat,
                    "stars": res["stars"],
                    "version": res["version"],
                    "published_at": res["published_at"],
                    "platform": infer_platform(res["asset_name"]),
                    "summary": desc[:220],
                    "keywords": f"{short_name}下载,{short_name}最新版,{CAT_LABEL_ZH[cat]},夸克网盘直链,{topics_str}".strip(","),
                    "cta_type": CTA_BY_CAT[cat],
                    "asset_name": res["asset_name"],
                    "asset_size_bytes": res["asset_size_bytes"],
                    "sha256": "",
                    "github_release_url": res["github_release_url"],
                    "github_download_url": res["github_download_url"],
                    "quark_url": "",
                })
                if len(qualified) >= CANDIDATE_BUFFER:
                    break
            print(f"  [{cat}] qualified {len(qualified)}/{CANDIDATE_BUFFER} (probed {idx}/{len(raw_candidates)})", flush=True)

    return qualified


def ensure_quark_dir(path: str) -> None:
    with LOCK:
        if path in DIR_CACHE:
            return
        parts = [p for p in path.strip("/").split("/") if p]
        cur = "/"
        for part in parts:
            target = f"{cur.rstrip('/')}/{part}" if cur != "/" else f"/{part}"
            if target not in DIR_CACHE:
                info = run_json(["kuake", "info", target], timeout=30)
                if not info.get("success"):
                    run_json(["kuake", "create", part, cur], timeout=30)
                DIR_CACHE.add(target)
            cur = target


def sync_one(item: dict[str, Any], share_map: dict[str, str], tmp_root: Path) -> dict[str, Any] | None:
    slug = item["slug"]
    cat = item["category"]
    asset_name = item["asset_name"]
    dl_url = item["github_download_url"]
    remote_dir = f"/Lain42-Releases/{cat}/{slug}"
    remote_path = f"{remote_dir}/{asset_name}"

    # Reuse existing share if already in share_map
    with LOCK:
        existing_share = share_map.get(asset_name)
    if item.get("quark_url"):
        return item
    if existing_share:
        item["quark_url"] = existing_share
        return item

    ensure_quark_dir(remote_dir)

    # Check if file already uploaded to Quark
    info = run_json(["kuake", "info", remote_path], timeout=30)
    if not info.get("success"):
        local_dir = tmp_root / f"{cat}-{slug}"
        local_dir.mkdir(parents=True, exist_ok=True)
        local_file = local_dir / asset_name
        try:
            dl = subprocess.run(
                ["curl", "-fL", "--retry", "2", "--connect-timeout", "15", "--max-time", "180", "-sS", "-o", str(local_file), dl_url],
                env=ENV,
                timeout=200,
            )
            if dl.returncode != 0 or not local_file.exists() or local_file.stat().st_size < 1000:
                return None

            # Compute SHA-256
            h = hashlib.sha256()
            with local_file.open("rb") as f:
                for chunk in iter(lambda: f.read(1024 * 1024), b""):
                    h.update(chunk)
            item["sha256"] = h.hexdigest()

            # Upload to Quark with retry
            up_ok = False
            for attempt in range(3):
                up = run_json(
                    ["kuake", "upload", str(local_file), remote_path, "--max_upload_parallel", "4"],
                    timeout=240,
                )
                if up.get("success"):
                    up_ok = True
                    break
                time.sleep(2 * (attempt + 1))
            if not up_ok:
                return None
        except Exception:
            return None
        finally:
            if local_file.exists():
                local_file.unlink(missing_ok=True)
            try:
                local_dir.rmdir()
            except Exception:
                pass

    # Share permanent no-passcode link with retry
    for attempt in range(3):
        sh = run_json(["kuake", "share", remote_path, "0", "false"], timeout=45)
        data = sh.get("data") if isinstance(sh.get("data"), dict) else {}
        url = data.get("share_url") or data.get("url") or ""
        if url:
            item["quark_url"] = url
            with LOCK:
                share_map[asset_name] = url
            return item
        time.sleep(2 * (attempt + 1))
    return None


def save_merged(base_items: list[dict[str, Any]], synced_by_slug: dict[str, dict[str, Any]]) -> None:
    merged: list[dict[str, Any]] = []
    seen: set[str] = set()
    for it in base_items:
        merged.append(it)
        seen.add(it["slug"])
    for slug, it in synced_by_slug.items():
        if slug not in seen and it.get("quark_url"):
            merged.append(it)
            seen.add(slug)
    tmp = DOWNLOADS_PATH.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(merged, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    tmp.replace(DOWNLOADS_PATH)


def main() -> int:
    # Load existing downloads.json (self + oss + any previously synced 400 items)
    existing: list[dict[str, Any]] = []
    if DOWNLOADS_PATH.exists():
        existing = json.loads(DOWNLOADS_PATH.read_text("utf-8"))

    base_items = [x for x in existing if x.get("category") in ("self", "oss")]
    synced_by_slug: dict[str, dict[str, Any]] = {
        x["slug"]: x for x in existing if x.get("category") in ("game", "video", "music", "ebook") and x.get("quark_url")
    }
    seen_repos = {x["repo"] for x in existing}
    seen_slugs = {x["slug"] for x in existing}

    # Phase 1: Discovery (resumable)
    discovered: dict[str, list[dict[str, Any]]] = {}
    if DISCOVERED_PATH.exists():
        try:
            discovered = json.loads(DISCOVERED_PATH.read_text("utf-8"))
        except Exception:
            discovered = {}

    changed_disc = False
    for cat in ("game", "video", "music", "ebook"):
        cur_list = discovered.get(cat) or []
        for x in cur_list:
            seen_repos.add(x["repo"])
            seen_slugs.add(x["slug"])
        if len(cur_list) < CANDIDATE_BUFFER:
            more = discover_category(cat, seen_repos, seen_slugs)
            discovered[cat] = cur_list + more
            changed_disc = True
            DISCOVERED_PATH.write_text(json.dumps(discovered, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    if changed_disc:
        print("\nDiscovery complete:", {k: len(v) for k, v in discovered.items()}, flush=True)

    # Phase 2: Preload existing Quark shares
    share_map: dict[str, str] = {}
    for page in range(1, 15):
        sl = run_json(["kuake", "share-list", str(page), "50"], timeout=30)
        lst = sl.get("data", {}).get("list", []) if isinstance(sl.get("data"), dict) else []
        if not lst:
            break
        for s in lst:
            t = s.get("title", "")
            u = s.get("share_url", "")
            if t and u:
                share_map[t] = u
        if len(lst) < 50:
            break
    print(f"Loaded {len(share_map)} existing Quark share links.", flush=True)

    for cat in ("game", "video", "music", "ebook"):
        ensure_quark_dir(f"/Lain42-Releases/{cat}")

    # Phase 3: Sync each category until >= TARGET_PER_CAT succeeded
    disk_tmp_base = Path("/home/lilyco/.cache/lain42-dl-tmp")
    disk_tmp_base.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="lain42-400-", dir=str(disk_tmp_base)) as td:
        tmp_root = Path(td)
        for cat in ("game", "video", "music", "ebook"):
            done_in_cat = sum(1 for x in synced_by_slug.values() if x["category"] == cat and x.get("quark_url"))
            print(f"\n=== Syncing [{cat}] (already {done_in_cat}/{TARGET_PER_CAT}) ===", flush=True)
            if done_in_cat >= TARGET_PER_CAT:
                continue

            queue = [x for x in discovered[cat] if x["slug"] not in synced_by_slug]
            with concurrent.futures.ThreadPoolExecutor(max_workers=4) as ex:
                # Submit in waves of 16 so we stop cleanly once we reach TARGET_PER_CAT
                q_idx = 0
                while done_in_cat < TARGET_PER_CAT and q_idx < len(queue):
                    need = TARGET_PER_CAT - done_in_cat
                    wave = queue[q_idx : q_idx + max(need, 8)]
                    q_idx += len(wave)
                    futs = {ex.submit(sync_one, item, share_map, tmp_root): item for item in wave}
                    for fut in concurrent.futures.as_completed(futs):
                        res = fut.result()
                        if res and res.get("quark_url"):
                            synced_by_slug[res["slug"]] = res
                            done_in_cat += 1
                            print(
                                f"  ✓ [{cat} {done_in_cat}/{TARGET_PER_CAT}] {res['slug']} ({res['version']}, ★{res['stars']}) -> {res['quark_url']}",
                                flush=True,
                            )
                            if done_in_cat % 5 == 0:
                                save_merged(base_items, synced_by_slug)
                    save_merged(base_items, synced_by_slug)

    save_merged(base_items, synced_by_slug)
    counts = {}
    final_items = json.loads(DOWNLOADS_PATH.read_text("utf-8"))
    for it in final_items:
        c = it.get("category", "other")
        counts[c] = counts.get(c, 0) + 1
    print(f"\nALL DONE! Total items in downloads.json: {len(final_items)} | Breakdown: {counts}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
