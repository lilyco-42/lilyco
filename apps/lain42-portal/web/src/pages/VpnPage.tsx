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
import { MembershipCard } from "../components/MembershipCard";

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

      {/* 4-Tier Metallic Pass Showcase (黑卡 / 金卡 / 银卡 / 白卡) */}
      <div className="grid gap-3.5 sm:grid-cols-2 lg:grid-cols-4">
        <MembershipCard
          tier="black"
          title="黑金卡 · 独享家宽 Pro"
          holder="CLAUDE PRO DEDICATED"
          cardNo="BPRO-8888-CLDE-2026"
          expiresText="¥120 / 月起"
          quotaText="1000 GB"
          compact
        />
        <MembershipCard
          tier="gold"
          title="尊享金卡 · 标准极速"
          holder="12.7MB/S ANYTLS"
          cardNo="PRXY-220G-GOLD-2026"
          expiresText="¥30 / 月起"
          quotaText="220 GB"
          compact
        />
        <MembershipCard
          tier="silver"
          title="极光银卡 · 轻量极速"
          holder="ZERO-AD EXPRESS"
          cardNo="PRXY-100G-SLVR-2026"
          expiresText="¥15 / 月起"
          quotaText="100 GB"
          compact
        />
        <MembershipCard
          tier="white"
          title="云雾白卡 · 体验/基础"
          holder="LAIN42 MEMBER"
          cardNo="PRXY-FREE-PASS-2026"
          expiresText="¥1 / ¥6 起"
          quotaText="10~40 GB"
          compact
        />
      </div>

      {/* Billing Cycle Switcher (按月 / 按季 / 按年 / 按三年) */}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap gap-2">
          {(
            [
              ["month", "按月付费"],
              ["quarter", "按季续费 (9折省¥9)"],
              ["year", "按年续费 (8折省¥72)"],
              ["3year", "按三年尊享 (67折省¥360)"],
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
          类 GPT 阶梯规则：低价体验档限普通节点池并推送云枢自研新品；极速专线档独享 12.7MB/s 最高优先且纯净零打扰
        </span>
      </div>

      {/* Pricing Cards by Billing Cycle */}
      {cycle === "month" && (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
          <Card className="flex flex-col justify-between p-5 border-dashed">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">¥1 体验月卡</span>
                <Badge variant="outline">公益池 · 含新品推荐</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥1</span>
                <span className="text-xs text-muted-foreground">/ 月（10 积分）</span>
              </div>
              <ul className="mt-3 space-y-1.5 text-xs text-muted-foreground">
                <li>• 10GB 流量 · 限 1 台设备</li>
                <li>• 仅开放公益普通节点池</li>
                <li>• 高峰期优先保障高阶专线</li>
                <li>• 站内推送云枢自研新软件推荐</li>
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
                <Badge variant="secondary">常规线路 · 轻新品推荐</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥6</span>
                <span className="text-xs text-muted-foreground">/ 月（60 积分）</span>
              </div>
              <ul className="mt-3 space-y-1.5 text-xs text-muted-foreground">
                <li>• 40GB 流量 · 限 2 台设备</li>
                <li>• 常规基础线路 + 公益节点池</li>
                <li>• 不含韩国 12.7MB/s 极速优先通道</li>
                <li>• 适合 AI 文字对话，含轻量新品推荐</li>
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
                <Badge variant="emerald">100GB · 纯净无打扰</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥15</span>
                <span className="text-xs text-muted-foreground">/ 月（150 积分）</span>
              </div>
              <ul className="mt-3 space-y-1.5 text-xs text-muted-foreground">
                <li>• 100GB 极速流量 · 3 台设备</li>
                <li>• 全量韩/美/澳/德/菲自建专线</li>
                <li>• 支持 AnyTLS + Hy2 端口跳跃</li>
                <li>• 高优先级供应 · 纯净零推荐打扰</li>
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
                <li>• 纯净零打扰 · 120s 自动容错切换</li>
              </ul>
            </div>
            <Button className="mt-4 w-full" variant="emerald" onClick={() => onNavigate("shop")}>
              推荐：选购 ¥30 主力档 (220GB)
            </Button>
          </Card>
        </div>
      )}

      {cycle === "quarter" && (
        <div className="grid gap-4 md:grid-cols-3">
          <Card className="flex flex-col justify-between p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">¥15 轻量极速 · 季卡（9 折）</span>
                <Badge variant="secondary">300GB / 90 天</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥40.5</span>
                <span className="text-xs text-muted-foreground">/ 季（折合 ¥13.5/月）</span>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                每月 100GB 全量自建极速专线（共 300GB），3 台设备，纯净零打扰。
              </p>
            </div>
            <Button className="mt-4 w-full" variant="outline" onClick={() => onNavigate("shop")}>
              选购轻量季卡 (¥40.5)
            </Button>
          </Card>

          <Card className="flex flex-col justify-between border-emerald-500/50 bg-emerald-500/[0.03] p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">¥30 标准极速 · 季卡（9 折）</span>
                <Badge variant="emerald">660GB / 90 天</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥81</span>
                <span className="text-xs text-muted-foreground">/ 季（折合 ¥27/月 · 省 ¥9）</span>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                每月 220GB 标准极速专线（共 660GB），5 台设备，韩国 12.7MB/s AnyTLS 最高优先供应，纯净零打扰。
              </p>
            </div>
            <Button className="mt-4 w-full" variant="emerald" onClick={() => onNavigate("shop")}>
              选购标准季卡 (¥81)
            </Button>
          </Card>

          <Card className="flex flex-col justify-between border-amber-500/60 bg-amber-500/[0.04] p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">👑 商业独享家宽 Pro · 季卡</span>
                <Badge variant="amber">独享 IP · 省 ¥36</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥324</span>
                <span className="text-xs text-muted-foreground">/ 季（折合 ¥108/月）</span>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                单人独享原生住宅家宽静态 IP，Claude Pro/Max 零风控专用，3000GB / 90 天，1 对 1 定制交付。
              </p>
            </div>
            <Button className="mt-4 w-full" variant="emerald" onClick={() => onNavigate("shop")}>
              开通独享家宽季卡 (¥324)
            </Button>
          </Card>
        </div>
      )}

      {cycle === "year" && (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
          <Card className="flex flex-col justify-between p-5 border-dashed">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">公益体验年卡</span>
                <Badge variant="outline">公益池 · 含新品推荐</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥9.9</span>
                <span className="text-xs text-muted-foreground">/ 年（折 ¥0.83/月）</span>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                全年 120GB，限 1 台设备，仅开放公益普通节点池，站内推送云枢自研新软件推荐。
              </p>
            </div>
            <Button className="mt-4 w-full" variant="outline" onClick={() => onNavigate("shop")}>
              选购 ¥9.9 公益年卡
            </Button>
          </Card>

          <Card className="flex flex-col justify-between p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">基础对话年卡（8 折）</span>
                <Badge variant="secondary">480GB · 省 ¥14</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥58</span>
                <span className="text-xs text-muted-foreground">/ 年（折 ¥4.8/月）</span>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                全年 480GB，限 2 台设备，常规基础线路 + 公益池，展示轻量云枢自研新品推荐。
              </p>
            </div>
            <Button className="mt-4 w-full" variant="outline" onClick={() => onNavigate("shop")}>
              选购 ¥58 基础年卡
            </Button>
          </Card>

          <Card className="flex flex-col justify-between p-5 border-emerald-500/40">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">¥15 轻量极速年卡（8 折）</span>
                <Badge variant="emerald">1200GB · 省 ¥36</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥144</span>
                <span className="text-xs text-muted-foreground">/ 年（折 ¥12/月）</span>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                全年 1200GB（100GB/月），3 台设备，全量自建极速专线，高优先供应，纯净零打扰。
              </p>
            </div>
            <Button className="mt-4 w-full" variant="outline" onClick={() => onNavigate("shop")}>
              选购 ¥144 轻量年卡
            </Button>
          </Card>

          <Card className="flex flex-col justify-between border-emerald-500/60 bg-emerald-500/[0.04] p-5">
            <div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">¥30 标准极速年卡（8 折）</span>
                <Badge variant="amber">2640GB · 省 ¥72</Badge>
              </div>
              <div className="mt-3 flex items-baseline gap-1">
                <span className="text-3xl font-bold">¥288</span>
                <span className="text-xs text-muted-foreground">/ 年（折 ¥24/月）</span>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                全年 2640GB（220GB/月），5 台设备，韩国 12.7MB/s AnyTLS 最高优先 + 纯净零打扰。
              </p>
            </div>
            <Button className="mt-4 w-full" variant="emerald" onClick={() => onNavigate("shop")}>
              选购 ¥288 标准年卡
            </Button>
          </Card>
        </div>
      )}

      {cycle === "3year" && (
        <div className="grid gap-4 lg:grid-cols-2">
          <Card className="border-emerald-500/50 bg-emerald-500/[0.04] p-6 flex flex-col justify-between space-y-4">
            <div className="space-y-2">
              <div className="flex flex-wrap items-center gap-2">
                <Badge variant="emerald">极速专线 · 三年 67 折立省 ¥360</Badge>
                <Badge variant="outline">折合仅 ¥20 / 月</Badge>
              </div>
              <h2 className="text-lg font-bold">标准极速专线 · 三年尊享卡（7,920 GB）</h2>
              <p className="text-xs text-muted-foreground">
                每月 220GB 满血极速专线，8 台设备，韩国 12.7MB/s AnyTLS 最高优先级、新节点免费自动扩容、全站纯净零打扰。
              </p>
            </div>
            <div className="flex items-end justify-between border-t pt-4">
              <div>
                <span className="text-3xl font-bold">¥720</span>
                <span className="ml-2 text-xs line-through text-muted-foreground">¥1,080</span>
              </div>
              <Button variant="emerald" onClick={() => onNavigate("shop")}>
                开通三年标准卡 (¥720)
              </Button>
            </div>
          </Card>

          <Card className="border-amber-500/60 bg-gradient-to-br from-amber-500/[0.08] to-orange-500/[0.04] p-6 flex flex-col justify-between space-y-4">
            <div className="space-y-2">
              <div className="flex flex-wrap items-center gap-2">
                <Badge variant="amber">👑 商业独享家宽 Pro · 三年 67 折省 ¥1,440</Badge>
                <Badge variant="outline">折合仅 ¥80 / 月</Badge>
              </div>
              <h2 className="text-lg font-bold">商业独享家宽 IP · 三年旗舰定制（36,000 GB）</h2>
              <p className="text-xs text-muted-foreground">
                单人独享海外真实家庭宽带原生静态 IP（绝不共享），专为 Claude Pro/Max、ChatGPT Pro 与海外商业账号防封定制，支持免费定制地区与专属协议。
              </p>
            </div>
            <div className="flex items-end justify-between border-t pt-4">
              <div>
                <span className="text-3xl font-bold">¥2,880</span>
                <span className="ml-2 text-xs line-through text-muted-foreground">¥4,320</span>
              </div>
              <Button variant="emerald" onClick={() => onNavigate("shop")}>
                开通三年独享家宽 (¥2,880)
              </Button>
            </div>
          </Card>
        </div>
      )}

      {/* Flagship GPT-Pro-Style Dedicated Residential IP Banner (Always Visible!) */}
      <Card className="border-amber-500/60 bg-gradient-to-r from-amber-500/[0.08] via-orange-500/[0.05] to-emerald-500/[0.05] p-6">
        <div className="flex flex-col gap-5 lg:flex-row lg:items-center lg:justify-between">
          <div className="space-y-2 max-w-2xl">
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant="amber">👑 PRO 旗舰定制 · 类 GPT Pro 500x 级定位</Badge>
              <Badge variant="emerald">单人独享原生家庭宽带 IP</Badge>
              <Badge variant="outline">Claude 订阅专用防封</Badge>
            </div>
            <h2 className="text-xl font-bold tracking-tight">
              商业专用独享 · 家庭宽带（家宽）独享 IP 定制服务
            </h2>
            <p className="text-sm text-muted-foreground leading-relaxed">
              告别共享机场 IP 导致的 Claude 封号、ChatGPT 降智与人机验证！为您 1 对 1 独立分配海外真实住宅家庭宽带静态 IP，<strong>仅供您个人独享使用</strong>，纯净零污染、超低风控分，专为 <strong>Claude Pro / Max 订阅、ChatGPT Pro、Cursor、海外支付与商业工作流</strong> 打造。
            </p>
            <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground pt-1">
              <span>✓ 1,000 GB / 月 · 10 台设备</span>
              <span>✓ 单人独享原生家宽静态 IP</span>
              <span>✓ 按需定制地区 / 专属 AnyTLS 节点</span>
              <span>✓ 1 对 1 专属技术交付与售后</span>
            </div>
          </div>

          <div className="shrink-0 space-y-2.5 lg:text-right border-t lg:border-t-0 pt-4 lg:pt-0">
            <div>
              <span className="text-3xl font-bold text-amber-600 dark:text-amber-400">¥120</span>
              <span className="text-xs text-muted-foreground"> / 月（季 ¥324 · 年 ¥1,152 · 三年 ¥2,880）</span>
            </div>
            <div className="flex flex-wrap gap-2 lg:justify-end">
              <Button variant="emerald" onClick={() => onNavigate("shop")}>
                立即定制独享家宽 IP
              </Button>
              <Button variant="outline" onClick={() => onNavigate("account")}>
                提交定制需求工单
              </Button>
            </div>
          </div>
        </div>
      </Card>

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
