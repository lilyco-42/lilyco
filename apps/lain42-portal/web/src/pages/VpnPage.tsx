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

      {/* Pricing Cards */}
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        <Card className="flex flex-col justify-between p-5">
          <div>
            <div className="flex items-center justify-between">
              <span className="text-sm font-semibold">轻量月卡</span>
              <Badge variant="secondary">100GB</Badge>
            </div>
            <div className="mt-3 flex items-baseline gap-1">
              <span className="text-3xl font-bold">¥15</span>
              <span className="text-xs text-muted-foreground">/ 30 天（150 积分）</span>
            </div>
            <p className="mt-2 text-xs text-muted-foreground">
              含 100GB 高速流量，全量韩/美/澳/德/菲专线通用，适合日常开发与 AI 对话。
            </p>
          </div>
          <Button className="mt-4 w-full" variant="outline" onClick={() => onNavigate("shop")}>
            选购 15 元档 (100GB)
          </Button>
        </Card>

        <Card className="flex flex-col justify-between border-emerald-500/50 bg-emerald-500/[0.03] p-5">
          <div>
            <div className="flex items-center justify-between">
              <span className="text-sm font-semibold">标准月卡</span>
              <Badge variant="emerald">220GB · 多送 20GB</Badge>
            </div>
            <div className="mt-3 flex items-baseline gap-1">
              <span className="text-3xl font-bold">¥30</span>
              <span className="text-xs text-muted-foreground">/ 30 天（300 积分）</span>
            </div>
            <p className="mt-2 text-xs text-muted-foreground">
              含 220GB 大流量，适合 4K 流媒体、高频 GitHub 拉取与重度 AI 工作流。
            </p>
          </div>
          <Button className="mt-4 w-full" variant="emerald" onClick={() => onNavigate("shop")}>
            选购 30 元档 (220GB)
          </Button>
        </Card>

        <Card className="flex flex-col justify-between p-5">
          <div>
            <div className="flex items-center justify-between">
              <span className="text-sm font-semibold">季卡（98 折）</span>
              <Badge variant="secondary">660GB</Badge>
            </div>
            <div className="mt-3 flex items-baseline gap-1">
              <span className="text-3xl font-bold">¥88.2</span>
              <span className="text-xs text-muted-foreground">/ 90 天（882 积分）</span>
            </div>
            <p className="mt-2 text-xs text-muted-foreground">
              按 30 元标准档 220GB/月享 98 折，总计 660GB 流量，一次开通省心一季。
            </p>
          </div>
          <Button className="mt-4 w-full" variant="outline" onClick={() => onNavigate("shop")}>
            选购季卡 (660GB)
          </Button>
        </Card>

        <Card className="flex flex-col justify-between p-5">
          <div>
            <div className="flex items-center justify-between">
              <span className="text-sm font-semibold">年卡（9 折）</span>
              <Badge variant="amber">2640GB · 立省 ¥36</Badge>
            </div>
            <div className="mt-3 flex items-baseline gap-1">
              <span className="text-3xl font-bold">¥324</span>
              <span className="text-xs text-muted-foreground">/ 365 天（3240 积分）</span>
            </div>
            <p className="mt-2 text-xs text-muted-foreground">
              按 30 元标准档 220GB/月享 9 折，全年总计 2640GB 流量，性价比最高。
            </p>
          </div>
          <Button className="mt-4 w-full" variant="outline" onClick={() => onNavigate("shop")}>
            选购年卡 (2640GB)
          </Button>
        </Card>
      </div>

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
