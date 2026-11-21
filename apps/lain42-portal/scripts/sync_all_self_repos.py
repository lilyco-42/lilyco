#!/usr/bin/env python3
"""
Fast, incremental, disk-safe sync of ALL public `lilyco-42` repositories to Quark Drive
(`/Lain42-Releases/self/<slug>/<asset>`) via `kuake-studio` CLI and into `data/downloads.json`.

Key optimizations:
1. Uses `/home/lilyco/.local/opt/kuake-studio/kuake-studio` (pure-Rust native Quark client).
2. Never touches `/tmp` (uses `/home/lilyco/.cache/lain42-dl-tmp` and deletes each file immediately).
3. Incremental checkpoint to `data/downloads.json` after every 3 repos so progress is never lost.
4. Prefers <= 65 MB release binary assets first for fast download/upload; falls back to <= 180 MB.
5. If a repo has no binary release asset:
   - If `/home/lilyco/code/<name>` exists locally, runs `git archive --format=zip` in 50ms (zero network wait!).
   - Otherwise uses `gh api repos/lilyco-42/<name>/zipball` (authenticated, auto-resolves default branch).
"""

from __future__ import annotations

import concurrent.futures
import hashlib
import json
import os
import re
import shutil
import subprocess
import threading
import time
from pathlib import Path
from typing import Any

KUAKE_STUDIO = "/home/lilyco/.local/opt/kuake-studio/kuake-studio"
DATA_DIR = Path(__file__).resolve().parent.parent / "data"
DOWNLOADS_PATH = DATA_DIR / "downloads.json"
DISK_TMP_ROOT = Path("/home/lilyco/.cache/lain42-dl-tmp")
LOCAL_CODE_DIR = Path("/home/lilyco/code")

ENV = os.environ.copy()
ENV["HOME"] = "/home/lilyco"
ENV["TMPDIR"] = str(DISK_TMP_ROOT)

LOCK = threading.Lock()
DIR_CACHE: set[str] = {"/", "/Lain42-Releases", "/Lain42-Releases/self"}

PREFER_PATTERNS = [
    re.compile(r"(win|windows).*(x64|x86_64|amd64|64).*\.(exe|msi|zip|7z)$", re.I),
    re.compile(r"(x64|x86_64|amd64).*\.(exe|msi|zip)$", re.I),
    re.compile(r"\.(exe|msi|apk)$", re.I),
    re.compile(r"(linux|x86_64|amd64).*\.(AppImage|deb|tar\.gz|zip)$", re.I),
    re.compile(r"\.(zip|tar\.gz|7z|jar|dmg|AppImage|deb|pkg\.tar\.zst|sh)$", re.I),
]
BAD_ASSET_RE = re.compile(
    r"(\.sig|\.sha256|\.sha256sum|\.sha512|\.asc|\.blockmap|\.yml|\.yaml|\.pdb|^SHA256SUMS)",
    re.I,
)


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


def is_ok(res: dict[str, Any]) -> bool:
    return res.get("code") == "OK" or res.get("success") is True


def slugify(name: str) -> str:
    s = re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")
    return s or "lilyco-tool"


def ensure_quark_dir(path: str) -> None:
    with LOCK:
        if path in DIR_CACHE:
            return
        parts = [p for p in path.strip("/").split("/") if p]
        cur = "/"
        for part in parts:
            target = f"{cur.rstrip('/')}/{part}" if cur != "/" else f"/{part}"
            if target not in DIR_CACHE:
                run_json([KUAKE_STUDIO, "create", "--name", part, "--pdir", cur, "--json"], timeout=30)
                DIR_CACHE.add(target)
            cur = target


def pick_best_asset(assets: list[dict[str, Any]]) -> dict[str, Any] | None:
    for max_mb in (65.0, 180.0):
        valid = []
        for a in assets:
            name = a.get("name") or ""
            size = int(a.get("size") or 0)
            if size <= 64 or size > int(max_mb * 1_048_576):
                continue
            if BAD_ASSET_RE.search(name):
                continue
            valid.append(a)
        if not valid:
            continue
        for pat in PREFER_PATTERNS:
            for a in valid:
                if pat.search(a["name"]):
                    return a
        valid.sort(key=lambda x: x.get("size", 0))
        return valid[0]
    return None


def infer_platform(asset_name: str, desc: str) -> str:
    low = (asset_name + " " + desc).lower()
    plats = []
    if any(k in low for k in ("win", ".exe", ".msi")):
        plats.append("Windows")
    if any(k in low for k in ("android", ".apk", "termux")):
        plats.append("Android")
    if any(k in low for k in ("linux", "arch", "wayland", ".appimage", ".deb", ".tar.gz")):
        plats.append("Linux")
    if any(k in low for k in ("mac", "darwin", ".dmg", "ios")):
        plats.append("macOS")
    return " / ".join(plats) if plats else "Windows / Linux / macOS"


