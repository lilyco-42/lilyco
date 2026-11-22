import { useEffect, useState } from "react";
import { Download, ExternalLink, Search, Star } from "lucide-react";
import { apiFetch, type DownloadItem } from "../lib/api";
import { Badge, Button, Card, Input } from "../components/ui/primitives";

const CATS = [
  { id: "all", label: "全部" },
  { id: "creative", label: "设计 / Adobe 平替" },
  { id: "office", label: "办公 / OCR / 笔记" },
  { id: "oss", label: "10k+ 开源效率" },
  { id: "self", label: "云枢自研" },
  { id: "game", label: "游戏" },
  { id: "video", label: "影视" },
  { id: "music", label: "音乐" },
  { id: "ebook", label: "电子书" },
];

export function DownloadsPage() {
  const [items, setItems] = useState<DownloadItem[]>([]);
  const [cat, setCat] = useState("all");
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(36);

  useEffect(() => {
    apiFetch<{ items?: DownloadItem[] } | DownloadItem[]>("/api/downloads")
      .then((res) => {
        const list = Array.isArray(res) ? res : res.items || [];
        setItems(list);
      })
      .catch(() => {});
  }, []);

  const filtered = items.filter((it) => {
    if (cat !== "all" && it.category !== cat) return false;
    if (query.trim()) {
      const q = query.toLowerCase();
      return (
        it.name.toLowerCase().includes(q) ||
        it.repo.toLowerCase().includes(q) ||
        it.summary.toLowerCase().includes(q)
      );
    }
    return true;
  });

  const shown = filtered.slice(0, limit);

  return (
    <div className="space-y-5">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <Badge variant="emerald">全部免费 · 10k+ 星开源 Release + 夸克高速镜像</Badge>
          <h1 className="mt-2 text-2xl font-bold tracking-tight">开源与自研高速下载中心</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            共收录 {items.length} 项精选资源（含 OBS Studio 多版本、SnowShot、Umi-OCR、Office/Adobe 平替与 10k+ 星 GitHub 最新 Release）。
          </p>
        </div>
        <div className="relative w-full sm:w-72">
          <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" />
          <Input
            className="pl-9"
            placeholder="搜索项目名、仓库或关键词..."
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setLimit(36);
            }}
          />
        </div>
      </div>

      {/* Category Tabs */}
      <div className="flex flex-wrap gap-2">
        {CATS.map((c) => {
          const count =
            c.id === "all" ? items.length : items.filter((i) => i.category === c.id).length;
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
        {shown.map((it) => (
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
                    {it.stars >= 1000 ? `${(it.stars / 1000).toFixed(1)}k` : it.stars}
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
            <div className="mt-4 flex items-center gap-2 border-t pt-3">
              {it.quark_url ? (
                <a
                  href={`/go/download/${it.slug}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex h-8 flex-1 items-center justify-center gap-1.5 rounded-md bg-emerald-600 px-3 text-xs font-medium text-white hover:bg-emerald-700"
                >
                  <Download className="h-3.5 w-3.5" /> 夸克免费高速下载
                </a>
              ) : (
                <a
                  href={it.github_download_url || it.github_release_url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex h-8 flex-1 items-center justify-center gap-1.5 rounded-md bg-emerald-600 px-3 text-xs font-medium text-white hover:bg-emerald-700"
                >
                  <Download className="h-3.5 w-3.5" /> 下载最新 Release
                </a>
              )}
              <a
                href={`/downloads/${it.slug}`}
                className="inline-flex h-8 items-center justify-center rounded-md border border-input px-2.5 text-xs font-medium hover:bg-accent"
              >
                详情 <ExternalLink className="ml-1 h-3 w-3" />
              </a>
            </div>
          </Card>
        ))}
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
