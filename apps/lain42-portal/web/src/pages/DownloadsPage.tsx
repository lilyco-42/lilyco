import { useEffect, useState } from "react";
import {
  Check,
  ChevronDown,
  ChevronUp,
  Copy,
  Download,
  ExternalLink,
  Globe,
  Search,
  Server,
  Smartphone,
  Star,
  Terminal,
  Zap,
} from "lucide-react";
import { apiFetch, type DownloadItem } from "../lib/api";
import { Badge, Button, Card, Input } from "../components/ui/primitives";

const CATS = [
  { id: "all", label: "全部" },
  { id: "creative", label: "设计 / Adobe 平替" },
  { id: "office", label: "办公 / OCR / 笔记" },
  { id: "oss", label: "10k+ 开源 / 运行环境" },
  { id: "self", label: "云枢自研" },
  { id: "game", label: "游戏" },
  { id: "video", label: "影视" },
  { id: "music", label: "音乐" },
  { id: "ebook", label: "电子书" },
];

const MIRRORS = [
  { id: "auto", label: "⚡ 智能加速" },
  { id: "aliyun", label: "阿里云 / npmmirror" },
  { id: "tuna", label: "清华 TUNA" },
  { id: "ustc", label: "中科大 USTC" },
  { id: "bfsu", label: "北外 BFSU" },
  { id: "tencent", label: "腾讯云" },
  { id: "bytedance", label: "字节跳动 rsproxy" },
  { id: "quark", label: "夸克网盘" },
  { id: "official", label: "GitHub 官方" },
];

interface MirrorEntry {
  id: string;
  name: string;
  scope: string;
  url: string;
  supports: string[];
}

interface MirrorRegistry {
  storage_policy: string;
  chsrc: {
    repo: string;
    linux_macos: string;
    windows_ps: string;
    examples: string[];
  };
  mirrors: MirrorEntry[];
}

function detectClientTarget(): string {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("android")) return "android:arm64";
  if (ua.includes("windows")) {
    return ua.includes("arm64") ? "win:arm64" : "win:x64";
  }
  if (ua.includes("mac os") || ua.includes("macintosh")) {
    return "mac:arm64";
  }
  return ua.includes("aarch64") || ua.includes("arm64")
    ? "linux:arm64"
    : "linux:x64";
}

