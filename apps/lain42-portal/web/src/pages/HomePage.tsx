import { useEffect, useState } from "react";
import {
  Activity,
  ArrowUpRight,
  Cpu,
  Download,
  Globe,
  Megaphone,
  ShieldCheck,
  ShoppingBag,
  Sparkles,
  Zap,
} from "lucide-react";
import { apiFetch, type Announcement, type ProxyQualityNode } from "../lib/api";
import { Badge, Button, Card } from "../components/ui/primitives";

interface HomePageProps {
  onNavigate: (tab: string) => void;
}

export function HomePage({ onNavigate }: HomePageProps) {
  const [announcements, setAnnouncements] = useState<Announcement[]>([]);
  const [nodes, setNodes] = useState<ProxyQualityNode[]>([]);
  const [dlTotal, setDlTotal] = useState<number>(538);

  useEffect(() => {
    apiFetch<Announcement[]>("/api/announcements")
      .then((res) => setAnnouncements(Array.isArray(res) ? res : []))
      .catch(() => {});
    apiFetch<{ nodes: ProxyQualityNode[] }>("/api/proxy/quality")
      .then((res) => setNodes(res.nodes || []))
      .catch(() => {});
    apiFetch<{ total?: number; items?: unknown[] }>("/api/downloads")
      .then((res) => {
        if (typeof res.total === "number") setDlTotal(res.total);
        else if (Array.isArray(res.items)) setDlTotal(res.items.length);
      })
      .catch(() => {});
  }, []);

  const topSpeedMb = nodes.length
    ? (Math.max(...nodes.map((n) => n.peak_kbps)) / 1024).toFixed(1)
    : "12.4";

  return (
    <div className="space-y-6">
      {/* Hero Banner */}
      <Card className="relative overflow-hidden border-emerald-500/20 bg-gradient-to-br from-card via-card to-emerald-500/5 p-6 md:p-8">
        <div className="flex flex-col gap-5 md:flex-row md:items-center md:justify-between">
          <div className="max-w-2xl space-y-3">
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant="emerald" className="gap-1">
                <Sparkles className="h-3 w-3" /> Lain42 2.0 商业级控制台
              </Badge>
              <Badge variant="outline">React + shadcn/ui + Rust Turso</Badge>
            </div>
            <h1 className="text-2xl font-bold tracking-tight md:text-3xl">
              把想法做成能直接使用的产品
            </h1>
            <p className="text-sm leading-relaxed text-muted-foreground md:text-base">
              一个账户统一管理 OpenAI / Anthropic 双兼容 AI API、六大地区高速自建专线（峰值{" "}
              <span className="font-semibold text-foreground">{topSpeedMb} MB/s</span>，每 120s
              自动切最优线路）以及{" "}
              <span className="font-semibold text-foreground">{dlTotal}+</span>{" "}
              款自研与高星开源软件免费高速下载。
            </p>
            <div className="flex flex-wrap gap-2.5 pt-1">
              <Button variant="emerald" onClick={() => onNavigate("vpn")}>
                <Globe className="h-4 w-4" /> 代理专线 (¥15 / ¥30)
              </Button>
              <Button variant="default" onClick={() => onNavigate("shop")}>
                <ShoppingBag className="h-4 w-4" /> 商城与积分充值
              </Button>
              <Button variant="outline" onClick={() => onNavigate("downloads")}>
                <Download className="h-4 w-4" /> 免费下载中心 ({dlTotal})
              </Button>
              <a
                href="https://lain42.top/"
                className="inline-flex h-9 items-center gap-1 rounded-md border border-input px-3 text-xs font-medium text-muted-foreground hover:bg-accent hover:text-foreground"
              >
                返回经典版 lain42.top <ArrowUpRight className="h-3.5 w-3.5" />
              </a>
            </div>
          </div>
          <div className="grid grid-cols-2 gap-3 sm:min-w-[260px]">
            <div className="rounded-lg border bg-background/80 p-3.5">
              <div className="text-xs text-muted-foreground">专线峰值实测</div>
              <div className="mt-1 text-xl font-bold text-emerald-600 dark:text-emerald-400">
                {topSpeedMb} MB/s
              </div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">AnyTLS / Hy2 跳跃</div>
            </div>
            <div className="rounded-lg border bg-background/80 p-3.5">
              <div className="text-xs text-muted-foreground">月卡两档区分</div>
              <div className="mt-1 text-xl font-bold">¥15 / ¥30</div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">100GB / 220GB</div>
            </div>
            <div className="rounded-lg border bg-background/80 p-3.5">
              <div className="text-xs text-muted-foreground">免费开源库</div>
              <div className="mt-1 text-xl font-bold">{dlTotal}+</div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">夸克网盘免密直连</div>
            </div>
            <div className="rounded-lg border bg-background/80 p-3.5">
              <div className="text-xs text-muted-foreground">AI API 汇率</div>
              <div className="mt-1 text-xl font-bold">1元 = 10分</div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">上游失败自动退款</div>
            </div>
          </div>
        </div>
      </Card>

      {/* Announcements */}
      {announcements.length > 0 && (
        <div className="space-y-3">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Megaphone className="h-4 w-4 text-emerald-500" />
            最新站点公告与测速报告
          </div>
          <div className="grid gap-3 md:grid-cols-2">
            {announcements.slice(0, 4).map((a) => (
              <Card key={a.id} className="p-4 transition-colors hover:border-emerald-500/40">
                <div className="flex items-center justify-between gap-2">
                  <Badge variant={a.pinned ? "emerald" : "secondary"}>
                    {a.pinned ? "置顶公告" : "动态"} · {a.published_date}
                  </Badge>
                  {a.link_url && (
                    <a
                      href={a.link_url}
                      className="inline-flex items-center gap-1 text-xs font-medium text-emerald-600 hover:underline dark:text-emerald-400"
                    >
                      查看详情 <ArrowUpRight className="h-3 w-3" />
                    </a>
                  )}
                </div>
                <h3 className="mt-2.5 text-sm font-semibold leading-snug">{a.title}</h3>
                <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">{a.summary}</p>
              </Card>
            ))}
          </div>
        </div>
      )}

      {/* Feature Matrix */}
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Card
          onClick={() => onNavigate("vpn")}
          className="cursor-pointer p-5 transition-all hover:border-emerald-500/40 hover:shadow-sm"
        >
          <Globe className="h-5 w-5 text-emerald-500" />
          <h3 className="mt-3 font-semibold">LYCO Connect 高速专线</h3>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            韩/美/澳/德/菲 20+ 实测高速节点，支持 AnyTLS / Reality / Hy2 端口跳跃与 120s 自动切换。
          </p>
        </Card>
        <Card
          onClick={() => onNavigate("models")}
          className="cursor-pointer p-5 transition-all hover:border-emerald-500/40 hover:shadow-sm"
        >
          <Cpu className="h-5 w-5 text-blue-500" />
          <h3 className="mt-3 font-semibold">AI API · 模型与文档</h3>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            DeepSeek / GLM / Kimi / Qwen / Claude / 生图模型，OpenAI 与 Anthropic 原生双协议兼容。
          </p>
        </Card>
        <Card
          onClick={() => onNavigate("downloads")}
          className="cursor-pointer p-5 transition-all hover:border-emerald-500/40 hover:shadow-sm"
        >
          <Download className="h-5 w-5 text-violet-500" />
          <h3 className="mt-3 font-semibold">免费高速软件下载中心</h3>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            126 款云枢自研工具 + 412 款高星开源项目（游戏/影视/音乐/电子书/效率），夸克网盘永久免密。
          </p>
        </Card>
        <Card
          onClick={() => onNavigate("account")}
          className="cursor-pointer p-5 transition-all hover:border-emerald-500/40 hover:shadow-sm"
        >
          <ShieldCheck className="h-5 w-5 text-amber-500" />
          <h3 className="mt-3 font-semibold">双钱包与零信任风控</h3>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            充值积分（全站通用含代理）与赠送积分（软件/可复制资源）硬隔离，订单与权益透明可查。
          </p>
        </Card>
      </div>

      {/* Live Node Throughput Preview */}
      {nodes.length > 0 && (
        <Card className="p-5">
          <div className="mb-4 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Activity className="h-4 w-4 text-emerald-500" />
              <h2 className="text-sm font-semibold">自有专线实时吞吐排行（4MB 真实下载实测）</h2>
            </div>
            <Button variant="ghost" size="sm" onClick={() => onNavigate("vpn")}>
              全部节点与教程 <Zap className="h-3.5 w-3.5" />
            </Button>
          </div>
          <div className="grid gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
            {nodes.slice(0, 6).map((n) => {
              const pct = Math.min(100, Math.round((n.kbps / 10000) * 100));
              return (
                <div key={n.name} className="rounded-lg border bg-background p-3">
                  <div className="flex items-center justify-between text-xs">
                    <span className="font-medium truncate">{n.name}</span>
                    <Badge variant="emerald">{(n.kbps / 1024).toFixed(1)} MB/s</Badge>
                  </div>
                  <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-secondary">
                    <div
                      className="h-full rounded-full bg-emerald-500"
                      style={{ width: `${Math.max(12, pct)}%` }}
                    />
                  </div>
                  <div className="mt-1.5 flex justify-between text-[11px] text-muted-foreground">
                    <span>协议: {n.protocol}</span>
                    <span>峰值: {(n.peak_kbps / 1024).toFixed(1)} MB/s</span>
                  </div>
                </div>
              );
            })}
          </div>
        </Card>
      )}
    </div>
  );
}
