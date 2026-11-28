import { useEffect, useMemo, useState } from "react";
import {
  Cloud,
  Download,
  ExternalLink,
  HardDriveDownload,
  Layers,
  Loader2,
  X,
  Zap,
} from "lucide-react";
import {
  apiFetch,
  type DownloadItem,
  type ReleaseAssetsResponse,
  type ResolvedAsset,
} from "../lib/api";
import { Badge, Button, Card } from "./ui/primitives";

type PlatformTabId = "win:x64" | "win:arm64" | "mac" | "linux" | "android" | "all";

interface MultiChannelDownloadModalProps {
  item: DownloadItem;
  ghProxy: string;
  initialOs: string;
  initialArch: string;
  onClose: () => void;
  onTrack?: (channel: "quark" | "mirror", mirrorLabel: string, os: string, arch: string) => void;
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "";
  if (bytes >= 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(0)} KB`;
  }
  return `${bytes} B`;
}

function defaultTabForClient(os: string, arch: string): PlatformTabId {
  if (os === "win") return arch === "arm64" ? "win:arm64" : "win:x64";
  if (os === "mac") return "mac";
  if (os === "linux") return "linux";
  if (os === "android") return "android";
  return "all";
}

function matchesTab(asset: ResolvedAsset, tab: PlatformTabId): boolean {
  if (tab === "all") return true;
  if (tab === "win:x64") {
    return asset.os === "win" && (asset.arch === "x64" || asset.arch === "universal" || asset.arch === "x86");
  }
  if (tab === "win:arm64") {
    return asset.os === "win" && (asset.arch === "arm64" || asset.arch === "universal");
  }
  return asset.os === tab;
}

function osLabel(os: string): string {
  switch (os) {
    case "win":
      return "Windows";
    case "mac":
      return "macOS";
    case "linux":
      return "Linux";
    case "android":
      return "Android";
    case "ios":
      return "iOS";
    default:
      return "通用";
  }
}

function archLabel(arch: string): string {
  switch (arch) {
    case "x64":
      return "x86_64";
    case "arm64":
      return "ARM64";
    case "x86":
      return "32-bit";
    case "universal":
      return "通用架构";
    default:
      return arch;
  }
}

const TABS: { id: PlatformTabId; label: string }[] = [
  { id: "win:x64", label: "Windows x86_64" },
  { id: "win:arm64", label: "Windows ARM64" },
  { id: "mac", label: "macOS" },
  { id: "linux", label: "Linux" },
  { id: "android", label: "Android" },
  { id: "all", label: "全部文件" },
];

export function MultiChannelDownloadModal({
  item,
  ghProxy,
  initialOs,
  initialArch,
  onClose,
  onTrack,
}: MultiChannelDownloadModalProps) {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [data, setData] = useState<ReleaseAssetsResponse | null>(null);
  const [tab, setTab] = useState<PlatformTabId>(() =>
    defaultTabForClient(initialOs, initialArch)
  );
  const [showAuxiliary, setShowAuxiliary] = useState(false);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    apiFetch<{ ok: boolean; data?: ReleaseAssetsResponse }>(
      `/api/downloads/${encodeURIComponent(item.slug)}/assets?ghproxy=${encodeURIComponent(ghProxy)}`
    )
      .then((res) => {
        if (!active) return;
        const payload = res.data || null;
        setData(payload);
        if (payload && payload.assets.length > 0) {
          const preferred = defaultTabForClient(initialOs, initialArch);
          const primaryInPreferred = payload.assets.some(
            (a) => !a.is_auxiliary && matchesTab(a, preferred)
          );
          if (primaryInPreferred) {
            setTab(preferred);
          } else if (
            payload.assets.some((a) => !a.is_auxiliary && matchesTab(a, "win:x64"))
          ) {
            setTab("win:x64");
          } else {
            setTab("all");
          }
        }
      })
      .catch((err: unknown) => {
        if (!active) return;
        setError(err instanceof Error ? err.message : "拉取 Release 文件列表失败");
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [item.slug, ghProxy, initialOs, initialArch]);

  const allAssets = data?.assets ?? [];

  const tabCounts = useMemo(() => {
    const counts: Record<PlatformTabId, number> = {
      "win:x64": 0,
      "win:arm64": 0,
      mac: 0,
      linux: 0,
      android: 0,
      all: 0,
    };
    for (const a of allAssets) {
      if (!showAuxiliary && a.is_auxiliary) continue;
      counts.all += 1;
      for (const t of TABS) {
        if (t.id !== "all" && matchesTab(a, t.id)) {
          counts[t.id] += 1;
        }
      }
    }
    return counts;
  }, [allAssets, showAuxiliary]);

  const auxiliaryCount = useMemo(
    () => allAssets.filter((a) => a.is_auxiliary).length,
    [allAssets]
  );

  const visibleAssets = useMemo(
    () =>
      allAssets.filter(
        (a) => (showAuxiliary || !a.is_auxiliary) && matchesTab(a, tab)
      ),
    [allAssets, showAuxiliary, tab]
  );

  const hasQuark = Boolean((data?.quark_url || item.quark_url || "").trim());

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-4">
      <div
        className="fixed inset-0 bg-black/60 backdrop-blur-xs"
        onClick={onClose}
      />
      <Card className="relative z-10 flex max-h-[88vh] w-full max-w-3xl flex-col overflow-hidden shadow-2xl">
        {/* Header */}
        <div className="flex items-start justify-between gap-3 border-b p-4 sm:p-5">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant="emerald">
                <Layers className="mr-1 h-3 w-3" />
                全平台多渠道下载
              </Badge>
              <Badge variant="secondary" className="font-mono">
                {data?.version || item.version}
              </Badge>
              {data?.source === "github_live" && (
                <Badge variant="outline" className="text-[11px]">
                  GitHub Release 实时同步
                </Badge>
              )}
            </div>
            <h3 className="mt-2 truncate text-lg font-bold">{item.name}</h3>
            <p className="mt-0.5 truncate font-mono text-xs text-muted-foreground">
              {item.repo}
            </p>
          </div>
          <Button variant="ghost" size="icon" onClick={onClose} aria-label="关闭">
            <X className="h-4 w-4" />
          </Button>
        </div>

        {/* Platform Filter Tabs */}
        <div className="flex flex-wrap items-center justify-between gap-2 border-b bg-muted/30 px-4 py-2.5 sm:px-5">
          <div className="flex flex-wrap gap-1.5">
            {TABS.map((t) => {
              const cnt = tabCounts[t.id];
              if (t.id !== "all" && cnt === 0 && !loading) return null;
              return (
                <Button
                  key={t.id}
                  size="sm"
                  variant={tab === t.id ? "default" : "outline"}
                  className="h-7 px-2.5 text-xs"
                  onClick={() => setTab(t.id)}
                >
                  {t.label}
                  {!loading && <span className="ml-1 opacity-75">({cnt})</span>}
                </Button>
              );
            })}
          </div>
          {auxiliaryCount > 0 && (
            <label className="flex cursor-pointer items-center gap-1.5 text-xs text-muted-foreground select-none">
              <input
                type="checkbox"
                checked={showAuxiliary}
                onChange={(e) => setShowAuxiliary(e.target.checked)}
                className="rounded border-input"
              />
              显示辅助组件 ({auxiliaryCount})
            </label>
          )}
        </div>

        {/* Body */}
        <div className="flex-1 space-y-3 overflow-y-auto p-4 sm:p-5">
          {loading ? (
            <div className="flex flex-col items-center justify-center gap-2 py-12 text-sm text-muted-foreground">
              <Loader2 className="h-5 w-5 animate-spin text-emerald-500" />
              <span>正在实时解析 GitHub Release 全平台安装包...</span>
            </div>
          ) : error ? (
            <div className="rounded-lg border border-destructive/40 bg-destructive/5 p-4 text-xs text-destructive">
              {error}
            </div>
          ) : visibleAssets.length === 0 ? (
            <div className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
              <p>当前平台筛选下暂无匹配的二进制安装包。</p>
              <div className="mt-3 flex flex-wrap justify-center gap-2">
                {tab !== "all" && allAssets.length > 0 && (
                  <Button size="sm" variant="outline" onClick={() => setTab("all")}>
                    查看全部 {allAssets.length} 个文件
                  </Button>
                )}
                <a
                  href={data?.release_url || item.github_release_url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex h-8 items-center gap-1 rounded-md border border-input px-3 text-xs font-medium hover:bg-accent"
                >
                  打开 GitHub Releases <ExternalLink className="h-3 w-3" />
                </a>
              </div>
            </div>
          ) : (
            <div className="space-y-2.5">
              {visibleAssets.map((asset) => (
                <div
                  key={asset.name}
                  className="rounded-lg border bg-card p-3.5 transition-colors hover:border-emerald-500/40"
                >
                  <div className="flex flex-wrap items-start justify-between gap-2">
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <HardDriveDownload className="h-4 w-4 shrink-0 text-emerald-500" />
                        <span className="break-all font-mono text-xs font-semibold sm:text-sm">
                          {asset.name}
                        </span>
                      </div>
                      <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
                        <Badge variant="secondary" className="h-5 px-1.5 text-[10px]">
                          {osLabel(asset.os)} · {archLabel(asset.arch)}
                        </Badge>
                        {asset.ext && (
                          <Badge variant="outline" className="h-5 px-1.5 font-mono text-[10px]">
                            {asset.ext}
                          </Badge>
                        )}
                        {asset.size_bytes > 0 && (
                          <span className="font-mono">{formatBytes(asset.size_bytes)}</span>
                        )}
                        {asset.is_auxiliary && (
                          <Badge variant="amber" className="h-5 px-1.5 text-[10px]">
                            辅助组件
                          </Badge>
                        )}
                      </div>
                    </div>
                  </div>

                  {/* Multi-channel buttons */}
                  <div className="mt-3 flex flex-wrap gap-2">
                    {asset.mirrors.map((m) => (
                      <a
                        key={m.id}
                        href={m.url}
                        target="_blank"
                        rel="noopener noreferrer"
                        onClick={() =>
                          onTrack?.("mirror", `${m.id}:${asset.name}`, asset.os, asset.arch)
                        }
                        className={
                          m.recommended
                            ? "inline-flex h-8 items-center gap-1.5 rounded-md bg-emerald-600 px-3 text-xs font-medium text-white shadow-xs transition-colors hover:bg-emerald-700"
                            : "inline-flex h-8 items-center gap-1.5 rounded-md border border-input bg-background px-2.5 text-xs font-medium transition-colors hover:bg-accent"
                        }
                      >
                        {m.recommended ? (
                          <Zap className="h-3.5 w-3.5" />
                        ) : (
                          <Download className="h-3.5 w-3.5 opacity-70" />
                        )}
                        {m.label}
                      </a>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Footer: Quark Pan + GitHub Releases */}
        <div className="flex flex-wrap items-center justify-between gap-2 border-t bg-muted/20 px-4 py-3 sm:px-5">
          <div className="flex flex-wrap items-center gap-2">
            {hasQuark && (
              <a
                href={`/go/download/${encodeURIComponent(item.slug)}`}
                target="_blank"
                rel="noopener noreferrer"
                onClick={() => onTrack?.("quark", "quark_modal_footer", initialOs, initialArch)}
                className="inline-flex h-8 items-center gap-1.5 rounded-md bg-gradient-to-r from-orange-500 to-amber-500 px-3 text-xs font-semibold text-white shadow-xs hover:from-orange-600 hover:to-amber-600"
              >
                <Cloud className="h-3.5 w-3.5" />
                夸克网盘打包下载（免密）
              </a>
            )}
            <a
              href={data?.release_url || item.github_release_url}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex h-8 items-center gap-1 rounded-md border border-input px-2.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
            >
              GitHub Release 原页 <ExternalLink className="h-3 w-3" />
            </a>
          </div>
          <Button size="sm" variant="ghost" onClick={onClose}>
            关闭
          </Button>
        </div>
      </Card>
    </div>
  );
}
