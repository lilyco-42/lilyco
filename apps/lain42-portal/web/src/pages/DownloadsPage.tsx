import { useCallback, useEffect, useState } from "react";
import { Download, ExternalLink, Search, Star, Zap } from "lucide-react";
import { apiFetch, type DownloadItem } from "../lib/api";
import { Badge, Button, Card, Input } from "../components/ui/primitives";
import { ChsrcWebPanel } from "../components/ChsrcWebPanel";

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
  const [cat, setCat] = useState("all");
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(36);
  const [mirror, setMirror] = useState<string>(
    () => localStorage.getItem("lain42_mirror") || "aliyun"
  );
  const [ghProxy, setGhProxy] = useState<string>(
    () => localStorage.getItem("lain42_ghproxy") || "cdn.gh-proxy.org"
  );
  const [target, setTarget] = useState<string>(() => detectClientTarget());

  useEffect(() => {
    apiFetch<{ items?: DownloadItem[] } | DownloadItem[]>("/api/downloads")
      .then((res) => {
        const list = Array.isArray(res) ? res : res.items || [];
        setItems(list);
      })
      .catch(() => {});
  }, []);

  const handleSelectMirror = useCallback((m: string) => {
    setMirror(m);
    localStorage.setItem("lain42_mirror", m);
  }, []);

  const handleSelectGhProxy = useCallback((host: string) => {
    setGhProxy(host);
    localStorage.setItem("lain42_ghproxy", host);
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

  return (
    <div className="space-y-5">
      {/* Header */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant="emerald">chsrc Web · 零本站存储 · 智能测速换源</Badge>
            <Badge variant="outline">夸克网盘 + Cloudflare Wrangler</Badge>
          </div>
          <h1 className="mt-2 text-2xl font-bold tracking-tight">
            开源与自研高速下载中心
          </h1>
          <p className="mt-1 text-sm text-muted-foreground">
            共收录 {items.length} 项精选资源，由您本机浏览器实时测速 8 大国内镜像站与 4 组 GitHub 加速节点并自动择优重定向。
          </p>
        </div>
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
          <select
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            className="h-9 rounded-md border border-input bg-background px-2.5 text-xs font-mono"
            title="自动识别客户端系统与 CPU 架构"
          >
            <option value="win:x64">Windows x86_64 (.msi / .exe)</option>
            <option value="win:arm64">Windows ARM64 (.msi / .exe)</option>
            <option value="mac:arm64">macOS Apple Silicon (.dmg / .pkg)</option>
            <option value="mac:x64">macOS Intel (.dmg / .pkg)</option>
            <option value="linux:x64">Linux x86_64 (.AppImage / .tar.xz)</option>
            <option value="linux:arm64">Linux ARM64 (.tar.xz / .deb)</option>
            <option value="android:arm64">Android 手机端 (.apk / F-Droid)</option>
          </select>
          <div className="relative w-full sm:w-64">
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
      </div>

      {/* chsrc Web Real-Time Browser Speedtest & Smart Mirror Switcher */}
      <ChsrcWebPanel
        activeMirror={mirror}
        activeGhProxy={ghProxy}
        onSelectMirror={handleSelectMirror}
        onSelectGhProxy={handleSelectGhProxy}
      />

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
          )}?mirror=${encodeURIComponent(mirror)}&ghproxy=${encodeURIComponent(
            ghProxy
          )}&os=${encodeURIComponent(os)}&arch=${encodeURIComponent(arch)}`;
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
                  <Zap className="h-3.5 w-3.5" /> 最快镜像下载
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
