#!/usr/bin/env python3
"""
Lain42 Release Mirror Sync Tool (GitHub Releases -> Quark Drive via `kuake` CLI -> data/downloads.json).

Idempotent:
- Creates `/Lain42-Releases/self` and `/Lain42-Releases/oss` in Quark Drive if missing.
- For each project in CATALOG, fetches latest GitHub Release via `gh api`, picks the best
  mainstream user asset (Windows x64 / Android APK / Linux x64), downloads it to a temp dir,
  computes SHA-256, uploads via `kuake upload`, creates a permanent no-passcode share link
  via `kuake share <path> 0 false`, and removes the local temp file.
- Outputs `apps/lain42-portal/data/downloads.json` for `lain42-portal` SSR & SEO pages.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

OUT_PATH = Path(__file__).resolve().parent.parent / "data" / "downloads.json"

CATALOG: list[dict[str, Any]] = [
    # ── 自研工具 (lilyco-42) ──────────────────────────────────────────────
    {
        "slug": "lyco-music",
        "name": "Lyco Music (落雪网易云风格重制版)",
        "repo": "lilyco-42/lyco_music",
        "category": "self",
        "platform": "Windows / Android / Linux / macOS",
        "summary": "基于 Vue3 + shadcn-vue 重制的落雪音乐高颜值网易云风客户端，支持桌面端与安卓端，配合 LYCO 音源享受无损音质。",
        "keywords": "Lyco Music下载,落雪音乐重制版,LX Music下载,免费无损音乐播放器,夸克网盘下载",
        "cta_type": "music",
        "prefer_patterns": [r"win.*x64.*\.(exe|zip)$", r"android.*universal\.apk$", r"\.apk$", r"\.exe$"],
        "existing_quark_url": "https://pan.quark.cn/s/4dffb41ad5d1",
    },
    {
        "slug": "kuake-studio",
        "name": "Kuake Studio 夸克网盘桌面客户端",
        "repo": "lilyco-42/kuake-studio",
        "category": "self",
        "platform": "Windows / Linux / macOS / Android",
        "summary": "Rust + lilyco 原生构建的轻量夸克网盘客户端与命令行工具，支持多线程高速传输、批量分享与 AI MCP 控制。",
        "keywords": "夸克网盘客户端,kuake-studio下载,夸克网盘Linux,夸克CLI命令行工具",
        "cta_type": "ai",
        "prefer_patterns": [r"windows.*x86_64.*\.(zip|exe)$", r"linux.*x86_64\.tar\.gz$", r"\.zip$", r"\.tar\.gz$"],
    },
    {
        "slug": "ghboost",
        "name": "GHBoost — GitHub / Google / YouTube 一键加速",
        "repo": "lilyco-42/ghboost",
        "category": "self",
        "platform": "Windows / Linux / macOS / Android",
        "summary": "开源免配置桌面托盘加速工具，内置 GitHub Hosts 加速、节点测速与一键部署，解决国内克隆和下载慢的问题。",
        "keywords": "GitHub加速工具,ghboost下载,GitHub国内下载加速,开源加速软件",
        "cta_type": "proxy",
        "prefer_patterns": [r"x86_64-pc-windows-msvc\.exe$", r"windows.*\.exe$", r"\.exe$"],
    },
    {
        "slug": "lycodl",
        "name": "LycoDL — 多平台高清视频下载器",
        "repo": "lilyco-42/lycodl",
        "category": "self",
        "platform": "Windows / Linux / macOS / Android",
        "summary": "基于 yt-dlp 内核 + lilyco 命令面与 shadcn Web 控制台的多平台视频下载器，支持 B站、YouTube、抖音等千余站点。",
        "keywords": "lycodl下载,yt-dlp图形界面,B站视频下载器,YouTube高清视频下载工具",
        "cta_type": "proxy",
        "prefer_patterns": [r"windows.*\.zip$", r"linux-x64\.tar\.gz$", r"\.tar\.gz$"],
    },
    {
        "slug": "lyblive",
        "name": "LyBLive — B站 AI 跨平台直播助手",
        "repo": "lilyco-42/lyblive",
        "category": "self",
        "platform": "Windows / Linux / macOS / Android",
        "summary": "基于 lilyco + shadcn UI 构建的 Bilibili 直播工具，原生支持 AI Agent 弹幕互动、开播控制与跨平台运行。",
        "keywords": "B站直播助手,lyblive下载,Bilibili开源直播工具,AI弹幕助手",
        "cta_type": "ai",
        "prefer_patterns": [r"windows.*\.exe$", r"linux-x86_64$", r"android-aarch64$"],
    },
    {
        "slug": "rembg-ui",
        "name": "Rembg UI — 本地 AI 一键去背景抠图神器",
        "repo": "lilyco-42/rembg-ui",
        "category": "self",
        "platform": "Windows / Linux / Android",
        "summary": "对 rembg 进行深度图形化封装的本地离线 AI 抠图工具，完全免费无水印，支持批量人像与商品透明底处理。",
        "keywords": "AI抠图软件免费,rembg-ui下载,本地离线去背景工具,批量证件照抠图",
        "cta_type": "ai",
        "prefer_patterns": [r"win.*\.(zip|exe)$", r"android-arm64-v8a\.apk$", r"\.apk$"],
    },
    {
        "slug": "lycomc",
        "name": "LilyCraft Web — 现代 Minecraft 启动器",
        "repo": "lilyco-42/lycomc",
        "category": "self",
        "platform": "Windows / Linux / macOS",
        "summary": "Web-first 架构的轻量我的世界启动器，提供清爽 shadcn 控制台，支持快速切换整合包与联机启动。",
        "keywords": "Minecraft启动器下载,lycomc,我的世界轻量启动器,LilyCraft",
        "cta_type": "ai",
        "prefer_patterns": [r"windows.*\.(exe|zip)$", r"linux.*\.(tar\.gz|AppImage)$", r"\.zip$", r"\.tar\.gz$"],
    },
    {
        "slug": "dsh-termux",
        "name": "DSH Termux — 安卓高效终端与 AI 开发环境",
        "repo": "lilyco-42/dsh-termux",
        "category": "self",
        "platform": "Android (Termux) / Linux",
        "summary": "24+ Star 热门开源项目，为 Termux 提供开箱即用的现代化终端增强与移动开发工作流。",
        "keywords": "Termux工具下载,dsh-termux,安卓Linux终端配置,手机写代码工具",
        "cta_type": "ai",
        "prefer_patterns": [r"\.(apk|tar\.gz|zip|sh)$"],
    },
    {
        "slug": "whisper-subtitle-tool",
        "name": "Whisper Subtitle Tool — 本地 AI 语音转 SRT 字幕",
        "repo": "lilyco-42/whisper-subtitle-tool",
        "category": "self",
        "platform": "Windows / Linux",
        "summary": "基于 faster-whisper 的图形化音视频自动转字幕工具，离线高速生成精准时间轴 SRT 字幕文件。",
        "keywords": "Whisper字幕生成工具,音频转SRT字幕软件,faster-whisper图形版,免费视频加字幕",
        "cta_type": "ai",
        "prefer_patterns": [r"\.(exe|zip|7z|tar\.gz)$"],
    },
    {
        "slug": "lyco-skill",
        "name": "Lyco Skill — AI Agent 项目预研先行技能包",
        "repo": "lilyco-42/lyco-skill",
        "category": "self",
        "platform": "Cross-Platform (Claude / Codex / Antigravity)",
        "summary": "写代码前先侦察！自动用 GitHub 近义词搜索 + 论坛避坑调研，帮 AI Agent 少造 80% 轮子。",
        "keywords": "AI Agent Skill,lyco-skill,Claude Code技能,Codex技能包,项目预研",
        "cta_type": "ai",
        "prefer_patterns": [r"\.(zip|tar\.gz|md)$"],
    },
    {
        "slug": "lyvideo",
        "name": "LyVideo — 轻量本地视频处理与剪辑工具",
        "repo": "lilyco-42/lyvideo",
        "category": "self",
        "platform": "Windows / Linux",
        "summary": "Rust 构建的轻量高速视频处理工具，支持 Windows 与 Linux 绿色免安装运行。",
        "keywords": "lyvideo下载,轻量视频工具,Rust视频处理,夸克网盘下载",
        "cta_type": "ai",
        "prefer_patterns": [r"windows-x64\.zip$", r"linux-x64\.zip$", r"\.zip$"],
    },
    {
        "slug": "mcport",
        "name": "McPort — Minecraft 跨版本 Mod 兼容性分析与迁移工具",
        "repo": "lilyco-42/mcport",
        "category": "self",
        "platform": "Windows / Linux / macOS (Java)",
        "summary": "Minecraft 跨版本 Mod 兼容性分析器与启动时 JavaAgent 重映射工具，助力整合包升级。",
        "keywords": "Minecraft Mod跨版本工具,McPort下载,MC模组迁移,整合包升级工具",
        "cta_type": "ai",
        "prefer_patterns": [r"\.tar\.gz$", r"\.zip$", r"\.jar$"],
    },
    # ── 高星开源项目国内高速镜像 (高搜索流量引流) ─────────────────────────
    {
        "slug": "clash-verge-rev",
        "name": "Clash Verge Rev 最新版国内高速下载",
        "repo": "clash-verge-rev/clash-verge-rev",
        "category": "oss",
        "platform": "Windows / macOS / Linux",
        "summary": "基于 Tauri + Mihomo 内核的新一代跨平台代理客户端，界面现代流畅、内存占用低，完美兼容 LYCO Connect 订阅。",
        "keywords": "Clash Verge Rev下载,Clash Verge最新版国内下载,Clash客户端夸克网盘,Mihomo代理客户端",
        "cta_type": "proxy",
        "prefer_patterns": [r"Clash\.Verge_.*_x64-setup\.exe$", r"x64-setup\.exe$", r"x64_portable\.zip$"],
    },
    {
        "slug": "v2rayn",
        "name": "v2rayN 最新版 Windows 国内高速下载",
        "repo": "2dust/v2rayN",
        "category": "oss",
        "platform": "Windows / Linux / macOS",
        "summary": "120k+ Star 现象级开源代理客户端，支持 Xray / sing-box / Mihomo 多核心，一键导入订阅链接即连。",
        "keywords": "v2rayN下载,v2rayN最新版夸克网盘,v2rayN Windows 64位国内镜像,代理客户端下载",
        "cta_type": "proxy",
        "prefer_patterns": [r"v2rayN-windows-64\.zip$", r"v2rayN-windows-64-desktop\.zip$", r"windows-64\.zip$"],
    },
    {
        "slug": "cherry-studio",
        "name": "Cherry Studio 最新版 — 全能 AI 桌面客户端",
        "repo": "CherryHQ/cherry-studio",
        "category": "oss",
        "platform": "Windows / macOS / Linux",
        "summary": "53k+ Star 开源多模型 AI 桌面客户端，内置知识库 RAG、MCP 工具调用与多助手切换，填上 Lain42 API Key 即可畅用 Claude / GPT / Gemini。",
        "keywords": "Cherry Studio下载,Cherry Studio最新版夸克网盘,AI桌面客户端,Claude客户端配置",
        "cta_type": "ai",
        "prefer_patterns": [r"Cherry-Studio-.*-x64-setup\.exe$", r"Cherry-Studio-.*-x64-portable\.exe$", r"setup\.exe$"],
    },
    {
        "slug": "lx-music-desktop",
        "name": "LX Music 落雪音乐助手桌面版最新版",
        "repo": "lyswhut/lx-music-desktop",
        "category": "oss",
        "platform": "Windows / macOS / Linux",
        "summary": "55k+ Star 开源免费音乐播放器，纯净无广告，支持自定义音源脚本与无损音质下载。",
        "keywords": "落雪音乐下载,LX Music最新版夸克网盘,落雪音乐桌面版,免费无损音乐软件",
        "cta_type": "music",
        "prefer_patterns": [r"lx-music-desktop-v.*-x64-Setup\.exe$", r"x64-Setup\.exe$", r"win_x64-green\.7z$"],
    },
    {
        "slug": "rustdesk",
        "name": "RustDesk 最新版 — 开源免费远程桌面软件",
        "repo": "rustdesk/rustdesk",
        "category": "oss",
        "platform": "Windows / macOS / Linux / Android",
        "summary": "130k+ Star 开源远程控制神器，完全免费不限速、支持自建中继服务器，向日葵与 ToDesk 的最佳开源替代品。",
        "keywords": "RustDesk下载,RustDesk最新版夸克网盘,开源远程桌面软件,免费远程控制替代向日葵",
        "cta_type": "proxy",
        "prefer_patterns": [r"rustdesk-.*-x86_64\.exe$", r"x86_64\.exe$", r"x86_64\.msi$"],
    },
    {
        "slug": "yt-dlp",
        "name": "yt-dlp 最新版 — 全网开源视频下载神器",
        "repo": "yt-dlp/yt-dlp",
        "category": "oss",
        "platform": "Windows / Linux / macOS",
        "summary": "100k+ Star 全网最强命令行视频下载工具，支持 YouTube 4K/8K、Bilibili、Twitter/X 等 1000+ 站点。",
        "keywords": "yt-dlp下载,yt-dlp最新版夸克网盘,YouTube视频下载工具,开源命令行视频下载",
        "cta_type": "proxy",
        "prefer_patterns": [r"^yt-dlp\.exe$", r"yt-dlp_win\.zip$"],
    },
    {
        "slug": "umi-ocr",
        "name": "Umi-OCR 最新版 — 免费离线批量截图文字识别",
        "repo": "hiroi-sora/Umi-OCR",
        "category": "oss",
        "platform": "Windows / Linux",
        "summary": "30k+ Star 免费开源离线 OCR 文字识别神器，免安装解压即用，支持截图识字、批量图片/PDF 转文字与二维码识别。",
        "keywords": "Umi-OCR下载,Umi-OCR最新版夸克网盘,免费离线OCR截图识字软件,PDF文字提取工具",
        "cta_type": "ai",
        "prefer_patterns": [r"Umi-OCR_Paddle_.*\.7z\.exe$", r"Paddle.*\.7z$", r"\.7z\.exe$", r"\.7z$"],
    },
    {
        "slug": "localsend",
        "name": "LocalSend 最新版 — 跨平台开源局域网传文件神器",
        "repo": "localsend/localsend",
        "category": "oss",
        "platform": "Windows / macOS / Linux / Android / iOS",
        "summary": "58k+ Star 跨平台隔空投送替代品，无需联网、不限速、无广告，Windows / Mac / 安卓 / iPhone 局域网秒传大文件。",
        "keywords": "LocalSend下载,LocalSend最新版夸克网盘,局域网传文件软件,Windows安卓隔空投送",
        "cta_type": "ai",
        "prefer_patterns": [r"LocalSend-.*-windows-x86-64\.exe$", r"windows-x86-64\.zip$", r"x86-64\.exe$"],
    },
]


def run_json(cmd: list[str], env: dict[str, str] | None = None) -> dict[str, Any]:
    p = subprocess.run(cmd, capture_output=True, text=True, env=env)
    out = p.stdout.strip()
    if not out:
        return {"success": False, "error": p.stderr.strip(), "returncode": p.returncode}
    try:
        return json.loads(out)
    except Exception:
        return {"success": False, "raw": out[:400], "error": p.stderr.strip(), "returncode": p.returncode}


def ensure_quark_dir(path: str) -> None:
    parts = [p for p in path.strip("/").split("/") if p]
    cur = "/"
    for part in parts:
        target = f"{cur.rstrip('/')}/{part}" if cur != "/" else f"/{part}"
        info = run_json(["kuake", "info", target])
        if not info.get("success"):
            run_json(["kuake", "create", part, cur])
        cur = target


def pick_asset(assets: list[dict[str, Any]], patterns: list[str], max_mb: float = 280.0) -> dict[str, Any] | None:
    filtered = [
        a for a in assets
        if a.get("size", 0) > 1024
        and a.get("size", 0) <= max_mb * 1048576
        and not a["name"].endswith((".sig", ".sha256", ".sha256sum", ".yml", ".yaml", ".blockmap", ".asc"))
    ]
    for pat in patterns:
        rx = re.compile(pat, re.IGNORECASE)
        for a in filtered:
            if rx.search(a["name"]):
                return a
    if filtered:
        # fallback: first non-tiny installer-like asset
        filtered.sort(key=lambda a: a.get("size", 0))
        return filtered[0]
    return None


def main() -> int:
    env = os.environ.copy()
    env["HOME"] = "/home/lilyco"

    # Load existing downloads.json if present to reuse already-uploaded share links for same version+asset
    existing_by_slug: dict[str, dict[str, Any]] = {}
    if OUT_PATH.exists():
        try:
            for item in json.loads(OUT_PATH.read_text("utf-8")):
                existing_by_slug[item["slug"]] = item
        except Exception:
            pass

    ensure_quark_dir("/Lain42-Releases/self")
    ensure_quark_dir("/Lain42-Releases/oss")

    # Fetch existing shares from kuake to reuse if same filename already shared
    share_map: dict[str, str] = {}
    sl = run_json(["kuake", "share-list", "1", "100"], env=env)
    for s in sl.get("data", {}).get("list", []) if isinstance(sl.get("data"), dict) else []:
        title = s.get("title", "")
        url = s.get("share_url", "")
        if title and url:
            share_map[title] = url

    results: list[dict[str, Any]] = []
    disk_tmp_base = Path("/home/lilyco/.cache/lain42-dl-tmp")
    disk_tmp_base.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="lain42-rel-", dir=str(disk_tmp_base)) as tmpdir:
        for entry in CATALOG:
            slug = entry["slug"]
            repo = entry["repo"]
            cat = entry["category"]
            print(f"[{cat}] {slug} ({repo}) ...", flush=True)

            rel = run_json(["gh", "api", f"repos/{repo}/releases/latest"], env=env)
            repo_meta = run_json(["gh", "api", f"repos/{repo}"], env=env)
            if not rel.get("tag_name"):
                print(f"  ! skip: no latest release for {repo}", flush=True)
                continue

            tag = rel["tag_name"]
            stars = int(repo_meta.get("stargazers_count", 0) or 0)
            published_at = (rel.get("published_at") or "")[:10]
            release_url = rel.get("html_url") or f"https://github.com/{repo}/releases"
            assets = rel.get("assets") or []

            asset = pick_asset(assets, entry.get("prefer_patterns", []))
            asset_name = asset["name"] if asset else f"{slug}-{tag}.zip"
            asset_size = int(asset["size"]) if asset else 0
            asset_dl_url = asset["browser_download_url"] if asset else release_url
            quark_url = entry.get("existing_quark_url", "")
            sha256_hex = ""

            prev = existing_by_slug.get(slug)
            if prev and prev.get("version") == tag and prev.get("asset_name") == asset_name and prev.get("quark_url"):
                quark_url = prev["quark_url"]
                sha256_hex = prev.get("sha256", "")
                print(f"  = cached {asset_name} -> {quark_url}", flush=True)
            elif asset:
                remote_dir = f"/Lain42-Releases/{cat}/{slug}"
                remote_path = f"{remote_dir}/{asset_name}"
                ensure_quark_dir(remote_dir)

                info = run_json(["kuake", "info", remote_path], env=env)
                if not info.get("success"):
                    local_file = Path(tmpdir) / asset_name
                    print(f"  ↓ downloading {asset_name} ({asset_size/1048576:.1f} MB) ...", flush=True)
                    dl = subprocess.run(
                        ["curl", "-fL", "--retry", "2", "--max-time", "180", "-sS", "-o", str(local_file), asset_dl_url],
                        env=env,
                    )
                    if dl.returncode == 0 and local_file.exists():
                        h = hashlib.sha256()
                        with local_file.open("rb") as f:
                            for chunk in iter(lambda: f.read(1024 * 1024), b""):
                                h.update(chunk)
                        sha256_hex = h.hexdigest()
                        print(f"  ↑ uploading to Quark {remote_path} ...", flush=True)
                        up = run_json(["kuake", "upload", str(local_file), remote_path, "--max_upload_parallel", "8"], env=env)
                        local_file.unlink(missing_ok=True)
                        if not up.get("success"):
                            print(f"  ! upload failed: {up}", flush=True)
                    else:
                        print(f"  ! download failed (rc={dl.returncode})", flush=True)

                # Create or reuse permanent no-passcode share link
                if asset_name in share_map:
                    quark_url = share_map[asset_name]
                    print(f"  ✓ reused share {quark_url}", flush=True)
                else:
                    sh = run_json(["kuake", "share", remote_path, "0", "false"], env=env)
                    data = sh.get("data") if isinstance(sh.get("data"), dict) else {}
                    url = data.get("share_url") or data.get("url") or ""
                    if url:
                        quark_url = url
                        print(f"  ✓ shared {quark_url}", flush=True)
                    else:
                        print(f"  ! share response: {sh}", flush=True)

            results.append({
                "slug": slug,
                "name": entry["name"],
                "repo": repo,
                "category": cat,
                "stars": stars,
                "version": tag,
                "published_at": published_at,
                "platform": entry["platform"],
                "summary": entry["summary"],
                "keywords": entry["keywords"],
                "cta_type": entry["cta_type"],
                "asset_name": asset_name,
                "asset_size_bytes": asset_size,
                "sha256": sha256_hex,
                "github_release_url": release_url,
                "github_download_url": asset_dl_url,
                "quark_url": quark_url,
            })

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(results, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"\nDONE! Wrote {len(results)} items to {OUT_PATH}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
