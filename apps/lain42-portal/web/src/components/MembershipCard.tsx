import { Crown, ShieldCheck, Sparkles, Wifi } from "lucide-react";

export type CardTier = "black" | "gold" | "silver" | "white";

export function inferCardTier(product: string): CardTier {
  const p = product.toLowerCase();
  if (p.startsWith("proxy_pro")) return "black";
  if (
    p === "proxy_1m_220g" ||
    p === "proxy_3m" ||
    p === "proxy_1y" ||
    p === "proxy_3y"
  ) {
    return "gold";
  }
  if (
    p === "proxy_1m" ||
    p === "proxy_15_3m" ||
    p === "proxy_15_1y" ||
    p.startsWith("music_")
  ) {
    return "silver";
  }
  return "white";
}

export function productDisplayTitle(product: string): string {
  const map: Record<string, string> = {
    proxy_pro_1m: "商业独享家宽 Pro · 月卡",
    proxy_pro_3m: "商业独享家宽 Pro · 季卡",
    proxy_pro_1y: "商业独享家宽 Pro · 年卡",
    proxy_pro_3y: "商业独享家宽 Pro · 三年旗舰",
    proxy_1m_220g: "极速专线 · 标准金卡",
    proxy_3m: "极速专线 · 季卡",
    proxy_1y: "极速专线 · 年卡",
    proxy_3y: "极速专线 · 三年尊享",
    proxy_1m: "极速专线 · 轻量银卡",
    proxy_15_3m: "极速专线 · 轻量季卡",
    proxy_15_1y: "极速专线 · 轻量年卡",
    proxy_6m: "基础对话 · 云雾白卡",
    proxy_1y_140g: "基础对话 · 年卡",
    proxy_1m_1g: "公益体验 · 云雾白卡",
    proxy_1y_15g: "公益体验 · 年卡",
    music_annual: "LYCO 无损音乐 · 年卡",
  };
  return map[product] || product;
}

const TIER_STYLES: Record<
  CardTier,
  {
    badge: string;
    subtitle: string;
    bg: string;
    border: string;
    glow: string;
    text: string;
    muted: string;
    chip: string;
    sheen: string;
  }
> = {
  black: {
    badge: "OBSIDIAN BLACK · PRO",
    subtitle: "RESIDENTIAL DEDICATED IP",
    bg: "bg-gradient-to-br from-zinc-950 via-neutral-900 to-zinc-950",
    border: "border-amber-400/50",
    glow: "shadow-[0_12px_40px_-10px_rgba(251,191,36,0.35)]",
    text: "text-amber-50",
    muted: "text-amber-200/70",
    chip: "bg-gradient-to-br from-amber-300 via-yellow-500 to-amber-600",
    sheen: "from-transparent via-amber-300/20 to-transparent",
  },
  gold: {
    badge: "CHAMPAGNE GOLD · EXPRESS",
    subtitle: "12.7 MB/S PRIORITY ANYTLS",
    bg: "bg-gradient-to-br from-amber-600 via-yellow-500 to-amber-700",
    border: "border-amber-200/70",
    glow: "shadow-[0_12px_36px_-10px_rgba(245,158,11,0.45)]",
    text: "text-zinc-950",
    muted: "text-zinc-900/75",
    chip: "bg-gradient-to-br from-amber-100 via-amber-200 to-yellow-400",
    sheen: "from-transparent via-white/35 to-transparent",
  },
  silver: {
    badge: "TITANIUM SILVER · LITE",
    subtitle: "ZERO-AD MULTI-REGION PASS",
    bg: "bg-gradient-to-br from-slate-300 via-zinc-200 to-slate-400",
    border: "border-white/80",
    glow: "shadow-[0_12px_34px_-10px_rgba(148,163,184,0.45)]",
    text: "text-slate-950",
    muted: "text-slate-700",
    chip: "bg-gradient-to-br from-white via-slate-200 to-slate-400",
    sheen: "from-transparent via-white/50 to-transparent",
  },
  white: {
    badge: "PEARL WHITE · STARTER",
    subtitle: "LAIN42 CLOUD MEMBER PASS",
    bg: "bg-gradient-to-br from-white via-zinc-50 to-slate-100",
    border: "border-zinc-300/90",
    glow: "shadow-[0_10px_30px_-10px_rgba(16,185,129,0.22)]",
    text: "text-zinc-900",
    muted: "text-zinc-500",
    chip: "bg-gradient-to-br from-emerald-200 via-teal-100 to-zinc-200",
    sheen: "from-transparent via-emerald-300/25 to-transparent",
  },
};

