import { useCallback, useEffect, useState } from "react";
import {
  Activity,
  BarChart3,
  Cloud,
  MousePointerClick,
  QrCode,
  RefreshCw,
  ShieldAlert,
} from "lucide-react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Legend,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { apiFetch, type MeResponse } from "../lib/api";
import { Badge, Button, Card } from "../components/ui/primitives";
import { AdminUsersTab } from "../components/AdminUsersTab";

interface AnalyticsSummary {
  ok: boolean;
  totals: {
    total_events: number;
    quark_clicks: number;
    mirror_clicks: number;
    qr_opens: number;
    page_views: number;
    quark_ctr_pct: number;
    total_catalog: number;
    quark_ready_catalog: number;
  };
  channel_breakdown: { channel: string; count: number }[];
  os_breakdown: { os: string; count: number }[];
  top_downloads: { slug: string; quark: number; mirror: number; total: number }[];
  recent_events: {
    id: number;
    slug: string;
    channel: string;
    mirror: string;
    os: string;
    arch: string;
    created_unix: number;
  }[];
}

const PIE_COLORS = ["#f97316", "#10b981", "#3b82f6", "#a855f7", "#eab308", "#ec4899"];

interface AdminAnalyticsPageProps {
  me: MeResponse | null;
  onNeedAuth: () => void;
}

