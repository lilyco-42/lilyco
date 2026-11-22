import { useState } from "react";
import { Check, Copy, ExternalLink, RotateCw } from "lucide-react";
import { type CardTier, MembershipCard } from "./MembershipCard";

interface FlippablePassCardProps {
  tier: CardTier;
  title: string;
  holder: string;
  licenseKey: string;
  expiresText: string;
  quotaText?: string;
  status: string;
  rawSubUrl: string;
}

/**
 * Matching X Money <= 3-color minimalist metallic finish for card back.
 */
const BACK_STYLES: Record<
  CardTier,
  {
    surface: string;
    stripe: string;
    stripeInk: string;
    label: string;
    control: string;
    primaryBtn: string;
    secondaryBtn: string;
    sheen: string;
  }
> = {
  black: {
    surface:
      "bg-gradient-to-br from-[#141417] via-[#0B0B0D] to-[#1A1815] border-[#D4AF37]/35 text-[#F5F5F7] shadow-[0_18px_40px_-14px_rgba(0,0,0,0.85),inset_0_1px_0_rgba(255,255,255,0.14)]",
    stripe: "bg-[#050506] border-[#D4AF37]/20",
    stripeInk: "text-[#D4AF37]/80",
    label: "text-[#D4AF37]/80",
    control:
      "border-[#D4AF37]/25 bg-black/50 text-[#F5F5F7] focus:ring-[#D4AF37]/50",
    primaryBtn: "bg-[#D4AF37] text-[#0B0B0D] hover:bg-[#E2C15A]",
    secondaryBtn:
      "border-[#D4AF37]/30 bg-transparent text-[#E6C875] hover:bg-[#D4AF37]/10",
    sheen: "from-transparent via-[#D4AF37]/12 to-transparent",
  },
  gold: {
    surface:
      "bg-gradient-to-br from-[#D8BE8B] via-[#C4A364] to-[#AE8B4B] border-[#F3E5C3]/60 text-[#161410] shadow-[0_18px_36px_-14px_rgba(174,139,75,0.5),inset_0_1px_0_rgba(255,255,255,0.45)]",
    stripe: "bg-[#161410]/85 border-[#F3E5C3]/25",
    stripeInk: "text-[#E6CFA0]",
    label: "text-[#161410]/75",
    control:
      "border-[#161410]/20 bg-white/40 text-[#161410] backdrop-blur-xs focus:ring-[#161410]/40",
    primaryBtn: "bg-[#161410] text-[#F3E5C3] hover:bg-[#26221B]",
    secondaryBtn:
      "border-[#161410]/25 bg-white/30 text-[#161410] hover:bg-white/50",
    sheen: "from-transparent via-white/25 to-transparent",
  },
  silver: {
    surface:
      "bg-gradient-to-br from-[#E2E4E9] via-[#CFD3DB] to-[#B9BFC9] border-white/75 text-[#111317] shadow-[0_18px_36px_-14px_rgba(100,116,139,0.4),inset_0_1px_0_rgba(255,255,255,0.65)]",
    stripe: "bg-[#111317]/85 border-white/25",
    stripeInk: "text-[#E2E4E9]",
    label: "text-[#111317]/70",
    control:
      "border-[#111317]/20 bg-white/55 text-[#111317] backdrop-blur-xs focus:ring-[#111317]/35",
    primaryBtn: "bg-[#111317] text-white hover:bg-[#232730]",
    secondaryBtn:
      "border-[#111317]/20 bg-white/45 text-[#111317] hover:bg-white/70",
    sheen: "from-transparent via-white/40 to-transparent",
  },
  white: {
    surface:
      "bg-gradient-to-br from-[#FFFFFF] via-[#F5F6F8] to-[#EAECEF] border-[#D4D7DD] text-[#111113] shadow-[0_16px_32px_-14px_rgba(15,23,42,0.16),inset_0_1px_0_rgba(255,255,255,0.9)]",
    stripe: "bg-[#111113] border-[#D4D7DD]/30",
    stripeInk: "text-[#EAECEF]",
    label: "text-[#111113]/60",
    control:
      "border-[#D4D7DD] bg-white text-[#111113] focus:ring-[#111113]/25",
    primaryBtn: "bg-[#111113] text-white hover:bg-[#27272A]",
    secondaryBtn:
      "border-[#D4D7DD] bg-white text-[#111113] hover:bg-[#F5F6F8]",
    sheen: "from-transparent via-white/60 to-transparent",
  },
};

