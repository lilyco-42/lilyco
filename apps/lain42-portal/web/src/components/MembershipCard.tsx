import Tilt from "react-parallax-tilt";
import { ArrowUpRight, Wifi } from "lucide-react";

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
    proxy_pro_1m: "Pro · 独享家宽月卡",
    proxy_pro_3m: "Pro · 独享家宽季卡",
    proxy_pro_1y: "Pro · 独享家宽年卡",
    proxy_pro_3y: "Pro · 独享家宽三年",
    proxy_1m_220g: "Gold · 标准极速月卡",
    proxy_3m: "Gold · 极速专线季卡",
    proxy_1y: "Gold · 极速专线年卡",
    proxy_3y: "Gold · 极速专线三年",
    proxy_1m: "Silver · 轻量极速月卡",
    proxy_15_3m: "Silver · 轻量季卡",
    proxy_15_1y: "Silver · 轻量年卡",
    proxy_6m: "Pearl · 基础对话月卡",
    proxy_1y_140g: "Pearl · 基础年卡",
    proxy_1m_1g: "Pearl · 体验月卡",
    proxy_1y_15g: "Pearl · 公益年卡",
    music_annual: "Silver · 无损音乐年卡",
  };
  return map[product] || product;
}

/**
 * X Money inspired minimalist metal palette — strictly <= 3 colors per card:
 * base metal surface + tonal specular sheen + 1 high-contrast etched ink.
 */
const TIER_STYLES: Record<
  CardTier,
  {
    tierMark: string;
    bg: string;
    rim: string;
    shadow: string;
    ink: string;
    subInk: string;
    chip: string;
    monogram: string;
    sheen: string;
    glare: string;
    pill: string;
  }