export function AdminAnalyticsPage({ me, onNeedAuth }: AdminAnalyticsPageProps) {
  const [subTab, setSubTab] = useState<"analytics" | "users">("users");
  const [data, setData] = useState<AnalyticsSummary | null>(null);
  const [err, setErr] = useState("");
  const [loading, setLoading] = useState(false);

  const load = useCallback(() => {
    setLoading(true);
    setErr("");
    apiFetch<AnalyticsSummary>("/api/admin/analytics/summary")
      .then((res) => setData(res))
      .catch((e: Error) => setErr(e.message || "无法加载数据看板"))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    if (me) load();
  }, [me, load]);

  if (!me) {
    return (
      <Card className="p-8 text-center space-y-3 max-w-md mx-auto">
        <ShieldAlert className="h-8 w-8 text-amber-500 mx-auto" />
        <h2 className="text-lg font-semibold">需要管理员登录</h2>
        <p className="text-xs text-muted-foreground">
          请使用管理员账号（lilyco42ovo）登录后查看实时用户行为与夸克转化数据看板。
        </p>
        <Button variant="emerald" onClick={onNeedAuth}>
          前往登录
        </Button>
      </Card>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <div className="flex items-center gap-2">
            <Badge variant="emerald">Admin Analytics · Recharts</Badge>
            <Badge variant="outline">Turso 实时行为采集</Badge>
          </div>
          <h1 className="mt-2 text-2xl font-bold tracking-tight flex items-center gap-2">
            <BarChart3 className="h-6 w-6 text-emerald-500" />
            管理员数据看板与用户行为追踪
          </h1>
          <p className="text-sm text-muted-foreground mt-1">
            实时监控全站下载点击、夸克网盘极速下载转化占比、扫码弹窗曝光与客户端系统分布
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant={subTab === "users" ? "emerald" : "outline"}
            onClick={() => setSubTab("users")}
          >
            用户管理与人工发卡
          </Button>
          <Button
            size="sm"
            variant={subTab === "analytics" ? "emerald" : "outline"}
            onClick={() => setSubTab("analytics")}
          >
            数据看板与行为追踪
          </Button>
          {subTab === "analytics" && (
            <Button size="sm" variant="outline" onClick={load} disabled={loading}>
              <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
              刷新
            </Button>
          )}
        </div>
      </div>

      {subTab === "users" ? (
        <AdminUsersTab />
      ) : (
        <>

      {err && (
        <Card className="p-4 border-destructive/40 text-sm text-destructive">
          {err}
        </Card>
      )}

      {data && (
        <>
          {/* 4 KPI Cards */}
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <Card className="p-4 space-y-1 border-orange-500/30 bg-orange-500/[0.04]">
              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>夸克极速下载占比 (CTR)</span>
                <Cloud className="h-4 w-4 text-orange-500" />
              </div>
              <div className="text-2xl font-bold font-mono text-orange-500">
                {data.totals.quark_ctr_pct.toFixed(1)}%
              </div>
              <div className="text-[11px] text-muted-foreground">
                夸克 {data.totals.quark_clicks} 次 vs 镜像 {data.totals.mirror_clicks} 次
              </div>
            </Card>

            <Card className="p-4 space-y-1">
              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>总采集用户行为事件</span>
                <MousePointerClick className="h-4 w-4 text-emerald-500" />
              </div>
              <div className="text-2xl font-bold font-mono">
                {data.totals.total_events}
              </div>
              <div className="text-[11px] text-muted-foreground">
                页面访问 {data.totals.page_views} 次
              </div>
            </Card>

            <Card className="p-4 space-y-1">
              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>夸克扫码弹窗曝光</span>
                <QrCode className="h-4 w-4 text-emerald-500" />
              </div>
              <div className="text-2xl font-bold font-mono">
                {data.totals.qr_opens}
              </div>
              <div className="text-[11px] text-muted-foreground">
                PC 端手机扫码秒存引导
              </div>
            </Card>

            <Card className="p-4 space-y-1 border-emerald-500/30">
              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>夸克网盘覆盖率</span>
                <Activity className="h-4 w-4 text-emerald-500" />
              </div>
              <div className="text-2xl font-bold font-mono text-emerald-600 dark:text-emerald-400">
                {data.totals.quark_ready_catalog} / {data.totals.total_catalog}
              </div>
              <div className="text-[11px] text-muted-foreground">
                后台不落盘守护持续同步中
              </div>
            </Card>
          </div>

          {/* Recharts Charts Grid */}
          <div className="grid gap-4 lg:grid-cols-3">
            <Card className="p-4 lg:col-span-2 space-y-3">
              <h2 className="text-sm font-semibold">
                热门软件点击对比（夸克极速 vs 普通镜像）
              </h2>
              {data.top_downloads.length === 0 ? (
                <div className="h-64 flex items-center justify-center text-xs text-muted-foreground">
                  暂无下载点击数据，前往下载中心点击测试后刷新即可看到实时柱状图
                </div>
              ) : (
                <div className="h-72 w-full">
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart
                      data={data.top_downloads}
                      margin={{ top: 8, right: 16, left: 0, bottom: 24 }}
                    >
                      <CartesianGrid strokeDasharray="3 3" opacity={0.2} />
                      <XAxis
                        dataKey="slug"
                        tick={{ fontSize: 11 }}
                        angle={-20}
                        textAnchor="end"
                        interval={0}
                      />
                      <YAxis allowDecimals={false} tick={{ fontSize: 11 }} />
                      <Tooltip />
                      <Legend />
                      <Bar dataKey="quark" name="夸克极速下载" fill="#f97316" radius={[4, 4, 0, 0]} />
                      <Bar dataKey="mirror" name="普通镜像源" fill="#10b981" radius={[4, 4, 0, 0]} />
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              )}
            </Card>

            <Card className="p-4 space-y-3">
              <h2 className="text-sm font-semibold">访客操作系统分布</h2>
              {data.os_breakdown.length === 0 ? (
                <div className="h-64 flex items-center justify-center text-xs text-muted-foreground">
                  暂无系统分布数据
                </div>
              ) : (
                <div className="h-72 w-full">
                  <ResponsiveContainer width="100%" height="100%">
                    <PieChart>
                      <Pie
                        data={data.os_breakdown}
                        dataKey="count"
                        nameKey="os"
                        cx="50%"
                        cy="50%"
                        innerRadius={45}
                        outerRadius={80}
                        paddingAngle={3}
                      >
                        {data.os_breakdown.map((entry, idx) => (
                          <Cell
                            key={entry.os}
                            fill={PIE_COLORS[idx % PIE_COLORS.length]}
                          />
                        ))}
                      </Pie>
                      <Tooltip />
                      <Legend />
                    </PieChart>
                  </ResponsiveContainer>
                </div>
              )}
            </Card>
          </div>

          {/* Live Recent Events Stream */}
          <Card className="p-4 space-y-3">
            <h2 className="text-sm font-semibold">实时用户行为追踪流水（最近 30 条）</h2>
            <div className="overflow-x-auto">
              <table className="w-full text-xs">
                <thead className="border-b text-muted-foreground">
                  <tr>
                    <th className="py-2 text-left font-medium">时间</th>
                    <th className="py-2 text-left font-medium">目标软件 / 页面</th>
                    <th className="py-2 text-left font-medium">渠道事件</th>
                    <th className="py-2 text-left font-medium">命中源</th>
                    <th className="py-2 text-left font-medium">客户端 OS / Arch</th>
                  </tr>
                </thead>
                <tbody className="divide-y">
                  {data.recent_events.map((ev) => (
                    <tr key={ev.id} className="hover:bg-muted/30">
                      <td className="py-2 font-mono text-muted-foreground">
                        {new Date(ev.created_unix * 1000).toLocaleString("zh-CN")}
                      </td>
                      <td className="py-2 font-mono font-medium">{ev.slug}</td>
                      <td className="py-2">
                        <Badge
                          variant={
                            ev.channel === "quark"
                              ? "amber"
                              : ev.channel === "quark_qr"
                              ? "emerald"
                              : "outline"
                          }
                        >
                          {ev.channel}
                        </Badge>
                      </td>
                      <td className="py-2 text-muted-foreground">{ev.mirror || "—"}</td>
                      <td className="py-2 font-mono text-muted-foreground">
                        {ev.os}/{ev.arch}
                      </td>
                    </tr>
                  ))}
                  {data.recent_events.length === 0 && (
                    <tr>
                      <td colSpan={5} className="py-6 text-center text-muted-foreground">
                        暂无实时事件记录
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </Card>
        </>
      )}
        </>
      )}
    </div>
  );
}