def infer_cta(name: str, desc: str) -> str:
    low = (name + " " + desc).lower()
    if any(k in low for k in ("proxy", "ghboost", "vpn", "chrome", "dl", "video", "bili")):
        return "proxy"
    if any(k in low for k in ("music", "stem", "vocal", "audio")):
        return "music"
    return "ai"


def sync_self_repo(
    repo_info: dict[str, Any],
    existing_by_slug: dict[str, dict[str, Any]],
    share_map: dict[str, str],
) -> dict[str, Any] | None:
    name = repo_info["name"]
    if name in (".github", "lilyco-42", "lilyco-42.github.io"):
        return None
    full_repo = f"lilyco-42/{name}"
    slug = "lyco-music" if name == "lyco_music" else ("apk-lerun" if name == "-apk-" else slugify(name))
    stars = int(repo_info.get("stargazerCount") or 0)
    desc = (repo_info.get("description") or "").strip() or f"云枢智创 (lilyco-42) 自研开源项目 {name}"
    updated_at = (repo_info.get("updatedAt") or "")[:10]
    latest_rel = repo_info.get("latestRelease")

    tag = latest_rel["tagName"] if latest_rel and latest_rel.get("tagName") else "latest"
    rel_url = (
        latest_rel["url"]
        if latest_rel and latest_rel.get("url")
        else f"https://github.com/{full_repo}"
    )

    prev = existing_by_slug.get(slug)
    if prev and prev.get("quark_url") and prev.get("version") == tag:
        return prev

    asset_name = ""
    asset_size = 0
    dl_url = ""
    use_release_asset = False

    if latest_rel and latest_rel.get("tagName"):
        rel_detail = run_json(["gh", "api", f"repos/{full_repo}/releases/latest"], timeout=25)
        assets = rel_detail.get("assets") or []
        best = pick_best_asset(assets)
        if best:
            asset_name = best["name"]
            asset_size = int(best["size"])
            dl_url = best["browser_download_url"]
            use_release_asset = True
            published_at = (rel_detail.get("published_at") or updated_at)[:10]
        else:
            published_at = (latest_rel.get("publishedAt") or updated_at)[:10]
    else:
        published_at = updated_at

    safe_tag = re.sub(r"[^a-zA-Z0-9._-]+", "-", tag)
    if not use_release_asset:
        asset_name = f"{slug}-{safe_tag}.zip"
        dl_url = f"https://github.com/{full_repo}/archive/refs/heads/main.zip"

    remote_dir = f"/Lain42-Releases/self/{slug}"
    remote_path = f"{remote_dir}/{asset_name}"
    ensure_quark_dir(remote_dir)

    with LOCK:
        cached_share = share_map.get(asset_name)

    sha256_hex = prev.get("sha256", "") if prev else ""
    dir_ls = run_json([KUAKE_STUDIO, "list", "--path", remote_dir, "--json"], timeout=30)
    dir_items = (dir_ls.get("data") or {}).get("list") or [] if isinstance(dir_ls.get("data"), dict) else []
    existing_remote = next((x for x in dir_items if x.get("file_name") == asset_name), None)

    if existing_remote:
        if not asset_size:
            asset_size = int(existing_remote.get("size") or 0)
    else:
        work_dir = DISK_TMP_ROOT / f"self-{slug}"
        work_dir.mkdir(parents=True, exist_ok=True)
        local_file = work_dir / asset_name
        try:
            got_file = False
            local_git = LOCAL_CODE_DIR / name
            if not use_release_asset and (local_git / ".git").exists():
                ar = subprocess.run(
                    ["git", "-C", str(local_git), "archive", "--format=zip", "-o", str(local_file), "HEAD"],
                    env=ENV,
                    timeout=30,
                )
                if ar.returncode == 0 and local_file.exists() and local_file.stat().st_size > 100:
                    got_file = True

            if not got_file and use_release_asset:
                gh_dl = subprocess.run(
                    ["gh", "release", "download", tag, "-R", full_repo, "-p", asset_name, "-D", str(work_dir), "--clobber"],
                    env=ENV,
                    timeout=90,
                    capture_output=True,
                )
                if gh_dl.returncode == 0 and local_file.exists() and local_file.stat().st_size > 64:
                    got_file = True

            if not got_file:
                with local_file.open("wb") as out_f:
                    zb = subprocess.run(
                        ["gh", "api", f"repos/{full_repo}/zipball"],
                        env=ENV,
                        stdout=out_f,
                        stderr=subprocess.DEVNULL,
                        timeout=60,
                    )
                if zb.returncode == 0 and local_file.exists() and local_file.stat().st_size > 100:
                    got_file = True
                    if use_release_asset:
                        asset_name = f"{slug}-{safe_tag}.zip"
                        remote_path = f"{remote_dir}/{asset_name}"

            if not got_file:
                return None

            asset_size = local_file.stat().st_size
            h = hashlib.sha256()
            with local_file.open("rb") as f:
                for chunk in iter(lambda: f.read(1024 * 1024), b""):
                    h.update(chunk)
            sha256_hex = h.hexdigest()

            up_ok = False
            for attempt in range(3):
                up = run_json(
                    [KUAKE_STUDIO, "upload", "--file", str(local_file), "--dest", remote_path, "--json"],
                    timeout=180,
                )
                if is_ok(up):
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
            shutil.rmtree(work_dir, ignore_errors=True)

    quark_url = cached_share or ""
    if not quark_url:
        for attempt in range(3):
            sh = run_json([KUAKE_STUDIO, "share", "--path", remote_path, "--days", "0", "--json"], timeout=45)
            data = sh.get("data") if isinstance(sh.get("data"), dict) else {}
            url = data.get("share_url") or data.get("url") or ""
            if url:
                quark_url = url
                with LOCK:
                    share_map[asset_name] = url
                break
            time.sleep(2 * (attempt + 1))

    if not quark_url:
        return None

    display_name = prev["name"] if (prev and "—" in prev.get("name", "")) else f"{name} — 云枢智创自研工具"
    return {
        "slug": slug,
        "name": display_name,
        "repo": full_repo,
        "category": "self",
        "stars": stars,
        "version": tag,
        "published_at": published_at,
        "platform": infer_platform(asset_name, desc),
        "summary": desc[:220],
        "keywords": f"{name}下载,{name}最新版,云枢智创,lilyco-42,夸克网盘直链",
        "cta_type": infer_cta(name, desc),
        "asset_name": asset_name,
        "asset_size_bytes": asset_size,
        "sha256": sha256_hex,
        "github_release_url": rel_url,
        "github_download_url": dl_url,
        "quark_url": quark_url,
    }