> = {
  black: {
    tierMark: "BLACK PRO",
    bg: "bg-gradient-to-br from-[#141417] via-[#0B0B0D] to-[#1A1815]",
    rim: "border-[#D4AF37]/35 shadow-[inset_0_1px_0_rgba(255,255,255,0.14)]",
    shadow: "shadow-[0_18px_40px_-14px_rgba(0,0,0,0.85)]",
    ink: "text-[#F5F5F7]",
    subInk: "text-[#D4AF37]/80",
    chip: "bg-gradient-to-br from-[#E6C875] via-[#C8A24B] to-[#9E7B2F]",
    monogram: "border-[#D4AF37]/40 text-[#E6C875]",
    sheen: "from-transparent via-[#D4AF37]/14 to-transparent",
    glare: "#E6C875",
    pill: "bg-[#D4AF37]/15 text-[#E6C875] border border-[#D4AF37]/30",
  },
  gold: {
    tierMark: "SATIN GOLD",
    bg: "bg-gradient-to-br from-[#D8BE8B] via-[#C4A364] to-[#AE8B4B]",
    rim: "border-[#F3E5C3]/60 shadow-[inset_0_1px_0_rgba(255,255,255,0.45)]",
    shadow: "shadow-[0_18px_36px_-14px_rgba(174,139,75,0.5)]",
    ink: "text-[#161410]",
    subInk: "text-[#161410]/60",
    chip: "bg-gradient-to-br from-[#F3E5C3] via-[#DFC48A] to-[#B59252]",
    monogram: "border-[#161410]/25 text-[#161410]",
    sheen: "from-transparent via-white/30 to-transparent",
    glare: "#FFF8E7",
    pill: "bg-[#161410]/12 text-[#161410] border border-[#161410]/20",
  },
  silver: {
    tierMark: "TITANIUM SILVER",
    bg: "bg-gradient-to-br from-[#E2E4E9] via-[#CFD3DB] to-[#B9BFC9]",
    rim: "border-white/75 shadow-[inset_0_1px_0_rgba(255,255,255,0.65)]",
    shadow: "shadow-[0_18px_36px_-14px_rgba(100,116,139,0.4)]",
    ink: "text-[#111317]",
    subInk: "text-[#111317]/55",
    chip: "bg-gradient-to-br from-[#F8F9FA] via-[#D8DCE3] to-[#AEB4BF]",
    monogram: "border-[#111317]/20 text-[#111317]",
    sheen: "from-transparent via-white/45 to-transparent",
    glare: "#FFFFFF",
    pill: "bg-[#111317]/10 text-[#111317] border border-[#111317]/15",
  },
  white: {
    tierMark: "CERAMIC WHITE",
    bg: "bg-gradient-to-br from-[#FFFFFF] via-[#F5F6F8] to-[#EAECEF]",
    rim: "border-[#D4D7DD] shadow-[inset_0_1px_0_rgba(255,255,255,0.9)]",
    shadow: "shadow-[0_16px_32px_-14px_rgba(15,23,42,0.16)]",
    ink: "text-[#111113]",
    subInk: "text-[#111113]/50",
    chip: "bg-gradient-to-br from-[#F1F2F4] via-[#E2E4E8] to-[#CDD1D8]",
    monogram: "border-[#111113]/15 text-[#111113]",
    sheen: "from-transparent via-white/70 to-transparent",
    glare: "#FFFFFF",
    pill: "bg-[#111113]/8 text-[#111113] border border-[#111113]/15",
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
  onClick?: () => void;
  actionLabel?: string;
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
  onClick,
  actionLabel,
}: MembershipCardProps) {
  const st = TIER_STYLES[tier];
  const isExpired = status !== "active";

  return (
    <Tilt
      tiltMaxAngleX={8}
      tiltMaxAngleY={8}
      glareEnable
      glareMaxOpacity={0.22}
      glareColor={st.glare}
      glarePosition="all"
      glareBorderRadius="20px"
      scale={onClick ? 1.018 : 1.008}
      transitionSpeed={500}
      className="rounded-[20px]"
    >
      <div
        role={onClick ? "button" : undefined}
        tabIndex={onClick ? 0 : undefined}
        onClick={onClick}
        onKeyDown={(e) => {
          if (onClick && (e.key === "Enter" || e.key === " ")) {
            e.preventDefault();
            onClick();
          }
        }}
        className={`group relative aspect-[1.586/1] w-full overflow-hidden rounded-[20px] border ${st.bg} ${st.rim} ${st.shadow} ${st.ink} ${
          compact ? "p-4" : "p-5 sm:p-6"
        } select-none flex flex-col justify-between ${
          onClick ? "cursor-pointer" : ""
        } ${isExpired ? "opacity-60 grayscale-[45%]" : ""}`}
      >
        {/* Subtle Satin Metallic Sheen */}
        <div className="pointer-events-none absolute inset-0 overflow-hidden">
          <div
            className={`animate-card-sheen absolute inset-y-0 w-1/3 bg-gradient-to-r ${st.sheen}`}
          />
        </div>

        {/* Fine brushed texture overlay */}
        <div
          className="pointer-events-none absolute inset-0 opacity-[0.04]"
          style={{
            backgroundImage:
              "repeating-linear-gradient(125deg, currentColor 0px, currentColor 1px, transparent 1px, transparent 4px)",
          }}
        />

        {/* Top Row: Minimal Geometric Monogram + Contactless Mark */}
        <div className="relative z-10 flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <span
              className={`inline-flex h-7 w-7 items-center justify-center rounded-lg border font-mono text-xs font-bold tracking-tight ${st.monogram}`}
            >
              L42
            </span>
            <div className="leading-none">
              <div className="text-[10px] font-semibold tracking-[0.22em] uppercase">
                LAIN42
              </div>
              <div
                className={`mt-0.5 text-[9px] font-medium tracking-[0.2em] uppercase ${st.subInk}`}
              >
                {st.tierMark}
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            {quotaText && (
              <span
                className={`rounded-full px-2 py-0.5 font-mono text-[10px] font-medium ${st.pill}`}
              >
                {quotaText}
              </span>
            )}
            <Wifi className={`h-4 w-4 rotate-90 ${st.subInk}`} />
          </div>
        </div>

        {/* Middle Row: Brushed EMV Chip + Etched Title & Number */}
        <div className="relative z-10 flex items-center gap-3.5">
          <div
            className={`h-7 w-10 rounded-[6px] ${st.chip} border border-black/15 shadow-inner shrink-0 relative overflow-hidden`}
          >
            <div className="absolute inset-x-0 top-1/2 h-px bg-black/15" />
            <div className="absolute inset-y-0 left-1/3 w-px bg-black/15" />
            <div className="absolute inset-y-0 right-1/3 w-px bg-black/15" />
          </div>
          <div className="min-w-0">
            <div className="text-sm font-semibold tracking-tight truncate">
              {title}
            </div>
            <div
              className={`font-mono text-[11px] tracking-[0.16em] truncate ${st.subInk}`}
            >
              {cardNo}
            </div>
          </div>
        </div>

        {/* Bottom Row: Minimalist Holder + Valid Thru / Action */}
        <div className="relative z-10 flex items-end justify-between pt-1">
          <div className="min-w-0">
            <div
              className={`text-[8px] font-medium uppercase tracking-[0.24em] ${st.subInk}`}
            >
              MEMBER
            </div>
            <div className="mt-0.5 font-mono text-xs font-medium tracking-wider truncate">
              {holder}
            </div>
          </div>

          <div className="text-right shrink-0">
            {actionLabel ? (
              <span
                className={`inline-flex items-center gap-0.5 rounded-full px-2.5 py-1 text-[10px] font-semibold tracking-wide transition-opacity group-hover:opacity-90 ${st.pill}`}
              >
                {actionLabel}
                <ArrowUpRight className="h-3 w-3" />
              </span>
            ) : (
              <>
                <div
                  className={`text-[8px] font-medium uppercase tracking-[0.24em] ${st.subInk}`}
                >
                  {isExpired ? "EXPIRED" : "VALID THRU"}
                </div>
                <div className="mt-0.5 font-mono text-xs font-medium tracking-wider">
                  {expiresText}
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    </Tilt>
  );
}
