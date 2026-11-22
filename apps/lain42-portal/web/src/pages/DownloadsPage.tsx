import { useCallback, useEffect, useState } from "react";
import {
  Cloud,
  ExternalLink,
  Search,
  Smartphone,
  Star,
  X,
  Zap,
} from "lucide-react";
import { QRCodeSVG } from "qrcode.react";
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

function trackBehavior(
  slug: string,
  channel: "quark" | "quark_qr" | "mirror" | "page_view",
  mirror: string,
  os: string,
  arch: string
) {
  const payload = JSON.stringify({ slug, channel, mirror, os, arch });
  if (navigator.sendBeacon) {
    navigator.sendBeacon(
      "/api/analytics/event",
      new Blob([payload], { type: "application/json" })
    );
  } else {
    fetch("/api/analytics/event", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: payload,
      keepalive: true,
    }).catch(() => {});
  }
}

export function DownloadsPage() {
  const [items, setItems] = useState<DownloadItem[]>([]);
  const [cat, setCat] = useState("all");
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(36);
  const [mirror, setMirror] = useState<string>(
    () => localStorage.getItem("lain42_mirror") || "tuna"
  );
  const [ghProxy, setGhProxy] = useState<string>(() => {
    const saved = localStorage.getItem("lain42_ghproxy") || "ghfast.top";
    return saved === "wget.la" ? "ghfast.top" : saved;
  });
  const [target, setTarget] = useState<string>(() => detectClientTarget());
  const [quarkModal, setQuarkModal] = useState<DownloadItem | null>(null);

  const [os, arch] = target.split(":");

  useEffect(() => {
    trackBehavior("downloads_page", "page_view", mirror, os, arch);
    apiFetch<{ items?: DownloadItem[] } | DownloadItem[]>("/api/downloads")
      .then((res) => {
        const list = Array.isArray(res) ? res : res.items || [];
        setItems(list);
      })
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleSelectMirror = useCallback((m: string) => {
    setMirror(m);
    localStorage.setItem("lain42_mirror", m);
  }, []);

  const handleSelectGhProxy = useCallback((host: string) => {
    setGhProxy(host);
    localStorage.setItem("lain42_ghproxy", host);
  }, []);

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

  function openQuarkExpress(it: DownloadItem) {
    trackBehavior(it.slug, "quark_qr", "quark_modal", os, arch);
    setQuarkModal(it);
  }

  return (
    <div className="space-y-5">
      {/* Header */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant="amber">⚡ 极速网盘免密秒存</Badge>
            <Badge variant="emerald">chsrc Web 真高速镜像</Badge>
          </div>
          <h1 className="mt-2 text-2xl font-bold tracking-tight">
            开源与自研高速下载中心
          </h1>
          <p className="mt-1 text-sm text-muted-foreground">
            共收录 {items.length} 项精选资源（含 Craft 纯 Rust Adobe/Office 19 合 1、OBS Studio、SnowShot、Umi-OCR），默认推荐极速网盘全平台秒存，同时提供国内真高速镜像源。
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
              placeholder="搜索 photocraft, obs, node, umi-ocr..."
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
          const hasQuark = Boolean(it.quark_url && it.quark_url.trim());
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

              {/* P0 Primary/Secondary Conversion Funnel */}
              <div className="mt-4 space-y-2 border-t pt-3">
                {hasQuark ? (
                  <>
                    <button
                      type="button"
                      onClick={() => openQuarkExpress(it)}
                      className="inline-flex h-9 w-full items-center justify-center gap-1.5 rounded-md bg-gradient-to-r from-orange-500 to-amber-500 px-3 text-xs font-semibold text-white shadow-xs hover:from-orange-600 hover:to-amber-600 transition-all cursor-pointer"
                    >
                      <Cloud className="h-3.5 w-3.5" />
                      极速网盘下载（夸克免密秒存）
                    </button>
                    <div className="flex items-center justify-between text-[11px] text-muted-foreground px-0.5">
                      <a
                        href={mirrorHref}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="hover:text-foreground underline-offset-2 hover:underline"
                      >
                        普通镜像源下载
                      </a>
                      <a
                        href={`/downloads/${it.slug}`}
                        className="inline-flex items-center gap-0.5 hover:text-foreground"
                      >
                        详情 <ExternalLink className="h-2.5 w-2.5" />
                      </a>
                    </div>
                  </>
                ) : (
                  <div className="flex items-center gap-1.5">
                    <a
                      href={mirrorHref}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex h-8 flex-1 items-center justify-center gap-1 rounded-md bg-emerald-600 px-2.5 text-xs font-medium text-white hover:bg-emerald-700"
                    >
                      <Zap className="h-3.5 w-3.5" /> 高速镜像源下载
                    </a>
                    <a
                      href={`/downloads/${it.slug}`}
                      className="inline-flex h-8 items-center justify-center rounded-md border border-input px-2 text-xs font-medium hover:bg-accent"
                    >
                      详情 <ExternalLink className="ml-1 h-3 w-3" />
                    </a>
                  </div>
                )}
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

      {/* P1 Express Quark Download + Mobile QR Code Modal */}
      {quarkModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
          <div
            className="fixed inset-0 bg-black/60 backdrop-blur-xs"
            onClick={() => setQuarkModal(null)}
          />
          <Card className="relative z-10 w-full max-w-lg p-6 space-y-5 shadow-2xl border-orange-500/30">
            <div className="flex items-start justify-between gap-3">
              <div>
                <Badge variant="amber">⚡ 极速网盘通道 · 免代理满速</Badge>
                <h3 className="mt-2 text-lg font-bold">{quarkModal.name}</h3>
                <p className="text-xs text-muted-foreground mt-0.5 font-mono">
                  {quarkModal.repo} · {quarkModal.version}
                </p>
              </div>
              <Button
                variant="ghost"
                size="icon"
                onClick={() => setQuarkModal(null)}
              >
                <X className="h-4 w-4" />
              </Button>
            </div>

            <div className="grid gap-4 sm:grid-cols-5 items-center">
              <div className="sm:col-span-3 space-y-3">
                <p className="text-xs text-muted-foreground leading-relaxed">
                  已为您打包全平台纯净原版安装包，存入夸克网盘即可永久免代理满速下载，手机/电脑均可随时提取。
                </p>
                <a
                  href={`/go/download/${encodeURIComponent(quarkModal.slug)}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex h-10 w-full items-center justify-center gap-2 rounded-md bg-gradient-to-r from-orange-500 to-amber-500 px-4 text-sm font-semibold text-white shadow-sm hover:from-orange-600 hover:to-amber-600 transition-all"
                >
                  <Cloud className="h-4 w-4" />
                  立即打开夸克网盘秒存
                </a>
                <div className="text-[11px] text-muted-foreground">
                  国内直连免梯子 · 免费无提取码
                </div>
              </div>

              <div className="sm:col-span-2 flex flex-col items-center justify-center rounded-lg border bg-white p-3 text-center">
                <QRCodeSVG
                  value={quarkModal.quark_url}
                  size={124}
                  level="M"
                  includeMargin={false}
                />
                <span className="mt-2 inline-flex items-center gap-1 text-[11px] font-medium text-zinc-700">
                  <Smartphone className="h-3 w-3 text-orange-500" />
                  手机夸克扫码秒存
                </span>
              </div>
            </div>

            <div className="flex items-center justify-between border-t pt-3 text-xs text-muted-foreground">
              <span>不想用网盘？</span>
              <a
                href={`/go/mirror/${encodeURIComponent(
                  quarkModal.slug
                )}?mirror=${encodeURIComponent(
                  mirror
                )}&ghproxy=${encodeURIComponent(ghProxy)}&os=${encodeURIComponent(
                  os
                )}&arch=${encodeURIComponent(arch)}`}
                target="_blank"
                rel="noopener noreferrer"
                className="text-muted-foreground hover:text-foreground underline underline-offset-4"
              >
                改用普通镜像源下载（TUNA / 阿里云 / GitHub）
              </a>
            </div>
          </Card>
        </div>
      )}
    </div>
  );
}
