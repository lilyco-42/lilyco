import { useEffect, useState } from "react";
import {
  Check,
  Copy,
  KeyRound,
  LogOut,
  MessageSquare,
  Package,
  Receipt,
  Send,
  ShieldCheck,
  User as UserIcon,
  Wallet,
} from "lucide-react";
import {
  apiFetch,
  setToken,
  type MeResponse,
  type OrderRecord,
} from "../lib/api";
import { Badge, Button, Card, Input } from "../components/ui/primitives";
import {
  inferCardTier,
  MembershipCard,
  productDisplayTitle,
} from "../components/MembershipCard";

interface AccountPageProps {
  me: MeResponse | null;
  onRefreshMe: () => void;
}

export function AccountPage({ me, onRefreshMe }: AccountPageProps) {
  const [authMode, setAuthMode] = useState<"login" | "register">("login");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [email, setEmail] = useState("");
  const [authMsg, setAuthMsg] = useState<{ ok: boolean; msg: string } | null>(null);
  const [authBusy, setAuthBusy] = useState(false);

  const [orders, setOrders] = useState<OrderRecord[]>([]);
  const [copiedKey, setCopiedKey] = useState("");
  const [subFormats, setSubFormats] = useState<Record<number, string>>({});

  // Support ticket state
  const [contactName, setContactName] = useState("");
  const [contactInfo, setContactInfo] = useState("");
  const [contactSubject, setContactSubject] = useState("");
  const [contactBody, setContactBody] = useState("");
  const [contactBusy, setContactBusy] = useState(false);
  const [contactMsg, setContactMsg] = useState<{ ok: boolean; msg: string } | null>(
    null
  );

  useEffect(() => {
    if (!me) {
      setOrders([]);
      return;
    }
    setContactName((prev) => prev || me.username);
    setContactInfo((prev) => prev || me.email);
    apiFetch<{ orders?: OrderRecord[] }>("/studio/api/account/orders")
      .then((res) => setOrders(res.orders || []))
      .catch(() => {});
  }, [me]);

  async function handleAuth(e: React.FormEvent) {
    e.preventDefault();
    setAuthBusy(true);
    setAuthMsg(null);
    try {
      const path =
        authMode === "login"
          ? "/studio/api/login"
          : "/studio/api/register";
      const payload =
        authMode === "login"
          ? { username: username.trim(), password }
          : { username: username.trim(), password, email: email.trim() };
      const res = await apiFetch<{ token?: string; detail?: string }>(path, {
        method: "POST",
        body: JSON.stringify(payload),
      });
      if (res.token) {
        setToken(res.token);
        onRefreshMe();
        setAuthMsg({
          ok: true,
          msg: authMode === "login" ? "登录成功！" : "注册并登录成功！",
        });
      }
    } catch (err) {
      setAuthMsg({
        ok: false,
        msg: err instanceof Error ? err.message : "操作失败",
      });
    } finally {
      setAuthBusy(false);
    }
  }

  function handleLogout() {
    setToken("");
    onRefreshMe();
  }

  function copyValue(id: string, val: string) {
    navigator.clipboard.writeText(val).then(() => {
      setCopiedKey(id);
      setTimeout(() => setCopiedKey(""), 1800);
    });
  }

  async function handleContact(e: React.FormEvent) {
    e.preventDefault();
    if (!contactBody.trim()) {
      setContactMsg({ ok: false, msg: "请填写具体问题或需求内容" });
      return;
    }
    setContactBusy(true);
    setContactMsg(null);
    try {
      const res = await apiFetch<{ ok: boolean; ticket_id: number }>(
        "/studio/api/contact",
        {
          method: "POST",
          body: JSON.stringify({
            kind: contactSubject.trim() || "售后支持",
            contact: contactInfo.trim() || contactName.trim() || "未填写",
            order_no: contactName.trim(),
            body: contactBody.trim(),
          }),
        }
      );
      setContactMsg({
        ok: true,
        msg: `工单已提交成功（编号 #${res.ticket_id}），我们会尽快联系您。`,
      });
      setContactSubject("");
      setContactBody("");
    } catch (err) {
      setContactMsg({
        ok: false,
        msg: err instanceof Error ? err.message : "工单提交失败",
      });
    } finally {
      setContactBusy(false);
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight flex items-center gap-2">
            <UserIcon className="h-6 w-6 text-emerald-500" />
            账户中心与工单支持
          </h1>
          <p className="text-sm text-muted-foreground mt-1">
            管理您的双钱包积分、订阅链接、历史订单，或直接提交工单联系团队
          </p>
        </div>
        {me && (
          <Button variant="outline" size="sm" onClick={handleLogout}>
            <LogOut className="h-4 w-4" />
            退出登录
          </Button>
        )}
      </div>

      {!me ? (
        <Card className="max-w-md p-6 space-y-5">
          <div className="flex gap-2 border-b pb-3">
            <Button
              size="sm"
              variant={authMode === "login" ? "default" : "ghost"}
              onClick={() => {
                setAuthMode("login");
                setAuthMsg(null);
              }}
            >
              账号登录
            </Button>
            <Button
              size="sm"
              variant={authMode === "register" ? "default" : "ghost"}
              onClick={() => {
                setAuthMode("register");
                setAuthMsg(null);
              }}
            >
              注册新账号
            </Button>
          </div>

          <form onSubmit={handleAuth} className="space-y-4">
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-muted-foreground">
                用户名
              </label>
              <Input
                required
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="输入用户名"
              />
            </div>

            {authMode === "register" && (
              <div className="space-y-1.5">
                <label className="text-xs font-medium text-muted-foreground">
                  邮箱（可选，用于找回密码与通知）
                </label>
                <Input
                  type="email"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  placeholder="you@example.com"
                />
              </div>
            )}

            <div className="space-y-1.5">
              <label className="text-xs font-medium text-muted-foreground">
                密码
              </label>
              <Input
                required
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="输入密码"
              />
            </div>

            {authMsg && (
              <p
                className={`text-xs ${
                  authMsg.ok ? "text-emerald-500" : "text-destructive"
                }`}
              >
                {authMsg.msg}
              </p>
            )}

            <Button
              type="submit"
              variant="emerald"
              className="w-full"
              disabled={authBusy}
            >
              <KeyRound className="h-4 w-4" />
              {authBusy
                ? "处理中..."
                : authMode === "login"
                ? "立即登录"
                : "注册并登录"}
            </Button>
          </form>
        </Card>
      ) : (
        <>
          {/* Dual Wallet Summary */}
          <div className="grid gap-4 sm:grid-cols-3">
            <Card className="p-4 space-y-1">
              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>当前账号</span>
                <ShieldCheck className="h-4 w-4 text-emerald-500" />
              </div>
              <div className="text-xl font-bold">{me.username}</div>
              <div className="text-xs text-muted-foreground">
                {me.email || "未绑定邮箱"}
              </div>
            </Card>

            <Card className="p-4 space-y-1 border-emerald-500/30">
              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>付费积分（可购代理/软件）</span>
                <Wallet className="h-4 w-4 text-emerald-500" />
              </div>
              <div className="text-2xl font-bold font-mono text-emerald-600 dark:text-emerald-400">
                {me.points.paid_balance}
              </div>
              <div className="text-xs text-muted-foreground">
                10 积分 = ¥1.00，支持专线月/季/年卡
              </div>
            </Card>

            <Card className="p-4 space-y-1">
              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>赠送积分（可购软件/工具）</span>
                <Badge variant="secondary">总计 {me.points.balance}</Badge>
              </div>
              <div className="text-2xl font-bold font-mono">
                {me.points.bonus_balance}
              </div>
              <div className="text-xs text-muted-foreground">
                活动与注册赠送，限非代理类软件使用
              </div>
            </Card>
          </div>

          {/* Active Entitlements / Subscriptions */}
          <Card className="p-5 space-y-4">
            <div className="flex items-center justify-between">
              <h2 className="text-base font-semibold flex items-center gap-2">
                <Package className="h-4 w-4 text-emerald-500" />
                已开通服务与订阅链接 ({me.entitlements.length})
              </h2>
            </div>

            {me.entitlements.length === 0 ? (
              <p className="text-sm text-muted-foreground py-4">
                暂无已开通的订阅或软件授权，前往「积分商城」即可自助兑换。
              </p>
            ) : (
              <div className="grid gap-4 lg:grid-cols-2">
                {me.entitlements.map((ent, idx) => {
                  const rawSub = ent.meta?.sub_url || "";
                  const subMatch = rawSub.match(/\/panel\/api\/sub\/([A-Za-z0-9_-]+)/);
                  const subToken = subMatch ? subMatch[1] : "";
                  const format = subFormats[idx] || "clash";
                  const resolvedSubUrl = subToken
                    ? `https://lain42.top/studio/api/sub/${encodeURIComponent(subToken)}/${format}`
                    : rawSub;
                  const displayValue = resolvedSubUrl || ent.license_key;
                  const isClashFormat = format.startsWith("clash");
                  const expiresRaw = ent.expires_at || ent.meta?.expires_at || "";
                  const expiresText = expiresRaw
                    ? new Date(expiresRaw).toLocaleDateString("zh-CN", {
                        year: "numeric",
                        month: "2-digit",
                        day: "2-digit",
                      })
                    : "PERMANENT";
                  const trafficGb = ent.meta?.traffic_gb;
                  const tier = inferCardTier(ent.product);
                  const cardTitle = productDisplayTitle(ent.product);

                  return (
                    <div
                      key={`${ent.product}-${idx}`}
                      className="rounded-2xl border p-3.5 space-y-3 bg-card shadow-xs"
                    >
                      <MembershipCard
                        tier={tier}
                        title={cardTitle}
                        holder={me.username.toUpperCase()}
                        cardNo={ent.license_key}
                        expiresText={expiresText}
                        quotaText={trafficGb ? `${trafficGb} GB/月` : undefined}
                        status={ent.status}
                      />

                      {subToken && ent.status === "active" && (
                        <select
                          value={format}
                          onChange={(e) =>
                            setSubFormats((prev) => ({
                              ...prev,
                              [idx]: e.target.value,
                            }))
                          }
                          className="h-8 w-full rounded-md border border-input bg-background px-2.5 text-xs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                        >
                          <option value="clash">
                            🖥️ Windows / Mac · Clash Verge Rev
                          </option>
                          <option value="base64">
                            📱 iPhone / Android · Karing / Base64 通用订阅
                          </option>
                          <option value="clashjp">
                            🇯🇵 日本节点 · Clash Verge
                          </option>
                          <option value="base64jp">
                            🇯🇵 日本节点 · Karing Base64
                          </option>
                          <option value="clashcommunity">
                            🌐 普通节点 · 已验证 · Clash
                          </option>
                          <option value="clashall">
                            🌐 普通节点 · 完整目录 · Clash
                          </option>
                        </select>
                      )}

                      <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
                        <input
                          readOnly
                          value={displayValue}
                          onFocus={(e) => e.currentTarget.select()}
                          className="flex-1 h-8 rounded-md border border-input bg-muted/40 px-2.5 font-mono text-[11px] text-foreground"
                        />
                        <div className="flex items-center gap-1.5 shrink-0">
                          <Button
                            size="sm"
                            variant="emerald"
                            onClick={() => copyValue(`ent-${idx}`, displayValue)}
                          >
                            {copiedKey === `ent-${idx}` ? (
                              <>
                                <Check className="h-3.5 w-3.5" />
                                已复制
                              </>
                            ) : (
                              <>
                                <Copy className="h-3.5 w-3.5" />
                                {subToken
                                  ? isClashFormat
                                    ? "复制 Clash"
                                    : "复制 Base64"
                                  : "复制卡号"}
                              </>
                            )}
                          </Button>
                          {subToken &&
                            ent.status === "active" &&
                            isClashFormat && (
                              <a
                                href={`clash://install-config?url=${encodeURIComponent(
                                  resolvedSubUrl
                                )}`}
                                className="inline-flex h-8 items-center justify-center rounded-md border border-input bg-background px-2.5 text-xs font-medium hover:bg-accent transition-colors"
                              >
                                一键导入
                              </a>
                            )}
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </Card>

          {/* Order History */}
          <Card className="p-5 space-y-4">
            <h2 className="text-base font-semibold flex items-center gap-2">
              <Receipt className="h-4 w-4 text-emerald-500" />
              历史订单记录 ({orders.length})
            </h2>
            {orders.length === 0 ? (
              <p className="text-sm text-muted-foreground">暂无历史订单记录。</p>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead className="border-b text-xs text-muted-foreground">
                    <tr>
                      <th className="py-2 text-left font-medium">订单号</th>
                      <th className="py-2 text-left font-medium">商品</th>
                      <th className="py-2 text-left font-medium">方式</th>
                      <th className="py-2 text-right font-medium">积分变动</th>
                      <th className="py-2 text-right font-medium">时间</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y">
                    {orders.map((o) => {
                      const delta = o.points_delta ?? o.points_amount ?? 0;
                      const ts =
                        typeof o.created_at === "number"
                          ? new Date(o.created_at * 1000).toLocaleString("zh-CN")
                          : o.created_at;
                      return (
                        <tr key={o.order_no}>
                          <td className="py-2.5 font-mono text-xs">{o.order_no}</td>
                          <td className="py-2.5 text-xs">{o.product}</td>
                          <td className="py-2.5">
                            <Badge variant="outline">
                              {o.payment_method || o.status || "积分"}
                            </Badge>
                          </td>
                          <td className="py-2.5 text-right font-mono text-xs">
                            {delta > 0 ? `+${delta}` : delta}
                          </td>
                          <td className="py-2.5 text-right text-xs text-muted-foreground">
                            {ts}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </Card>
        </>
      )}

      {/* Support / Contact Ticket Form (available to both guests and logged-in users) */}
      <Card className="p-5 space-y-4">
        <div>
          <h2 className="text-base font-semibold flex items-center gap-2">
            <MessageSquare className="h-4 w-4 text-emerald-500" />
            工单反馈 / 联系团队
          </h2>
          <p className="text-xs text-muted-foreground mt-1">
            节点异常、充值掉单、大客户定制或软件建议，均可直接提交工单，记录将持久化保存。
          </p>
        </div>

        <form onSubmit={handleContact} className="space-y-3">
          <div className="grid gap-3 sm:grid-cols-3">
            <Input
              value={contactName}
              onChange={(e) => setContactName(e.target.value)}
              placeholder="称呼 / 用户名"
            />
            <Input
              value={contactInfo}
              onChange={(e) => setContactInfo(e.target.value)}
              placeholder="联系方式（邮箱 / Telegram / 微信）"
            />
            <Input
              value={contactSubject}
              onChange={(e) => setContactSubject(e.target.value)}
              placeholder="主题（如：节点测速 / 充值核对）"
            />
          </div>
          <textarea
            rows={4}
            value={contactBody}
            onChange={(e) => setContactBody(e.target.value)}
            placeholder="请详细描述您遇到的问题、订单号或需求..."
            className="flex w-full rounded-md border border-input bg-background px-3 py-2 text-sm shadow-xs placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          />
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            {contactMsg ? (
              <span
                className={`text-xs ${
                  contactMsg.ok ? "text-emerald-500" : "text-destructive"
                }`}
              >
                {contactMsg.msg}
              </span>
            ) : (
              <span className="text-xs text-muted-foreground">
                支持游客直接留言，登录后自动预填账号信息
              </span>
            )}
            <Button type="submit" variant="emerald" disabled={contactBusy}>
              <Send className="h-4 w-4" />
              {contactBusy ? "提交中..." : "提交工单"}
            </Button>
          </div>
        </form>
      </Card>
    </div>
  );
}