export function FlippablePassCard({
  tier,
  title,
  holder,
  licenseKey,
  expiresText,
  quotaText,
  status,
  rawSubUrl,
}: FlippablePassCardProps) {
  const [flipped, setFlipped] = useState(false);
  const [format, setFormat] = useState("clash");
  const [copied, setCopied] = useState(false);

  const st = BACK_STYLES[tier];
  const subMatch = rawSubUrl.match(/\/panel\/api\/sub\/([A-Za-z0-9_-]+)/);
  const subToken = subMatch ? subMatch[1] : "";
  const resolvedUrl = subToken
    ? `https://lain42.top/studio/api/sub/${encodeURIComponent(subToken)}/${format}`
    : rawSubUrl;
  const displayValue = resolvedUrl || licenseKey;
  const isClash = format.startsWith("clash");

  function handleCopy(e: React.MouseEvent) {
    e.stopPropagation();
    navigator.clipboard.writeText(displayValue).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    });
  }

  return (
    <div className="space-y-2">
      {/* 3D Perspective Container matching exact 1.586:1 physical card ratio */}
      <div style={{ perspective: "1200px" }} className="w-full">
        <div
          style={{
            transformStyle: "preserve-3d",
            transition: "transform 0.65s cubic-bezier(0.22, 1, 0.36, 1)",
            transform: flipped ? "rotateY(180deg)" : "rotateY(0deg)",
          }}
          className="relative w-full"
        >
          {/* FRONT FACE */}
          <div
            style={{
              backfaceVisibility: "hidden",
              WebkitBackfaceVisibility: "hidden",
            }}
            className="w-full"
          >
            <MembershipCard
              tier={tier}
              title={title}
              holder={holder}
              cardNo={licenseKey}
              expiresText={expiresText}
              quotaText={quotaText}
              status={status}
              onClick={() => setFlipped(true)}
            />
          </div>

          {/* BACK FACE */}
          <div
            style={{
              backfaceVisibility: "hidden",
              WebkitBackfaceVisibility: "hidden",
              transform: "rotateY(180deg)",
            }}
            onClick={() => setFlipped(false)}
            className={`absolute inset-0 rounded-[20px] border overflow-hidden cursor-pointer flex flex-col justify-between ${st.surface}`}
          >
            {/* Matching Specular Sweep */}
            <div className="pointer-events-none absolute inset-0 overflow-hidden">
              <div
                className={`animate-card-sheen absolute inset-y-0 w-1/3 bg-gradient-to-r ${st.sheen}`}
              />
            </div>

            {/* Brushed texture */}
            <div
              className="pointer-events-none absolute inset-0 opacity-[0.04]"
              style={{
                backgroundImage:
                  "repeating-linear-gradient(125deg, currentColor 0px, currentColor 1px, transparent 1px, transparent 4px)",
              }}
            />

            {/* Minimalist Magnetic Stripe */}
            <div
              className={`relative z-10 mt-4 h-8 w-full border-y flex items-center justify-between px-4 ${st.stripe}`}
            >
              <span
                className={`font-mono text-[9px] tracking-[0.24em] uppercase ${st.stripeInk}`}
              >
                LAIN42 PASS · ENCRYPTED
              </span>
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  setFlipped(false);
                }}
                className={`inline-flex items-center gap-1 text-[10px] hover:opacity-80 cursor-pointer ${st.stripeInk}`}
              >
                <RotateCw className="h-3 w-3" /> 翻回正面
              </button>
            </div>

            {/* Back Controls (click anywhere outside inputs/buttons to flip back) */}
            <div className="relative z-10 px-5 pb-5 pt-2 space-y-2.5 flex-1 flex flex-col justify-center">
              {subToken && status === "active" ? (
                <>
                  <div className="flex items-center justify-between gap-2">
                    <span
                      className={`text-[10px] font-semibold uppercase tracking-[0.16em] ${st.label}`}
                    >
                      SUBSCRIPTION
                    </span>
                    <select
                      value={format}
                      onClick={(e) => e.stopPropagation()}
                      onChange={(e) => setFormat(e.target.value)}
                      className={`h-7 rounded-md border px-2 text-[11px] font-medium focus:outline-none focus:ring-1 ${st.control}`}
                    >
                      <option value="clash">Clash Verge Rev</option>
                      <option value="base64">Karing / Base64</option>
                      <option value="clashjp">JP · Clash</option>
                      <option value="base64jp">JP · Base64</option>
                      <option value="clashcommunity">Community · Clash</option>
                      <option value="clashall">All · Clash</option>
                    </select>
                  </div>

                  <input
                    readOnly
                    value={displayValue}
                    onClick={(e) => e.stopPropagation()}
                    onFocus={(e) => e.currentTarget.select()}
                    className={`h-8 w-full rounded-md border px-2.5 font-mono text-[11px] focus:outline-none focus:ring-1 ${st.control}`}
                  />

                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={handleCopy}
                      className={`inline-flex h-8 flex-1 items-center justify-center gap-1.5 rounded-md px-3 text-xs font-semibold transition-colors cursor-pointer ${st.primaryBtn}`}
                    >
                      {copied ? (
                        <>
                          <Check className="h-3.5 w-3.5" /> 已复制
                        </>
                      ) : (
                        <>
                          <Copy className="h-3.5 w-3.5" />
                          {isClash ? "复制 Clash 订阅" : "复制 Base64 订阅"}
                        </>
                      )}
                    </button>
                    {isClash && (
                      <a
                        href={`clash://install-config?url=${encodeURIComponent(
                          resolvedUrl
                        )}`}
                        onClick={(e) => e.stopPropagation()}
                        className={`inline-flex h-8 items-center justify-center gap-1 rounded-md border px-3 text-xs font-semibold transition-colors ${st.secondaryBtn}`}
                      >
                        一键导入 <ExternalLink className="h-3 w-3" />
                      </a>
                    )}
                  </div>
                </>
              ) : (
                <div className="space-y-2">
                  <div
                    className={`text-[10px] font-semibold uppercase tracking-[0.16em] ${st.label}`}
                  >
                    {status === "active" ? "LICENSE KEY" : "EXPIRED"}
                  </div>
                  <input
                    readOnly
                    value={licenseKey}
                    onClick={(e) => e.stopPropagation()}
                    onFocus={(e) => e.currentTarget.select()}
                    className={`h-8 w-full rounded-md border px-2.5 font-mono text-xs focus:outline-none focus:ring-1 ${st.control}`}
                  />
                  <button
                    type="button"
                    onClick={handleCopy}
                    className={`inline-flex h-8 w-full items-center justify-center gap-1.5 rounded-md px-3 text-xs font-semibold transition-colors cursor-pointer ${st.primaryBtn}`}
                  >
                    {copied ? (
                      <>
                        <Check className="h-3.5 w-3.5" /> 已复制激活码
                      </>
                    ) : (
                      <>
                        <Copy className="h-3.5 w-3.5" /> 复制激活码
                      </>
                    )}
                  </button>
                </div>
              )}
            </div>
          </div>
        </div>
      </div>

      <div className="text-center">
        <button
          type="button"
          onClick={() => setFlipped((f) => !f)}
          className="inline-flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground transition-colors cursor-pointer"
        >
          <RotateCw className="h-3 w-3" />
          {flipped ? "点击卡面空白处翻回正面" : "请点击卡片翻转查看订阅详情"}
        </button>
      </div>
    </div>
  );
}