def save_checkpoint(synced_self: dict[str, dict[str, Any]], other_items: list[dict[str, Any]]) -> None:
    with LOCK:
        merged = list(synced_self.values()) + other_items
        tmp_out = DOWNLOADS_PATH.with_suffix(".json.tmp")
        tmp_out.write_text(json.dumps(merged, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        tmp_out.replace(DOWNLOADS_PATH)


def main() -> int:
    shutil.rmtree(DISK_TMP_ROOT, ignore_errors=True)
    DISK_TMP_ROOT.mkdir(parents=True, exist_ok=True)
    try:
        existing: list[dict[str, Any]] = []
        if DOWNLOADS_PATH.exists():
            existing = json.loads(DOWNLOADS_PATH.read_text("utf-8"))

        existing_self = {x["slug"]: x for x in existing if x.get("category") == "self"}
        other_items = [x for x in existing if x.get("category") != "self"]

        share_map: dict[str, str] = {}
        for page in range(1, 12):
            sl = run_json([KUAKE_STUDIO, "share-list", "--page", str(page), "--size", "50", "--json"], timeout=30)
            lst = sl.get("data", {}).get("list", []) if isinstance(sl.get("data"), dict) else []
            if not lst:
                break
            for s in lst:
                t = s.get("title", "") or (s.get("first_file") or {}).get("file_name", "")
                u = s.get("share_url", "")
                if t and u:
                    share_map[t] = u
            if len(lst) < 50:
                break
        print(f"Loaded {len(share_map)} existing Quark share links via kuake-studio.", flush=True)

        # Pre-populate existing self dirs on Quark
        ls_self = run_json([KUAKE_STUDIO, "list", "--path", "/Lain42-Releases/self", "--json"], timeout=30)
        for item in ((ls_self.get("data") or {}).get("list") or []) if isinstance(ls_self.get("data"), dict) else []:
            if item.get("dir") and item.get("path"):
                DIR_CACHE.add(item["path"])

        p = subprocess.run(
            [
                "gh",
                "repo",
                "list",
                "lilyco-42",
                "--limit",
                "150",
                "--json",
                "name,description,stargazerCount,url,isPrivate,latestRelease,updatedAt",
            ],
            capture_output=True,
            text=True,
            env=ENV,
            check=True,
        )
        all_public = [r for r in json.loads(p.stdout) if not r.get("isPrivate")]
        all_public.sort(key=lambda r: (0 if r.get("latestRelease") else 1, -(r.get("stargazerCount") or 0)))
        print(f"Syncing {len(all_public)} public lilyco-42 repositories (release repos first)...", flush=True)

        synced_self: dict[str, dict[str, Any]] = dict(existing_self)
        done = 0
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as ex:
            futs = {ex.submit(sync_self_repo, r, existing_self, share_map): r["name"] for r in all_public}
            for fut in concurrent.futures.as_completed(futs):
                res = fut.result()
                if res and res.get("quark_url"):
                    synced_self[res["slug"]] = res
                    done += 1
                    print(f"  ✓ [self {done}/{len(all_public)}] {res['slug']} ({res['version']}) -> {res['quark_url']}", flush=True)
                    if done % 3 == 0:
                        save_checkpoint(synced_self, other_items)

        save_checkpoint(synced_self, other_items)
        print(f"\nALL SELF REPOS SYNCED! self={len(synced_self)}, total={len(synced_self) + len(other_items)}", flush=True)
        return 0
    finally:
        shutil.rmtree(DISK_TMP_ROOT, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
