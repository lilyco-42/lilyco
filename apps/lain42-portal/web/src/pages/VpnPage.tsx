import { useEffect, useState } from "react";
import {
  Activity,
  CheckCircle2,
  Copy,
  Download,
  ExternalLink,
  Globe,
  Terminal,
} from "lucide-react";
import { apiFetch, type ProxyQualityNode } from "../lib/api";
import { Badge, Button, Card } from "../components/ui/primitives";

interface VpnPageProps {
  onNavigate: (tab: string) => void;
}

export function VpnPage({ onNavigate }: VpnPageProps) {
  const [nodes, setNodes] = useState<ProxyQualityNode[]>([]);
  const [copiedCmd, setCopiedCmd] = useState<string>("");
  const [cycle, setCycle] = useState<"month" | "quarter" | "year" | "3year">("month");

  useEffect(() => {
    apiFetch<{ nodes: ProxyQualityNode[] }>("/api/proxy/quality")
      .then((res) => setNodes(res.nodes || []))
      .catch(() => {});
  }, []);

  const copyText = (txt: string, key: string) => {
    navigator.clipboard.writeText(txt).catch(() => {});
    setCopiedCmd(key);
    setTimeout(() => setCopiedCmd(""), 1800);
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <Badge variant="emerald">LYCO Connect · 自动测速容错</Badge>
          <h1 className="mt-2 text-2xl font-bold tracking-tight">高速代理专线与使用指南</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            15 元（100GB）与 30 元（220GB）两档月卡清晰区分，内置 AnyTLS + VLESS Reality + Hy2
            端口跳跃，每 120 秒自动切至最快线路。
          </p>
        </div>
        <Button variant="emerald" onClick={() => onNavigate("shop")}>
          前往商城开通 / 续费
        </Button>
      </div>

      {/* Billing Cycle Switcher (按月 / 按季 / 按年 / 按三年) */}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap gap-2">
          {(
            [
              ["month", "按月付费"],
              ["quarter", "按季续费 (98折)"],
              ["year", "按年续费 (9折省¥36)"],
              ["3year", "按三年尊享 (74折省¥281)"],
            ] as const
          ).map(([id, label]) => (
            <Button
              key={id}
              size="sm"
              variant={cycle === id ? "emerald" : "outline"}
              onClick={() => setCycle(id)}
            >
              {label}
            </Button>
          ))}
        </div>
        <span className="text-xs text-muted-foreground">
          低端引流档限公益/基础池且含广告；极速专线档独享 12.7MB/s 韩国 AnyTLS 最高优先且零广告
        </span>
      </div>

      {/* Pricing Cards by Billing Cycle */}
      {cycle === "month" && (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
          <Card className="flex flex-col justify-between p-5 border-dashed">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">¥1 体验月卡</span>
                <Badge variant="outline">公益池 · 含广告</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥1</span>
                <span className="text-xs text-muted-foreground">/ 月（10 积分）</span>
              </div>
              <ul className="mt-3 space-y-1.5 text-xs text-muted-foreground">
                <li>• 10GB 流量 · 限 1 台设备</li>
                <li>• 仅开放公益普通节点池</li>
                <li>• 高峰期优先保障高阶专线</li>
                <li>• 站内/订阅页展示赞助广告</li>
              </ul>
            </div>
            <Button className="mt-4 w-full" variant="outline" onClick={() => onNavigate("shop")}>
              体验 ¥1 档 (10GB)
            </Button>
          </Card>

          <Card className="flex flex-col justify-between p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">¥6 基础月卡</span>
                <Badge variant="secondary">常规线路 · 轻广告</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥6</span>
                <span className="text-xs text-muted-foreground">/ 月（60 积分）</span>
              </div>
              <ul className="mt-3 space-y-1.5 text-xs text-muted-foreground">
                <li>• 40GB 流量 · 限 2 台设备</li>
                <li>• 常规基础线路 + 公益节点池</li>
                <li>• 不含韩国 12.7MB/s 极速优先通道</li>
                <li>• 适合 AI 文字对话，含轻量推荐广告</li>
              </ul>
            </div>
            <Button className="mt-4 w-full" variant="outline" onClick={() => onNavigate("shop")}>
              选购 ¥6 基础档 (40GB)
            </Button>
          </Card>

          <Card className="flex flex-col justify-between p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">¥15 轻量极速月卡</span>
                <Badge variant="emerald">100GB · 零广告</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥15</span>
                <span className="text-xs text-muted-foreground">/ 月（150 积分）</span>
              </div>
              <ul className="mt-3 space-y-1.5 text-xs text-muted-foreground">
                <li>• 100GB 极速流量 · 3 台设备</li>
                <li>• 全量韩/美/澳/德/菲自建专线</li>
                <li>• 支持 AnyTLS + Hy2 端口跳跃</li>
                <li>• 高优先级供应 · 彻底零广告</li>
              </ul>
            </div>
            <Button className="mt-4 w-full" variant="outline" onClick={() => onNavigate("shop")}>
              选购 ¥15 轻量专线 (100GB)
            </Button>
          </Card>

          <Card className="flex flex-col justify-between border-emerald-500/60 bg-emerald-500/[0.04] p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">¥30 标准极速月卡</span>
                <Badge variant="emerald">220GB · 最高优先</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥30</span>
                <span className="text-xs text-muted-foreground">/ 月（300 积分）</span>
              </div>
              <ul className="mt-3 space-y-1.5 text-xs text-muted-foreground">
                <li>• 220GB 大流量 · 5 台设备</li>
                <li>• 韩国 12.7MB/s AnyTLS 最高优先</li>
                <li>• 4K 流媒体 / Claude Code / GitHub 满速</li>
                <li>• 彻底零广告 · 120s 自动容错切换</li>
              </ul>
            </div>
            <Button className="mt-4 w-full" variant="emerald" onClick={() => onNavigate("shop")}>
              推荐：选购 ¥30 主力档 (220GB)
            </Button>
          </Card>
        </div>
      )}

      {cycle === "quarter" && (
        <div className="grid gap-4 md:grid-cols-2">
          <Card className="flex flex-col justify-between border-emerald-500/50 bg-emerald-500/[0.03] p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-base font-semibold">极速专线 · 季卡（98 折）</span>
                <Badge variant="emerald">660GB / 90 天</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥88.2</span>
                <span className="text-xs text-muted-foreground">/ 季（882 积分 · 折合 ¥29.4/月）</span>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                含 3 个月标准 220GB/月极速专线（共 660GB），5 台设备，韩国 12.7MB/s AnyTLS 最高优先级供应，零广告干扰。
              </p>
            </div>
            <Button className="mt-4 w-full" variant="emerald" onClick={() => onNavigate("shop")}>
              前往商城开通季卡 (660GB)
            </Button>
          </Card>
          <Card className="flex flex-col justify-between p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-base font-semibold">低预算用户？可切按月或按年</span>
                <Badge variant="outline">¥1 / ¥6 起</Badge>
              </div>
              <p className="mt-3 text-xs text-muted-foreground leading-relaxed">
                仅需轻度网页或文字对话可选择按月 ¥1（公益池）/ ¥6（基础线路），或选择更划算的年卡（9折）与三年尊享卡（74折立省 ¥281）。
              </p>
            </div>
            <Button className="mt-4 w-full" variant="outline" onClick={() => setCycle("year")}>
              查看年卡 / 三年卡更省方案
            </Button>
          </Card>
        </div>
      )}

      {cycle === "year" && (
        <div className="grid gap-4 md:grid-cols-3">
          <Card className="flex flex-col justify-between p-5 border-dashed">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">公益入门年卡</span>
                <Badge variant="outline">公益池 · 含广告</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥15</span>
                <span className="text-xs text-muted-foreground">/ 年（折合 ¥1.25/月）</span>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                全年 120GB，限 1 台设备，仅开放公益普通节点池，高峰期优先让渡高阶专线，含站内赞助广告。
              </p>
            </div>
            <Button className="mt-4 w-full" variant="outline" onClick={() => onNavigate("shop")}>
              选购 ¥15 公益年卡
            </Button>
          </Card>

          <Card className="flex flex-col justify-between p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">基础对话年卡</span>
                <Badge variant="secondary">常规线路 · 轻广告</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥72</span>
                <span className="text-xs text-muted-foreground">/ 年（折合 ¥6/月）</span>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                全年 480GB，限 2 台设备，常规基础线路 + 公益池，不含韩国 AnyTLS 最高优先通道，含轻量广告。
              </p>
            </div>
            <Button className="mt-4 w-full" variant="outline" onClick={() => onNavigate("shop")}>
              选购 ¥72 基础年卡
            </Button>
          </Card>

          <Card className="flex flex-col justify-between border-emerald-500/60 bg-emerald-500/[0.04] p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">极速专线年卡（9 折）</span>
                <Badge variant="amber">2640GB · 立省 ¥36</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥324</span>
                <span className="text-xs text-muted-foreground">/ 年（折合 ¥27/月）</span>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                全年 2640GB（220GB/月），5 台设备，韩国 12.7MB/s AnyTLS 最高优先级 + 工单优先响应 + 零广告。
              </p>
            </div>
            <Button className="mt-4 w-full" variant="emerald" onClick={() => onNavigate("shop")}>
              选购 ¥324 极速年卡
            </Button>
          </Card>
        </div>
      )}

      {cycle === "3year" && (
        <Card className="border-amber-500/50 bg-gradient-to-br from-amber-500/[0.06] to-emerald-500/[0.06] p-6">
          <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
            <div className="space-y-2">
              <div className="flex flex-wrap items-center gap-2">
                <Badge variant="amber">👑 三年尊享 · 74 折立省 ¥281</Badge>
                <Badge variant="emerald">折合仅 ¥22.2 / 月</Badge>
              </div>
              <h2 className="text-xl font-bold">极速专线 · 三年尊享卡（7,920 GB / 1,095 天）</h2>
              <p className="text-sm text-muted-foreground max-w-2xl">
                每月 220GB 满血极速专线，支持 8 台设备同时在线，享有韩国 12.7MB/s AnyTLS 最高优先级通道、未来新增海外专线免费自动扩容、全站彻底零广告与专属工单优先支持。
              </p>
            </div>
            <div className="shrink-0 text-left lg:text-right space-y-2">
              <div>
                <span className="text-3xl font-bold">¥799</span>
                <span className="ml-2 text-xs line-through text-muted-foreground">原价 ¥1,080</span>
              </div>
              <div className="text-xs text-muted-foreground">7,990 付费积分 · 一次锁定三年低价</div>
              <Button variant="emerald" size="lg" className="w-full lg:w-auto" onClick={() => onNavigate("shop")}>
                前往商城开通三年尊享卡
              </Button>
            </div>
          </div>
        </Card>
      )}

      {/* Clash Verge Rev Free Download Alert */}
      <Card className="border-amber-500/30 bg-amber-500/[0.04] p-5">
        <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
          <div className="space-y-1">
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Download className="h-4 w-4 text-amber-500" />
              强烈建议升级客户端至 Clash Verge Rev v2.5.10+（免费下载）
            </div>
            <p className="text-xs text-muted-foreground">
              新版 Mihomo 内核原生支持韩国 <b>AnyTLS（峰值 12.7 MB/s）</b> 与美国西雅图{" "}
              <b>Hysteria2 端口跳跃</b>。更新后在订阅页点击刷新即可体验全速。
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <a
              href="https://pan.quark.cn/s/6d35f67343ba"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-emerald-600 px-3 text-xs font-medium text-white hover:bg-emerald-700"
            >
              夸克网盘免费下载 <ExternalLink className="h-3.5 w-3.5" />
            </a>
            <a
              href="https://dl.lilyco42.top/d/clash-verge-rev"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex h-8 items-center gap-1.5 rounded-md border border-input bg-background px-3 text-xs font-medium hover:bg-accent"
            >
              Wrangler 加速下载 <ExternalLink className="h-3.5 w-3.5" />
            </a>
            <a
              href="https://github.com/clash-verge-rev/clash-verge-rev/releases/latest"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex h-8 items-center gap-1.5 rounded-md border border-input bg-background px-3 text-xs font-medium hover:bg-accent"
            >
              GitHub 官方 Release <ExternalLink className="h-3.5 w-3.5" />
            </a>
          </div>
        </div>
      </Card>

      {/* Live Node Throughput Table */}
      <Card className="overflow-hidden">
        <div className="flex items-center justify-between border-b px-5 py-3.5">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Activity className="h-4 w-4 text-emerald-500" />
            实时节点吞吐监控（全部实测存活）
          </div>
          <Badge variant="outline">自动测速周期: 120s</Badge>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full text-left text-xs">
            <thead className="border-b bg-muted/50 text-muted-foreground">
              <tr>
                <th className="px-5 py-2.5 font-medium">节点名称</th>
                <th className="px-4 py-2.5 font-medium">协议</th>
                <th className="px-4 py-2.5 font-medium">均速 (MB/s)</th>
                <th className="px-4 py-2.5 font-medium">峰值 (MB/s)</th>
                <th className="px-4 py-2.5 font-medium">状态</th>
              </tr>
            </thead>
            <tbody className="divide-y">
              {nodes.map((n) => (
                <tr key={n.name} className="hover:bg-muted/30">
                  <td className="px-5 py-2.5 font-medium">{n.name}</td>
                  <td className="px-4 py-2.5">
                    <Badge variant="secondary">{n.protocol}</Badge>
                  </td>
                  <td className="px-4 py-2.5 font-mono font-semibold text-emerald-600 dark:text-emerald-400">
                    {(n.kbps / 1024).toFixed(2)} MB/s
                  </td>
                  <td className="px-4 py-2.5 font-mono text-muted-foreground">
                    {(n.peak_kbps / 1024).toFixed(2)} MB/s
                  </td>
                  <td className="px-4 py-2.5">
                    <span className="inline-flex items-center gap-1 text-emerald-600 dark:text-emerald-400">
                      <CheckCircle2 className="h-3.5 w-3.5" /> 正常
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>

      {/* Multi-Platform Tutorial + VPS Deploy */}
      <div className="grid gap-4 md:grid-cols-2">
        <Card className="p-5 space-y-3">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Globe className="h-4 w-4 text-emerald-500" />
            全平台客户端导入教程（三步连通）
          </div>
          <ol className="list-decimal space-y-2 pl-4 text-xs leading-relaxed text-muted-foreground">
            <li>
              <b className="text-foreground">桌面端（Clash Verge Rev）</b>：登录「我的账户」复制
              Clash 订阅链接 → 打开客户端「订阅」粘贴导入 → 「代理」保持{" "}
              <code>🆕 自建专线 · 自动测速</code> → 开启「系统代理」（终端/游戏可开 TUN）。
            </li>
            <li>
              <b className="text-foreground">iOS（Karing / Shadowrocket）</b>：复制通用订阅链接 →
              配置 → 添加远程配置 → 更新并连接。
            </li>
            <li>
              <b className="text-foreground">Android（FlClash）</b>：导入 Clash 订阅链接即可；校园网屏蔽
              UDP 时可切换至 <code>🏫 校园网友好 · 443/TCP</code>。
            </li>
          </ol>
        </Card>

        <Card className="p-5 space-y-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Terminal className="h-4 w-4 text-blue-500" />
              运维：一键部署新 VPS（sh.lain42.top 大陆加速）
            </div>
            <a
              href="https://github.com/lilyco-42/lain42-node-deploy/blob/main/GUIDELINE.md"
              target="_blank"
              rel="noopener noreferrer"
              className="text-xs text-emerald-600 hover:underline dark:text-emerald-400"
            >
              GitHub Guideline ↗
            </a>
          </div>
          <div className="space-y-2">
            {[
              { key: "install", label: "一键五协议部署 + BBR/64MB 调优 + Hy2 端口跳跃", cmd: "curl -fsSL https://sh.lain42.top | bash" },
              { key: "yg", label: "拉起 sing-box-yg 交互菜单（大陆加速）", cmd: "curl -fsSL https://sh.lain42.top/sb.sh | bash" },
            ].map((item) => (
              <div key={item.key} className="rounded-md border bg-muted/40 p-2.5">
                <div className="mb-1 text-[11px] text-muted-foreground">{item.label}</div>
                <div className="flex items-center justify-between gap-2">
                  <code className="truncate font-mono text-xs">{item.cmd}</code>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-7 px-2"
                    onClick={() => copyText(item.cmd, item.key)}
                  >
                    <Copy className="h-3.5 w-3.5" />
                    {copiedCmd === item.key ? "已复制" : "复制"}
                  </Button>
                </div>
              </div>
            ))}
          </div>
        </Card>
      </div>
    </div>
  );
}
