import { useEffect, useState } from "react";
import { Drawer } from "vaul";
import {
  Check,
  CheckCircle2,
  Copy,
  CreditCard,
  ExternalLink,
  ShieldCheck,
  Sparkles,
  Wallet,
  X,
} from "lucide-react";
import { apiFetch, type MeResponse } from "../lib/api";
import { Badge, Button } from "./ui/primitives";
import { type CardTier, MembershipCard } from "./MembershipCard";

export interface CheckoutPlanOption {
  code: string;
  cycleLabel: string;
  priceCny: number;
  pointsPrice: number;
  quotaText: string;
  durationText: string;
  saveBadge?: string;
}

export interface CheckoutTierConfig {
  tier: CardTier;
  title: string;
  subtitle: string;
  cardNo: string;
  highlights: string[];
  options: CheckoutPlanOption[];
}

export const TIER_CHECKOUT_CONFIGS: Record<CardTier, CheckoutTierConfig> = {
  black: {
    tier: "black",
    title: "黑金卡 · 商业独享家宽 Pro",
    subtitle: "单人独享海外真实住宅家宽静态 IP · Claude Pro/Max 专用防封",
    cardNo: "BPRO-8888-CLDE-2026",
    highlights: [
      "1,000 GB / 月超大配额 · 10 台设备",
      "单人独享原生住宅家宽 IP（绝不共享、极低风控分）",
      "专为 Claude Pro/Max、ChatGPT Pro、Cursor 稳定防封定制",
      "1 对 1 专属技术交付与优先线路维护",
    ],
    options: [
      { code: "proxy_pro_1m", cycleLabel: "按月", priceCny: 120, pointsPrice: 1200, quotaText: "1000 GB", durationText: "30 天" },
      { code: "proxy_pro_3m", cycleLabel: "按季 (9折)", priceCny: 324, pointsPrice: 3240, quotaText: "3000 GB", durationText: "90 天", saveBadge: "省 ¥36" },
      { code: "proxy_pro_1y", cycleLabel: "按年 (8折)", priceCny: 1152, pointsPrice: 11520, quotaText: "12000 GB", durationText: "365 天", saveBadge: "省 ¥288" },
      { code: "proxy_pro_3y", cycleLabel: "三年 (67折)", priceCny: 2880, pointsPrice: 28800, quotaText: "36000 GB", durationText: "1095 天", saveBadge: "省 ¥1440" },
    ],
  },
  gold: {
    tier: "gold",
    title: "尊享金卡 · 标准极速专线",
    subtitle: "韩国 12.7MB/s AnyTLS 最高优先级 · 220GB/月主力推荐",
    cardNo: "PRXY-220G-GOLD-2026",
    highlights: [
      "220 GB / 月高速流量 · 5 台设备（三年卡 8 台）",
      "韩国 12.7MB/s AnyTLS + Hy2 端口跳跃最高优先级",
      "4K 流媒体 / Claude Code / GitHub 满速零卡顿",
      "全站彻底纯净零推荐打扰",
    ],
    options: [
      { code: "proxy_1m_220g", cycleLabel: "按月", priceCny: 30, pointsPrice: 300, quotaText: "220 GB", durationText: "30 天" },
      { code: "proxy_3m", cycleLabel: "按季 (9折)", priceCny: 81, pointsPrice: 810, quotaText: "660 GB", durationText: "90 天", saveBadge: "省 ¥9" },
      { code: "proxy_1y", cycleLabel: "按年 (8折)", priceCny: 288, pointsPrice: 2880, quotaText: "2640 GB", durationText: "365 天", saveBadge: "省 ¥72" },
      { code: "proxy_3y", cycleLabel: "三年 (67折)", priceCny: 720, pointsPrice: 7200, quotaText: "7920 GB", durationText: "1095 天", saveBadge: "省 ¥360" },
    ],
  },
  silver: {
    tier: "silver",
    title: "极光银卡 · 轻量极速专线",
    subtitle: "100GB/月全量自建专线 · 纯净无打扰",
    cardNo: "PRXY-100G-SLVR-2026",
    highlights: [
      "100 GB / 月高速流量 · 3 台设备",
      "全量韩/美/澳/德/菲自建专线通用",
      "支持 AnyTLS + VLESS Reality + Hy2 端口跳跃",
      "高优先级供应 · 纯净零推荐打扰",
    ],
    options: [
      { code: "proxy_1m", cycleLabel: "按月", priceCny: 15, pointsPrice: 150, quotaText: "100 GB", durationText: "30 天" },
      { code: "proxy_15_3m", cycleLabel: "按季 (9折)", priceCny: 40.5, pointsPrice: 405, quotaText: "300 GB", durationText: "90 天", saveBadge: "省 ¥4.5" },
      { code: "proxy_15_1y", cycleLabel: "按年 (8折)", priceCny: 144, pointsPrice: 1440, quotaText: "1200 GB", durationText: "365 天", saveBadge: "省 ¥36" },
    ],
  },
  white: {
    tier: "white",
    title: "云雾白卡 · 体验 / 基础对话",
    subtitle: "类 GPT Free 入门档 · 公益池与基础线路 · 含云枢自研新品推荐",
    cardNo: "PRXY-STARTER-2026",
    highlights: [
      "¥1 公益体验档（10GB/月 · 1设备）或 ¥6 基础档（40GB/月 · 2设备）",
      "开放公益普通节点池与常规基础线路（高峰优先保障高阶专线）",
      "站内推送云枢自研新软件推荐，零恶意第三方广告",
      "适合先试后买、查资料与日常 AI 文字对话",
    ],
    options: [
      { code: "proxy_1m_1g", cycleLabel: "¥1 体验月卡", priceCny: 1, pointsPrice: 10, quotaText: "10 GB", durationText: "30 天" },
      { code: "proxy_6m", cycleLabel: "¥6 基础月卡", priceCny: 6, pointsPrice: 60, quotaText: "40 GB", durationText: "30 天" },
      { code: "proxy_1y_15g", cycleLabel: "¥9.9 公益年卡", priceCny: 9.9, pointsPrice: 99, quotaText: "120 GB", durationText: "365 天", saveBadge: "¥0.83/月" },
      { code: "proxy_1y_140g", cycleLabel: "¥58 基础年卡", priceCny: 58, pointsPrice: 580, quotaText: "480 GB", durationText: "365 天", saveBadge: "省 ¥14" },
    ],
  },
};