export function DownloadsPage() {
  const [items, setItems] = useState<DownloadItem[]>([]);
  const [registry, setRegistry] = useState<MirrorRegistry | null>(null);
  const [cat, setCat] = useState("all");
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(36);
  const [mirror, setMirror] = useState("auto");
  const [target, setTarget] = useState<string>(() => detectClientTarget());
  const [showMirrors, setShowMirrors] = useState(false);
  const [copiedId, setCopiedId] = useState("");

  useEffect(() => {
    apiFetch<{ items?: DownloadItem[] } | DownloadItem[]>("/api/downloads")
      .then((res) => {
        const list = Array.isArray(res) ? res : res.items || [];
        setItems(list);
      })
      .catch(() => {});
    apiFetch<MirrorRegistry>("/api/mirrors")
      .then((res) => setRegistry(res))
      .catch(() => {});
  }, []);

  const [os, arch] = target.split(":");

  const filtered = items.filter((it) => {
    if (cat !== "all" && it.category !== cat) return false;
    if (query.trim()) {
      const q = query.toLowerCase();
      return (
        it.name.toLowerCase().includes(q) ||
        it.repo.toLowerCase().includes(q) ||
        it.summary.toLowerCase().includes(q) ||
        it.slug.toLowerCase().includes(q)
      );
    }
    return true;
  });

  const shown = filtered.slice(0, limit);

  function copyCmd(id: string, text: string) {
    navigator.clipboard.writeText(text).then(() => {
      setCopiedId(id);
      setTimeout(() => setCopiedId(""), 1800);
    });
  }

  return (
    <div className="space-y-5">
      {/* Header */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant="emerald">零本站存储 · 智能国内镜像重定向</Badge>
            <Badge variant="outline">chsrc + 夸克 + Wrangler</Badge>
          </div>
          <h1 className="mt-2 text-2xl font-bold tracking-tight">
            开源与自研高速下载中心
          </h1>
          <p className="mt-1 text-sm text-muted-foreground">
            共收录 {items.length} 项精选资源（自动识别您的系统架构，实时解析最新
            Release 并重定向至阿里云 / TUNA / 腾讯云 / 字节跳动 / F-Droid / 夸克网盘）。
          </p>
        </div>
        <div className="relative w-full sm:w-72">
          <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" />
          <Input
            className="pl-9"
            placeholder="搜索 node, obs, snow-shot, umi-ocr..."
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setLimit(36);
            }}
          />
        </div>
      </div>

      {/* Smart Mirror + OS/Arch Hook Control Bar */}
      <Card className="p-4 space-y-3 border-emerald-500/25 bg-emerald-500/[0.03]">
        <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Zap className="h-4 w-4 text-emerald-500 shrink-0" />
            <span>智能镜像 Hook 与平台自动识别</span>
            <select
              value={target}
              onChange={(e) => setTarget(e.target.value)}
              className="ml-1 h-8 rounded-md border border-input bg-background px-2.5 text-xs font-mono"
            >
              <option value="win:x64">Windows x86_64 (.msi / .exe)</option>
              <option value="win:arm64">Windows ARM64 (.msi / .exe)</option>
              <option value="mac:arm64">macOS Apple Silicon (.dmg / .pkg)</option>
              <option value="mac:x64">macOS Intel (.dmg / .pkg)</option>
              <option value="linux:x64">Linux x86_64 (.AppImage / .tar.xz)</option>
              <option value="linux:arm64">Linux ARM64 (.tar.xz / .deb)</option>
              <option value="android:arm64">Android 手机端 (.apk / F-Droid)</option>
            </select>
          </div>

          <Button
            size="sm"
            variant="outline"
            onClick={() => setShowMirrors((s) => !s)}
          >
            <Server className="h-3.5 w-3.5" />
            全部国内镜像源与 chsrc 一键换源
            {showMirrors ? (
              <ChevronUp className="h-3.5 w-3.5" />
            ) : (
              <ChevronDown className="h-3.5 w-3.5" />
            )}
          </Button>
        </div>

        {/* Mirror Selector Pills */}
        <div className="flex flex-wrap gap-1.5">
          {MIRRORS.map((m) => (
            <Button
              key={m.id}
              size="sm"
              variant={mirror === m.id ? "emerald" : "outline"}
              onClick={() => setMirror(m.id)}
            >
              {m.label}
            </Button>
          ))}
        </div>

        {/* Expandable Full Supported Mirrors & chsrc Panel */}
        {showMirrors && registry && (
          <div className="pt-3 border-t space-y-4">
            <p className="text-xs text-muted-foreground">
              {registry.storage_policy}
            </p>

            {/* chsrc Quick CLI */}
            <div className="grid gap-3 md:grid-cols-2">
              <div className="rounded-lg border bg-background p-3 space-y-1.5">
                <div className="flex items-center justify-between text-xs font-medium">
                  <span className="flex items-center gap-1.5">
                    <Terminal className="h-3.5 w-3.5 text-emerald-500" />
                    Linux / macOS 一键安装 chsrc 换源
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-6 px-2 text-xs"
                    onClick={() => copyCmd("chsrc-posix", registry.chsrc.linux_macos)}
                  >
                    {copiedId === "chsrc-posix" ? (
                      <Check className="h-3 w-3 text-emerald-500" />
                    ) : (
                      <Copy className="h-3 w-3" />
                    )}
                  </Button>
                </div>
                <code className="block rounded bg-muted px-2 py-1 text-[11px] font-mono">
                  {registry.chsrc.linux_macos}
                </code>
              </div>

              <div className="rounded-lg border bg-background p-3 space-y-1.5">
                <div className="flex items-center justify-between text-xs font-medium">
                  <span className="flex items-center gap-1.5">
                    <Terminal className="h-3.5 w-3.5 text-amber-500" />
                    Windows PowerShell 一键安装 chsrc
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-6 px-2 text-xs"
                    onClick={() => copyCmd("chsrc-win", registry.chsrc.windows_ps)}
                  >
                    {copiedId === "chsrc-win" ? (
                      <Check className="h-3 w-3 text-emerald-500" />
                    ) : (
                      <Copy className="h-3 w-3" />
                    )}
                  </Button>
                </div>
                <code className="block rounded bg-muted px-2 py-1 text-[11px] font-mono">
                  {registry.chsrc.windows_ps}
                </code>
              </div>
            </div>

            {/* All Supported Mirrors Grid (Desktop + Mobile F-Droid + Quark + Wrangler) */}
            <div className="grid gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
              {registry.mirrors.map((m) => (
                <div
                  key={m.id}
                  className="rounded-lg border bg-background p-3 space-y-1.5"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-xs font-semibold flex items-center gap-1.5">
                      {m.id === "fdroid" ? (
                        <Smartphone className="h-3.5 w-3.5 text-emerald-500" />
                      ) : (
                        <Globe className="h-3.5 w-3.5 text-emerald-500" />
                      )}
                      {m.name}
                    </span>
                    <Badge variant="secondary" className="text-[10px]">
                      {m.scope}
                    </Badge>
                  </div>
                  <a
                    href={m.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="block truncate font-mono text-[11px] text-emerald-600 dark:text-emerald-400 hover:underline"
                  >
                    {m.url}
                  </a>
                  <div className="flex flex-wrap gap-1 pt-0.5">
                    {m.supports.map((s) => (
                      <span
                        key={s}
                        className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground"
                      >
                        {s}
                      </span>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
      </Card>

      {/* Category Tabs */}
      <div className="flex flex-wrap gap-2">
        {CATS.map((c) => {
          const count =
            c.id === "all"
              ? items.length
              : items.filter((i) => i.category === c.id).length;
          return (
            <Button
              key={c.id}
              size="sm"
              variant={cat === c.id ? "default" : "outline"}
              onClick={() => {
                setCat(c.id);
                setLimit(36);
              }}
            >
              {c.label} ({count})
            </Button>
          );
        })}
      </div>

      {/* Grid */}
      <div className="grid gap-3.5 sm:grid-cols-2 lg:grid-cols-3">
        {shown.map((it) => {
          const mirrorHref = `/go/mirror/${encodeURIComponent(
            it.slug
          )}?mirror=${encodeURIComponent(mirror)}&os=${encodeURIComponent(
            os
          )}&arch=${encodeURIComponent(arch)}`;
          return (
            <Card key={it.slug} className="flex flex-col justify-between p-4">
              <div>
                <div className="flex items-start justify-between gap-2">
                  <a
                    href={`/downloads/${it.slug}`}
                    className="text-sm font-semibold hover:text-emerald-600 dark:hover:text-emerald-400"
                  >
                    {it.name}
                  </a>
                  {it.stars > 0 && (
                    <Badge variant="secondary" className="shrink-0 gap-1">
                      <Star className="h-3 w-3 fill-amber-400 text-amber-400" />
                      {it.stars >= 1000
                        ? `${(it.stars / 1000).toFixed(1)}k`
                        : it.stars}
                    </Badge>
                  )}
                </div>
                <div className="mt-1 flex items-center gap-2 text-[11px] text-muted-foreground">
                  <span className="truncate font-mono">{it.repo}</span>
                  <span>·</span>
                  <span>{it.version}</span>
                </div>
                <p className="mt-2 line-clamp-2 text-xs leading-relaxed text-muted-foreground">
                  {it.summary}
                </p>
              </div>
              <div className="mt-4 flex items-center gap-1.5 border-t pt-3">
                <a
                  href={mirrorHref}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex h-8 flex-1 items-center justify-center gap-1 rounded-md bg-emerald-600 px-2.5 text-xs font-medium text-white hover:bg-emerald-700"
                >
                  <Zap className="h-3.5 w-3.5" /> 镜像加速下载
                </a>
                {it.quark_url && (
                  <a
                    href={`/go/download/${it.slug}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex h-8 items-center justify-center gap-1 rounded-md border border-emerald-500/40 bg-emerald-500/10 px-2.5 text-xs font-medium text-emerald-700 dark:text-emerald-300 hover:bg-emerald-500/20"
                  >
                    <Download className="h-3 w-3" /> 夸克
                  </a>
                )}
                <a
                  href={`/downloads/${it.slug}`}
                  className="inline-flex h-8 items-center justify-center rounded-md border border-input px-2 text-xs font-medium hover:bg-accent"
                >
                  详情 <ExternalLink className="ml-1 h-3 w-3" />
                </a>
              </div>
            </Card>
          );
        })}
      </div>

      {filtered.length > limit && (
        <div className="flex justify-center pt-2">
          <Button variant="outline" onClick={() => setLimit((n) => n + 36)}>
            加载更多（剩余 {filtered.length - limit} 项）
          </Button>
        </div>
      )}
    </div>
  );
}
