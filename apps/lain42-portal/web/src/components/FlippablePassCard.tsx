import { useState } from "react";
import { Check, Copy, ExternalLink, RotateCw } from "lucide-react";
import { Button } from "./ui/primitives";
import {
  type CardTier,
  MembershipCard,
} from "./MembershipCard";

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

const BACK_THEME: Record<CardTier, string> = {
  black:
    "bg-gradient-to-br from-zinc-950 via-neutral-900 to-black border-amber-400/50 text-amber-50 shadow-[0_14px_42px_-10px_rgba(251,191,36,0.35)]",
  gold:
    "bg-gradient-to-br from-zinc-900 via-amber-950 to-zinc-950 border-amber-400/60 text-amber-50 shadow-[0_14px_38px_-10px_rgba(245,158,11,0.4)]",
  silver:
    "bg-gradient-to-br from-slate-900 via-slate-800 to-zinc-900 border-slate-300/60 text-slate-50 shadow-[0_14px_36px_-10px_rgba(148,163,184,0.4)]",
  white:
    "bg-gradient-to-br from-zinc-900 via-zinc-800 to-slate-900 border-emerald-400/40 text-zinc-50 shadow-[0_12px_32px_-10px_rgba(16,185,129,0.25)]",
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
            style={{ backfaceVisibility: "hidden", WebkitBackfaceVisibility: "hidden" }}
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
            className={`absolute inset-0 rounded-2xl border overflow-hidden cursor-pointer flex flex-col justify-between ${BACK_THEME[tier]}`}
          >
            {/* Magnetic Stripe */}
            <div className="mt-3 h-7 w-full bg-black/75 border-y border-white/10 flex items-center justify-between px-4">
              <span className="font-mono text-[10px] tracking-[0.2em] text-white/50">
                LAIN42 ENCRYPTED PASS
              </span>
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  setFlipped(false);
                }}
                className="inline-flex items-center gap-1 text-[10px] text-white/75 hover:text-white cursor-pointer"
              >
                <RotateCw className="h-3 w-3" /> 翻回正面
              </button>
            </div>

            {/* Subscription Controls on Card Back */}
            <div
              className="p-4 pt-2 space-y-2.5 flex-1 flex flex-col justify-center"
              onClick={(e) => e.stopPropagation()}
            >
              {subToken && status === "active" ? (
                <>
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-[11px] font-semibold text-white/85">
                      选择订阅格式
                    </span>
                    <select
                      value={format}
                      onChange={(e) => setFormat(e.target.value)}
                      className="h-7 rounded-md border border-white/20 bg-black/50 px-2 text-[11px] text-white focus:outline-none focus:ring-1 focus:ring-emerald-400"
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
                    onFocus={(e) => e.currentTarget.select()}
                    className="h-8 w-full rounded-md border border-white/15 bg-black/50 px-2.5 font-mono text-[11px] text-emerald-300"
                  />

                  <div className="flex items-center gap-2">
                    <Button
                      size="sm"
                      variant="emerald"
                      className="flex-1 h-8 text-xs"
                      onClick={handleCopy}
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
                    </Button>
                    {isClash && (
                      <a
                        href={`clash://install-config?url=${encodeURIComponent(
                          resolvedUrl
                        )}`}
                        className="inline-flex h-8 items-center justify-center gap-1 rounded-md border border-white/25 bg-white/10 px-3 text-xs font-medium text-white hover:bg-white/20 transition-colors"
                      >
                        一键导入 <ExternalLink className="h-3 w-3" />
                      </a>
                    )}
                  </div>
                </>
              ) : (
                <div className="space-y-2">
                  <div className="text-[11px] text-white/75">
                    {status === "active" ? "授权激活码 / 卡号" : "该订阅已到期"}
                  </div>
                  <input
                    readOnly
                    value={licenseKey}
                    onFocus={(e) => e.currentTarget.select()}
                    className="h-8 w-full rounded-md border border-white/15 bg-black/50 px-2.5 font-mono text-xs text-amber-300"
                  />
                  <Button
                    size="sm"
                    variant="emerald"
                    className="w-full h-8 text-xs"
                    onClick={handleCopy}
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
                  </Button>
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
          {flipped ? "点击卡片翻回金属卡正面" : "请点击卡片翻转查看订阅链接 / 激活信息"}
        </button>
      </div>
    </div>
  );
}