interface CardCheckoutDrawerProps {
  open: boolean;
  tier: CardTier;
  initialCode?: string;
  me: MeResponse | null;
  onClose: () => void;
  onRefreshMe: () => void;
  onNeedAuth: () => void;
  onGoAccount: () => void;
}

export function CardCheckoutDrawer({
  open,
  tier,
  initialCode,
  me,
  onClose,
  onRefreshMe,
  onNeedAuth,
  onGoAccount,
}: CardCheckoutDrawerProps) {
  const cfg = TIER_CHECKOUT_CONFIGS[tier];
  const [selectedCode, setSelectedCode] = useState<string>(
    initialCode || cfg.options[0].code
  );
  const [busy, setBusy] = useState<string>("");
  const [feedback, setFeedback] = useState<{ ok: boolean; msg: string } | null>(null);
  const [purchased, setPurchased] = useState<{
    license_key: string;
    clash_url: string;
    base64_url: string;
  } | null>(null);
  const [copiedKey, setCopiedKey] = useState("");

  useEffect(() => {
    const valid = cfg.options.find((o) => o.code === initialCode);
    setSelectedCode(valid ? valid.code : cfg.options[0].code);
    setFeedback(null);
    setPurchased(null);
    setCopiedKey("");
  }, [tier, initialCode, open, cfg.options]);

  function copyLink(id: string, text: string) {
    navigator.clipboard.writeText(text).then(() => {
      setCopiedKey(id);
      setTimeout(() => setCopiedKey(""), 1800);
    });
  }

  const opt =
    cfg.options.find((o) => o.code === selectedCode) || cfg.options[0];
  const paidBal = me?.points.paid_balance ?? 0;
  const enoughPoints = paidBal >= opt.pointsPrice;

  async function handlePointsPurchase() {
    if (!me) {
      onClose();
      onNeedAuth();
      return;
    }
    setBusy("points");
    setFeedback(null);
    try {
      const ref = `card_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`;
      const res = await apiFetch<{
        entitlement?: {
          license_key?: string;
          clash_url?: string;
          base64_url?: string;
        };
      }>("/studio/api/points/purchase", {
        method: "POST",
        body: JSON.stringify({ product: opt.code, reference: ref }),
      });
      setPurchased({
        license_key: res.entitlement?.license_key || cfg.cardNo,
        clash_url: res.entitlement?.clash_url || "",
        base64_url: res.entitlement?.base64_url || "",
      });
      setFeedback({ ok: true, msg: `开通成功！「${cfg.title}」已发放到您的账户钱包。` });
      onRefreshMe();
    } catch (e) {
      setFeedback({ ok: false, msg: (e as Error).message });
    } finally {
      setBusy("");
    }
  }

  async function handleAfdianPay() {
    if (!me) {
      onClose();
      onNeedAuth();
      return;
    }
    setBusy("afdian");
    setFeedback(null);
    try {
      // Pick closest afdian points pack >= priceCny
      const packs = [1, 5, 6, 10, 30, 45, 100, 200, 300, 1000];
      const chosen = packs.find((p) => p >= opt.priceCny) || 1000;
      const res = await apiFetch<{ automatic_checkout_url?: string; checkout_url?: string }>(
        "/studio/api/payments/afdian/intents",
        {
          method: "POST",
          body: JSON.stringify({
            product: `points_cny_${chosen}`,
            reference: `af_card_${Date.now()}`,
          }),
        }
      );
      const url = res.automatic_checkout_url || res.checkout_url;
      if (url) {
        window.open(url, "_blank", "noopener,noreferrer");
        setFeedback({
          ok: true,
          msg: `已打开爱发电 ¥${chosen} 专属充值页，付款到账后点击上方「付费积分一键开通」即可秒发卡。`,
        });
      }
    } catch (e) {
      setFeedback({ ok: false, msg: (e as Error).message });
    } finally {
      setBusy("");
    }
  }

  async function handleAlipay() {
    if (!me) {
      onClose();
      onNeedAuth();
      return;
    }
    setBusy("alipay");
    setFeedback(null);
    try {
      const res = await apiFetch<{ payment_html?: string; checkout_url?: string }>(
        "/studio/api/payments/alipay/intents",
        {
          method: "POST",
          body: JSON.stringify({
            amount_cny: opt.priceCny,
            product: opt.code,
          }),
        }
      );
      if (res.checkout_url) {
        window.open(res.checkout_url, "_blank", "noopener,noreferrer");
        setFeedback({ ok: true, msg: "已打开支付宝收银台，完成支付后自动到账。" });
      } else if (res.payment_html) {
        const div = document.createElement("div");
        div.innerHTML = res.payment_html;
        const form = div.querySelector("form");
        if (form) {
          document.body.appendChild(form);
          form.submit();
        }
      }
    } catch (e) {
      setFeedback({ ok: false, msg: (e as Error).message });
    } finally {
      setBusy("");
    }
  }

  return (
    <Drawer.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <Drawer.Portal>
        <Drawer.Overlay className="fixed inset-0 z-50 bg-black/60 backdrop-blur-xs" />
        <Drawer.Content className="fixed inset-x-0 bottom-0 z-50 mx-auto max-w-2xl rounded-t-2xl border bg-card text-card-foreground shadow-2xl outline-none">
          <div className="mx-auto mt-3 h-1.5 w-12 rounded-full bg-muted" />
          <div className="max-h-[85vh] overflow-y-auto p-5 sm:p-6 space-y-5">
            {/* Header */}
            <div className="flex items-start justify-between gap-3">
              <div>
                <Drawer.Title className="text-lg font-bold tracking-tight flex items-center gap-2">
                  <Sparkles className="h-4 w-4 text-amber-500" />
                  {cfg.title}
                </Drawer.Title>
                <Drawer.Description className="text-xs text-muted-foreground mt-0.5">
                  {cfg.subtitle}
                </Drawer.Description>
              </div>
              <Button variant="ghost" size="icon" onClick={onClose}>
                <X className="h-4 w-4" />
              </Button>
            </div>

            {purchased ? (
              <div className="py-3 space-y-4">
                <div className="flex items-center gap-2 text-emerald-600 dark:text-emerald-400">
                  <CheckCircle2 className="h-6 w-6 shrink-0" />
                  <div>
                    <h3 className="text-base font-bold text-foreground">
                      会员卡已激活！直接复制下方订阅即可使用
                    </h3>
                    <p className="text-xs text-muted-foreground">
                      已同步保存到「账户中心」，随时可再次查看或一键导入。
                    </p>
                  </div>
                </div>

                <div className="grid gap-4 md:grid-cols-2 items-start">
                  <MembershipCard
                    tier={cfg.tier}
                    title={`${cfg.title} (${opt.cycleLabel})`}
                    holder={(me?.username || "LAIN42 MEMBER").toUpperCase()}
                    cardNo={purchased.license_key}
                    expiresText={opt.durationText}
                    quotaText={opt.quotaText}
                  />

                  <div className="space-y-3">
                    {purchased.clash_url && (
                      <div className="space-y-1.5">
                        <div className="text-xs font-semibold">
                          🖥️ Windows / Mac · Clash Verge Rev 订阅链接
                        </div>
                        <div className="flex gap-1.5">
                          <input
                            readOnly
                            value={purchased.clash_url}
                            onFocus={(e) => e.currentTarget.select()}
                            className="flex-1 h-8 rounded-md border border-input bg-muted/40 px-2 font-mono text-[11px]"
                          />
                          <Button
                            size="sm"
                            variant="emerald"
                            onClick={() => copyLink("clash", purchased.clash_url)}
                          >
                            {copiedKey === "clash" ? (
                              <Check className="h-3.5 w-3.5" />
                            ) : (
                              <Copy className="h-3.5 w-3.5" />
                            )}
                          </Button>
                        </div>
                        <a
                          href={`clash://install-config?url=${encodeURIComponent(
                            purchased.clash_url
                          )}`}
                          className="inline-flex h-8 w-full items-center justify-center rounded-md bg-emerald-600 px-3 text-xs font-medium text-white hover:bg-emerald-700"
                        >
                          一键导入 Clash Verge Rev
                        </a>
                      </div>
                    )}

                    {purchased.base64_url && (
                      <div className="space-y-1.5">
                        <div className="text-xs font-semibold">
                          📱 iPhone / Android · Karing / Base64 通用订阅
                        </div>
                        <div className="flex gap-1.5">
                          <input
                            readOnly
                            value={purchased.base64_url}
                            onFocus={(e) => e.currentTarget.select()}
                            className="flex-1 h-8 rounded-md border border-input bg-muted/40 px-2 font-mono text-[11px]"
                          />
                          <Button
                            size="sm"
                            variant="outline"
                            onClick={() => copyLink("base64", purchased.base64_url)}
                          >
                            {copiedKey === "base64" ? (
                              <Check className="h-3.5 w-3.5 text-emerald-500" />
                            ) : (
                              <Copy className="h-3.5 w-3.5" />
                            )}
                          </Button>
                        </div>
                      </div>
                    )}

                    <div className="rounded-lg border bg-muted/30 p-2.5 text-[11px] text-muted-foreground space-y-1">
                      <div className="font-semibold text-foreground">3 步极速上手：</div>
                      <div>1. 下载 Clash Verge Rev（电脑）或 Karing（手机）</div>
                      <div>2. 点击上方「一键导入」或粘贴订阅链接更新配置</div>
                      <div>3. 选择「自动测速」并开启系统代理即可满速上网</div>
                    </div>
                  </div>
                </div>

                <div className="flex justify-end gap-2 border-t pt-3">
                  <Button
                    variant="emerald"
                    size="sm"
                    onClick={() => {
                      onClose();
                      onGoAccount();
                    }}
                  >
                    前往账户中心管理卡片
                  </Button>
                  <Button variant="outline" size="sm" onClick={onClose}>
                    完成
                  </Button>
                </div>
              </div>
            ) : (
              <div className="grid gap-5 md:grid-cols-2 items-start">
                {/* Left: Live 3D Card Preview + Highlights */}
                <div className="space-y-3">
                  <MembershipCard
                    tier={cfg.tier}
                    title={`${cfg.title} · ${opt.cycleLabel}`}
                    holder={(me?.username || "YOUR PASS").toUpperCase()}
                    cardNo={cfg.cardNo}
                    expiresText={`¥${opt.priceCny} / ${opt.durationText}`}
                    quotaText={opt.quotaText}
                  />
                  <ul className="space-y-1.5 text-xs text-muted-foreground pt-1">
                    {cfg.highlights.map((h) => (
                      <li key={h} className="flex items-start gap-1.5">
                        <ShieldCheck className="h-3.5 w-3.5 text-emerald-500 shrink-0 mt-0.5" />
                        <span>{h}</span>
                      </li>
                    ))}
                  </ul>
                </div>

                {/* Right: Cycle Picker + Instant Checkout */}
                <div className="space-y-4">
                  <div className="space-y-2">
                    <div className="text-xs font-semibold text-muted-foreground">
                      1. 选择订阅周期
                    </div>
                    <div className="grid grid-cols-2 gap-2">
                      {cfg.options.map((o) => {
                        const active = o.code === opt.code;
                        return (
                          <button
                            key={o.code}
                            type="button"
                            onClick={() => setSelectedCode(o.code)}
                            className={`rounded-lg border p-2.5 text-left transition-all cursor-pointer ${
                              active
                                ? "border-emerald-500 bg-emerald-500/10 ring-1 ring-emerald-500"
                                : "hover:bg-accent"
                            }`}
                          >
                            <div className="flex items-center justify-between gap-1">
                              <span className="text-xs font-semibold">{o.cycleLabel}</span>
                              {o.saveBadge && (
                                <Badge variant="amber" className="px-1.5 py-0 text-[10px]">
                                  {o.saveBadge}
                                </Badge>
                              )}
                            </div>
                            <div className="mt-1 flex items-baseline gap-1">
                              <span className="text-base font-bold">¥{o.priceCny}</span>
                              <span className="text-[10px] text-muted-foreground">
                                ({o.pointsPrice}分)
                              </span>
                            </div>
                          </button>
                        );
                      })}
                    </div>
                  </div>

                  {/* Balance & Payment Buttons */}
                  <div className="space-y-2.5 border-t pt-3">
                    <div className="flex items-center justify-between text-xs">
                      <span className="text-muted-foreground flex items-center gap-1">
                        <Wallet className="h-3.5 w-3.5 text-emerald-500" />
                        当前付费积分
                      </span>
                      <span className="font-mono font-semibold">
                        {me ? `${paidBal} 分` : "未登录"}
                      </span>
                    </div>

                    {feedback && (
                      <div
                        className={`rounded-md border px-3 py-2 text-xs ${
                          feedback.ok
                            ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                            : "border-destructive/30 bg-destructive/10 text-destructive"
                        }`}
                      >
                        {feedback.msg}
                      </div>
                    )}

                    <Button
                      variant="emerald"
                      size="lg"
                      className="w-full"
                      disabled={busy !== ""}
                      onClick={handlePointsPurchase}
                    >
                      <CreditCard className="h-4 w-4" />
                      {!me
                        ? "请先登录后开通"
                        : busy === "points"
                        ? "正在激活会员卡..."
                        : enoughPoints
                        ? `付费积分一键开通（扣 ${opt.pointsPrice} 分）`
                        : `付费积分不足（还差 ${opt.pointsPrice - paidBal} 分）`}
                    </Button>

                    <div className="grid grid-cols-2 gap-2">
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={busy !== ""}
                        onClick={handleAfdianPay}
                      >
                        <ExternalLink className="h-3.5 w-3.5" />
                        爱发电充值
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={busy !== ""}
                        onClick={handleAlipay}
                      >
                        <CreditCard className="h-3.5 w-3.5" />
                        支付宝支付
                      </Button>
                    </div>
                  </div>
                </div>
              </div>
            )}
          </div>
        </Drawer.Content>
      </Drawer.Portal>
    </Drawer.Root>
  );
}