interface MembershipCardProps {
  tier: CardTier;
  title: string;
  holder: string;
  cardNo: string;
  expiresText: string;
  quotaText?: string;
  status?: string;
  compact?: boolean;
}

export function MembershipCard({
  tier,
  title,
  holder,
  cardNo,
  expiresText,
  quotaText,
  status = "active",
  compact = false,
}: MembershipCardProps) {
  const st = TIER_STYLES[tier];
  const isExpired = status !== "active";

  return (
    <div
      className={`relative overflow-hidden rounded-2xl border ${st.bg} ${st.border} ${st.glow} ${st.text} ${
        compact ? "p-4" : "p-5"
      } transition-transform duration-300 hover:-translate-y-0.5 select-none ${
        isExpired ? "opacity-65 grayscale-[40%]" : ""
      }`}
    >
      {/* Metallic specular sweep */}
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div
          className={`animate-card-sheen absolute inset-y-0 w-1/3 bg-gradient-to-r ${st.sheen}`}
        />
      </div>

      {/* Subtle radial glow orb */}
      <div className="pointer-events-none absolute -right-10 -top-10 h-32 w-32 rounded-full bg-white/15 blur-2xl" />

      <div className="relative z-10 flex flex-col justify-between gap-4">
        {/* Top Row: Brand + Tier Emblem */}
        <div className="flex items-start justify-between gap-2">
          <div className="space-y-0.5">
            <div className="flex items-center gap-1.5 text-[10px] font-bold tracking-[0.18em] uppercase">
              {tier === "black" ? (
                <Crown className="h-3.5 w-3.5 text-amber-400" />
              ) : tier === "gold" ? (
                <Sparkles className="h-3.5 w-3.5" />
              ) : (
                <ShieldCheck className="h-3.5 w-3.5" />
              )}
              <span>{st.badge}</span>
            </div>
            <div className={`text-[10px] tracking-widest uppercase ${st.muted}`}>
              {st.subtitle}
            </div>
          </div>
          <div className="flex items-center gap-1.5">
            {quotaText && (
              <span className="rounded-full bg-black/15 px-2 py-0.5 text-[10px] font-mono font-semibold backdrop-blur-xs">
                {quotaText}
              </span>
            )}
            <Wifi className={`h-4 w-4 rotate-90 ${st.muted}`} />
          </div>
        </div>

        {/* Middle: Metallic Chip + Card Number */}
        <div className="flex items-center gap-3">
          <div
            className={`h-7 w-10 rounded-md ${st.chip} border border-black/15 shadow-inner shrink-0`}
          />
          <div className="min-w-0">
            <div className="text-sm font-bold tracking-tight truncate">{title}</div>
            <div className={`font-mono text-xs tracking-[0.14em] truncate ${st.muted}`}>
              {cardNo}
            </div>
          </div>
        </div>

        {/* Bottom: Holder + Expiry + Status */}
        <div className="flex items-end justify-between text-[11px] pt-1 border-t border-current/10">
          <div>
            <div className={`text-[9px] uppercase tracking-widest ${st.muted}`}>
              HOLDER
            </div>
            <div className="font-mono font-semibold">{holder}</div>
          </div>
          <div className="text-right">
            <div className={`text-[9px] uppercase tracking-widest ${st.muted}`}>
              {isExpired ? "EXPIRED" : "VALID THRU"}
            </div>
            <div className="font-mono font-semibold">{expiresText}</div>
          </div>
        </div>
      </div>
    </div>
  );
}
