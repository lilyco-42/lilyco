import { useEffect, useState } from "react";
import { CreditCard, Gift, ShieldAlert, ShoppingBag, Ticket } from "lucide-react";
import { apiFetch, type MeResponse, type StudioProduct } from "../lib/api";
import { Badge, Button, Card, Input } from "../components/ui/primitives";

interface ShopPageProps {
  me: MeResponse | null;
  onRefreshMe: () => void;
  onNeedAuth: () => void;
}

const TOPUP_TIERS = [
  { code: "points_cny_15", cny: 15, pts: 150, label: "轻量月卡刚好" },
  { code: "points_cny_30", cny: 30, pts: 300, label: "标准 220GB 月卡" },
  { code: "points_cny_100", cny: 100, pts: 1000, label: "季卡 / AI 高频" },
  { code: "points_cny_300", cny: 300, pts: 3000, label: "开发者主力包" },
];

export function ShopPage({ me, onRefreshMe, onNeedAuth }: ShopPageProps) {
  const [products, setProducts] = useState<StudioProduct[]>([]);
  const [filter, setFilter] = useState<"all" | "proxy" | "software">("all");
  const [redeemCode, setRedeemCode] = useState("");
  const [feedback, setFeedback] = useState<{ ok: boolean; msg: string } | null>(null);
  const [busy, setBusy] = useState<string>("");

  useEffect(() => {
    apiFetch<{ products: StudioProduct[] }>("/studio/api/products")
      .then((res) => setProducts((res.products || []).filter((p) => p.sale_open !== false)))
      .catch(() => {});
  }, []);

  const handlePurchase = async (p: StudioProduct) => {
    if (!me) {
      onNeedAuth();
      return;
    }
    setBusy(p.code);
    setFeedback(null);
    try {
      const ref = `web_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
      await apiFetch("/studio/api/points/purchase", {
        method: "POST",
        body: JSON.stringify({ product: p.code, reference: ref }),
      });
      setFeedback({ ok: true, msg: `购买成功：「${p.name}」已发放到我的账户！` });
      onRefreshMe();
    } catch (e) {
      setFeedback({ ok: false, msg: (e as Error).message });
    } finally {
      setBusy("");
    }
  };

  const handleRedeem = async () => {
    if (!me) {
      onNeedAuth();
      return;
    }
    if (!redeemCode.trim()) return;
    setBusy("redeem");
    setFeedback(null);
    try {
      const res = await apiFetch<{ message?: string }>("/studio/api/redeem", {
        method: "POST",
        body: JSON.stringify({ code: redeemCode.trim() }),
      });
      setFeedback({ ok: true, msg: res.message || "卡密兑换成功！" });
      setRedeemCode("");
      onRefreshMe();
    } catch (e) {
      setFeedback({ ok: false, msg: (e as Error).message });
    } finally {
      setBusy("");
    }
  };

  const handleTopup = async (tierCode: string) => {
    if (!me) {
      onNeedAuth();
      return;
    }
    setBusy(tierCode);
    setFeedback(null);
    try {
      const ref = `af_${Date.now()}`;
      const res = await apiFetch<{ automatic_checkout_url?: string; checkout_url?: string }>(
        "/studio/api/payments/afdian/intents",
        {
          method: "POST",
          body: JSON.stringify({ product: tierCode, reference: ref }),
        }
      );
      const url = res.automatic_checkout_url || res.checkout_url;
      if (url) {
        window.open(url, "_blank", "noopener,noreferrer");
        setFeedback({ ok: true, msg: "已在新标签页打开爱发电专属收银台，付款后将自动到账。" });
      }
    } catch (e) {
      setFeedback({ ok: false, msg: (e as Error).message });
    } finally {
      setBusy("");
    }
  };

  const filtered = products.filter((p) => {
    if (filter === "proxy") return p.code.startsWith("proxy_");
    if (filter === "software") return !p.code.startsWith("proxy_");
    return true;
  });

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">数字商品商城与积分收银台</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            1 元 = 10 积分。代理专线属于不可复制高价值资源，仅支持充值付费积分购买；软件与工具可优先抵扣免费赠送积分。
          </p>
        </div>
        {me && (
          <div className="flex items-center gap-2">
            <Badge variant="emerald">充值积分: {me.points.paid_balance}</Badge>
            <Badge variant="amber">赠送积分: {me.points.bonus_balance}</Badge>
          </div>
        )}
      </div>

      {feedback && (
        <Card
          className={`p-3.5 text-sm ${
            feedback.ok
              ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
              : "border-destructive/40 bg-destructive/10 text-destructive"
          }`}
        >
          {feedback.msg}
        </Card>
      )}

      {/* Topup + Redeem Row */}
      <div className="grid gap-4 lg:grid-cols-3">
        <Card className="p-5 lg:col-span-2">
          <div className="mb-3 flex items-center gap-2 text-sm font-semibold">
            <CreditCard className="h-4 w-4 text-emerald-500" />
            快捷积分充值（爱发电自动核验到账）
          </div>
          <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-4">
            {TOPUP_TIERS.map((t) => (
              <button
                key={t.code}
                disabled={busy === t.code}
                onClick={() => handleTopup(t.code)}
                className="flex flex-col items-start rounded-lg border bg-background p-3 text-left transition-colors hover:border-emerald-500 hover:bg-emerald-500/5 cursor-pointer"
              >
                <span className="text-lg font-bold">¥{t.cny}</span>
                <span className="text-xs font-medium text-emerald-600 dark:text-emerald-400">
                  +{t.pts} 积分
                </span>
                <span className="mt-1 text-[11px] text-muted-foreground">{t.label}</span>
              </button>
            ))}
          </div>
        </Card>

        <Card className="flex flex-col justify-between p-5">
          <div>
            <div className="mb-2 flex items-center gap-2 text-sm font-semibold">
              <Ticket className="h-4 w-4 text-amber-500" />
              卡密 / 激活码兑换
            </div>
            <p className="text-xs text-muted-foreground">
              支持兑换赠送积分卡密或商品激活卡（赠送积分可用于软件下载与数字资源）。
            </p>
          </div>
          <div className="mt-3 flex gap-2">
            <Input
              placeholder="输入卡密，如 LAIN42-..."
              value={redeemCode}
              onChange={(e) => setRedeemCode(e.target.value)}
            />
            <Button disabled={busy === "redeem"} onClick={handleRedeem}>
              兑换
            </Button>
          </div>
        </Card>
      </div>

      {/* Category Filter */}
      <div className="flex items-center gap-2">
        {[
          { id: "all", label: "全部商品" },
          { id: "proxy", label: "代理专线（100GB / 220GB / 季年卡）" },
          { id: "software", label: "软件授权与数字资源" },
        ].map((t) => (
          <Button
            key={t.id}
            size="sm"
            variant={filter === t.id ? "default" : "outline"}
            onClick={() => setFilter(t.id as "all" | "proxy" | "software")}
          >
            {t.label}
          </Button>
        ))}
      </div>

      {/* Products Grid */}
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {filtered.map((p) => {
          const isProxy = p.code.startsWith("proxy_");
          return (
            <Card key={p.code} className="flex flex-col justify-between p-5">
              <div>
                <div className="flex items-start justify-between gap-2">
                  <h3 className="text-sm font-semibold leading-snug">{p.name}</h3>
                  {isProxy ? (
                    <Badge variant="emerald" className="shrink-0">
                      {p.traffic_gb ? `${p.traffic_gb}GB` : "专线"}
                    </Badge>
                  ) : (
                    <Badge variant="secondary" className="shrink-0">
                      <Gift className="mr-1 h-3 w-3" /> 可用赠送分
                    </Badge>
                  )}
                </div>
                <p className="mt-2 text-xs leading-relaxed text-muted-foreground">{p.desc}</p>
              </div>
              <div className="mt-4 space-y-3 border-t pt-3">
                <div className="flex items-baseline justify-between">
                  <div className="text-lg font-bold">
                    {p.points_price} <span className="text-xs font-normal text-muted-foreground">积分</span>
                  </div>
                  {p.price_cny != null && (
                    <span className="text-xs text-muted-foreground">约 ¥{p.price_cny}</span>
                  )}
                </div>
                {isProxy && (
                  <div className="flex items-center gap-1 text-[11px] text-amber-600 dark:text-amber-400">
                    <ShieldAlert className="h-3.5 w-3.5 shrink-0" />
                    仅限充值付费积分购买（不支持免费赠送积分）
                  </div>
                )}
                <Button
                  className="w-full"
                  variant={isProxy ? "emerald" : "default"}
                  disabled={busy === p.code}
                  onClick={() => handlePurchase(p)}
                >
                  <ShoppingBag className="h-4 w-4" />
                  {busy === p.code ? "处理中..." : "立即使用积分购买"}
                </Button>
              </div>
            </Card>
          );
        })}
      </div>
    </div>
  );
}
