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

const BACK_STYLES: Record<
  CardTier,
  {
    surface: string;
    stripe: string;
    stripeText: string;
    label: string;
    control: string;
    primaryBtn: string;
    secondaryBtn: string;
    sheen: string;
  }
> = {
  black: {
    surface:
      "bg-gradient-to-br from-zinc-950 via-neutral-900 to-zinc-950 border-amber-400/55 text-amber-50 shadow-[0_14px_42px_-10px_rgba(251,191,36,0.38)]",
    stripe: "bg-black/80 border-amber-400/20",
    stripeText: "text-amber-300/75",
    label: "text-amber-200/80",
    control:
      "border-amber-400/30 bg-black/55 text-amber-100 focus:ring-amber-400/60",
    primaryBtn:
      "bg-gradient-to-r from-amber-400 to-yellow-500 text-zinc-950 hover:from-amber-300 hover:to-yellow-400",
    secondaryBtn:
      "border-amber-400/35 bg-amber-400/10 text-amber-100 hover:bg-amber-400/20",
    sheen: "from-transparent via-amber-300/15 to-transparent",
  },
  gold: {
    surface:
      "bg-gradient-to-br from-amber-600 via-yellow-500 to-amber-700 border-amber-200/75 text-zinc-950 shadow-[0_14px_38px_-10px_rgba(245,158,11,0.48)]",
    stripe: "bg-zinc-950/75 border-amber-200/30",
    stripeText: "text-amber-200/90",
    label: "text-zinc-950/80",
    control:
      "border-zinc-950/20 bg-white/45 text-zinc-950 placeholder:text-zinc-800 backdrop-blur-xs focus:ring-zinc-900/40",
    primaryBtn: "bg-zinc-950 text-amber-300 hover:bg-zinc-900",
    secondaryBtn:
      "border-zinc-950/25 bg-white/35 text-zinc-950 hover:bg-white/55",
    sheen: "from-transparent via-white/30 to-transparent",
  },
  silver: {
    surface:
      "bg-gradient-to-br from-slate-300 via-zinc-200 to-slate-400 border-white/85 text-slate-950 shadow-[0_14px_36px_-10px_rgba(148,163,184,0.48)]",
    stripe: "bg-slate-900/80 border-white/30",
    stripeText: "text-slate-200/90",
    label: "text-slate-800",
    control:
      "border-slate-900/20 bg-white/60 text-slate-950 backdrop-blur-xs focus:ring-slate-900/40",
    primaryBtn: "bg-slate-900 text-white hover:bg-slate-800",
    secondaryBtn:
      "border-slate-900/25 bg-white/45 text-slate-950 hover:bg-white/70",
    sheen: "from-transparent via-white/45 to-transparent",
  },
  white: {
    surface:
      "bg-gradient-to-br from-white via-zinc-50 to-slate-100 border-zinc-300/90 text-zinc-900 shadow-[0_12px_32px_-10px_rgba(16,185,129,0.24)]",
    stripe: "bg-zinc-900/85 border-zinc-300/40",
    stripeText: "text-emerald-300/90",
    label: "text-zinc-600",
    control:
      "border-zinc-300 bg-white text-zinc-900 focus:ring-emerald-500/50",
    primaryBtn: "bg-emerald-600 text-white hover:bg-emerald-700",
    secondaryBtn:
      "border-zinc-300 bg-zinc-100 text-zinc-800 hover:bg-zinc-200",
    sheen: "from-transparent via-emerald-300/20 to-transparent",
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
      {/* 3D Perspective Container */}
      <div style={{ perspective: "1200px" }} className="w-full">
        <div
          style={{
            transformStyle: "preserve-3d",
            transition: "transform 0.65s cubic-bezier(0.22, 1, 0.36, 1)",
            transform: flipped ? "rotateY(180deg)" : "rotateY(0deg)",
          }}
          className="relative w-full min-h-[218px]"
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

          {/* BACK FACE — Matching Front Metallic Surface + Click Anywhere Else to Flip Back */}
          <div
            style={{
              backfaceVisibility: "hidden",
              WebkitBackfaceVisibility: "hidden",
              transform: "rotateY(180deg)",
            }}
            onClick={() => setFlipped(false)}
            className={`absolute inset-0 rounded-2xl border overflow-hidden cursor-pointer flex flex-col justify-between ${st.surface}`}
          >
            {/* Matching Specular Sweep */}
            <div className="pointer-events-none absolute inset-0 overflow-hidden">
              <div
                className={`animate-card-sheen absolute inset-y-0 w-1/3 bg-gradient-to-r ${st.sheen}`}
              />
            </div>

            {/* Subtle Radial Glow */}
            <div className="pointer-events-none absolute -left-8 -bottom-8 h-28 w-28 rounded-full bg-white/15 blur-2xl" />

            {/* Magnetic Stripe */}
            <div
              className={`relative z-10 mt-3 h-7 w-full border-y flex items-center justify-between px-4 ${st.stripe}`}
            >
              <span
                className={`font-mono text-[10px] tracking-[0.2em] ${st.stripeText}`}
              >
                LAIN42 ENCRYPTED PASS
              </span>
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  setFlipped(false);
                }}
                className="inline-flex items-center gap-1 text-[10px] text-white/85 hover:text-white cursor-pointer"
              >
                <RotateCw className="h-3 w-3" /> 翻回正面
              </button>
            </div>

            {/* Back Controls (only interactive elements stop propagation; clicking empty space flips back!) */}
            <div className="relative z-10 p-4 pt-2 space-y-2.5 flex-1 flex flex-col justify-center">
              {subToken && status === "active" ? (
                <>
                  <div className="flex items-center justify-between gap-2">
                    <span className={`text-[11px] font-semibold ${st.label}`}>
                      订阅格式
                    </span>
                    <select
                      value={format}
                      onClick={(e) => e.stopPropagation()}
                      onChange={(e) => setFormat(e.target.value)}
                      className={`h-7 rounded-md border px-2 text-[11px] font-medium focus:outline-none focus:ring-2 ${st.control}`}
                    >
                      <option value="clash">🖥️ Clash Verge Rev</option>
                      <option value="base64">📱 Karing / Base64 通用</option>
                      <option value="clashjp">🇯🇵 日本 · Clash</option>
                      <option value="base64jp">🇯🇵 日本 · Base64</option>
                      <option value="clashcommunity">🌐 普通已验证 · Clash</option>
                      <option value="clashall">🌐 完整目录 · Clash</option>
                    </select>
                  </div>

                  <input
                    readOnly
                    value={displayValue}
                    onClick={(e) => e.stopPropagation()}
                    onFocus={(e) => e.currentTarget.select()}
                    className={`h-8 w-full rounded-md border px-2.5 font-mono text-[11px] focus:outline-none focus:ring-2 ${st.control}`}
                  />

                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={handleCopy}
                      className={`inline-flex h-8 flex-1 items-center justify-center gap-1.5 rounded-md px-3 text-xs font-semibold shadow-xs transition-colors cursor-pointer ${st.primaryBtn}`}
                    >
                      {copied ? (
                        <>
                          <Check className="h-3.5 w-3.5" /> 已复制订阅
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
                  <div className={`text-[11px] font-semibold ${st.label}`}>
                    {status === "active" ? "授权激活码 / 卡号" : "该订阅已到期"}
                  </div>
                  <input
                    readOnly
                    value={licenseKey}
                    onClick={(e) => e.stopPropagation()}
                    onFocus={(e) => e.currentTarget.select()}
                    className={`h-8 w-full rounded-md border px-2.5 font-mono text-xs focus:outline-none focus:ring-2 ${st.control}`}
                  />
                  <button
                    type="button"
                    onClick={handleCopy}
                    className={`inline-flex h-8 w-full items-center justify-center gap-1.5 rounded-md px-3 text-xs font-semibold shadow-xs transition-colors cursor-pointer ${st.primaryBtn}`}
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

      {/* Subtle hint below the card */}
      <div className="text-center">
        <button
          type="button"
          onClick={() => setFlipped((f) => !f)}
          className="inline-flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground transition-colors cursor-pointer"
        >
          <RotateCw className="h-3 w-3" />
          {flipped ? "点击卡面空白处翻回正面" : "请点击卡片翻转查看订阅链接 / 激活信息"}
        </button>
      </div>
    </div>
  );
}
