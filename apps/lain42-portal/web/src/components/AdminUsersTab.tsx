import { useCallback, useEffect, useState } from "react";
import {
  CreditCard,
  KeyRound,
  Mail,
  PlusCircle,
  Search,
  Users,
} from "lucide-react";
import { apiFetch } from "../lib/api";
import { Badge, Button, Card, Input } from "./ui/primitives";

interface AdminUserRow {
  id: number;
  username: string;
  email: string;
  paid_balance: number;
  bonus_balance: number;
  created_unix: number;
  active_entitlements: number;
}

const SUB_PRESETS = [
  { code: "proxy_pro_1m", label: "黑金卡 · 商业独享家宽 Pro 月卡 (1000GB / 30天)", days: 30, gb: 1000 },
  { code: "proxy_1m_220g", label: "尊享金卡 · 标准极速月卡 (220GB / 30天)", days: 30, gb: 220 },
  { code: "proxy_3m", label: "尊享金卡 · 极速专线季卡 (660GB / 90天)", days: 90, gb: 220 },
  { code: "proxy_1y", label: "尊享金卡 · 极速专线年卡 (2640GB / 365天)", days: 365, gb: 220 },
  { code: "proxy_3y", label: "尊享金卡 · 极速专线三年卡 (7920GB / 1095天)", days: 1095, gb: 220 },
  { code: "proxy_1m", label: "极光银卡 · 轻量极速月卡 (100GB / 30天)", days: 30, gb: 100 },
  { code: "proxy_6m", label: "云雾白卡 · 基础对话月卡 (40GB / 30天)", days: 30, gb: 40 },
  { code: "proxy_1m_1g", label: "云雾白卡 · 公益体验月卡 (10GB / 30天)", days: 30, gb: 10 },
];

export function AdminUsersTab() {
  const [users, setUsers] = useState<AdminUserRow[]>([]);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<AdminUserRow | null>(null);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  // Action form states
  const [newEmail, setNewEmail] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [paidDelta, setPaidDelta] = useState("100");
  const [bonusDelta, setBonusDelta] = useState("0");
  const [grantProduct, setGrantProduct] = useState("proxy_1m_220g");
  const [grantDays, setGrantDays] = useState("30");
  const [grantGb, setGrantGb] = useState("220");
  const [grantSubUrl, setGrantSubUrl] = useState("");

  const loadUsers = useCallback((q = "") => {
    apiFetch<{ users: AdminUserRow[] }>(
      `/api/admin/users?q=${encodeURIComponent(q)}`
    )
      .then((res) => setUsers(res.users || []))
      .catch((e: Error) => setMsg({ ok: false, text: e.message }));
  }, []);

  useEffect(() => {
    loadUsers("");
  }, [loadUsers]);

  function pickUser(u: AdminUserRow) {
    setSelected(u);
    setNewEmail(u.email || "");
    setNewPassword("");
    setMsg(null);
  }

  async function runAction(label: string, path: string, body: Record<string, unknown>) {
    setBusy(true);
    setMsg(null);
    try {
      const res = await apiFetch<Record<string, unknown>>(path, {
        method: "POST",
        body: JSON.stringify(body),
      });
      setMsg({
        ok: true,
        text: `${label}成功！${res.license_key ? ` 卡号：${String(res.license_key)}` : ""}`,
      });
      loadUsers(query);
    } catch (e) {
      setMsg({ ok: false, text: e instanceof Error ? e.message : "操作失败" });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="grid gap-5 lg:grid-cols-5">
      {/* Left: User Search & Table */}
      <Card className="p-4 space-y-3 lg:col-span-3">
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <h2 className="text-sm font-semibold flex items-center gap-2">
            <Users className="h-4 w-4 text-emerald-500" />
            全站用户列表（共 {users.length} 条）
          </h2>
          <div className="flex gap-2">
            <div className="relative">
              <Search className="h-3.5 w-3.5 text-muted-foreground absolute left-2.5 top-2.5" />
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && loadUsers(query)}
                placeholder="搜用户名或邮箱..."
                className="pl-8 h-8 text-xs w-48"
              />
            </div>
            <Button size="sm" variant="outline" onClick={() => loadUsers(query)}>
              搜索
            </Button>
          </div>
        </div>

        <div className="overflow-x-auto max-h-[560px] overflow-y-auto">
          <table className="w-full text-xs">
            <thead className="border-b text-muted-foreground sticky top-0 bg-card">
              <tr>
                <th className="py-2 text-left font-medium">用户名</th>
                <th className="py-2 text-left font-medium">绑定邮箱</th>
                <th className="py-2 text-right font-medium">付费/赠分</th>
                <th className="py-2 text-center font-medium">有效卡</th>
                <th className="py-2 text-right font-medium">操作</th>
              </tr>
            </thead>
            <tbody className="divide-y">
              {users.map((u) => {
                const isSel = selected?.username === u.username;
                return (
                  <tr
                    key={u.id}
                    className={`transition-colors ${
                      isSel ? "bg-emerald-500/10" : "hover:bg-muted/30"
                    }`}
                  >
                    <td className="py-2.5 font-mono font-semibold">{u.username}</td>
                    <td className="py-2.5 text-muted-foreground truncate max-w-[160px]">
                      {u.email || "未绑定"}
                    </td>
                    <td className="py-2.5 text-right font-mono">
                      <span className="text-emerald-600 dark:text-emerald-400">
                        {u.paid_balance}
                      </span>
                      <span className="text-muted-foreground"> / {u.bonus_balance}</span>
                    </td>
                    <td className="py-2.5 text-center">
                      <Badge variant={u.active_entitlements > 0 ? "emerald" : "outline"}>
                        {u.active_entitlements}
                      </Badge>
                    </td>
                    <td className="py-2.5 text-right">
                      <Button
                        size="sm"
                        variant={isSel ? "emerald" : "outline"}
                        className="h-7 px-2.5 text-xs"
                        onClick={() => pickUser(u)}
                      >
                        管理
                      </Button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </Card>

      {/* Right: Selected User Operations */}
      <Card className="p-4 space-y-4 lg:col-span-2">
        {!selected ? (
          <div className="py-16 text-center text-xs text-muted-foreground">
            请在左侧点击任意用户的「管理」按钮，即可修改邮箱、重置密码、补发积分或手动签发黑/金/银/白订阅卡。
          </div>
        ) : (
          <>
            <div className="border-b pb-3 flex items-center justify-between">
              <div>
                <div className="text-sm font-bold font-mono">{selected.username}</div>
                <div className="text-[11px] text-muted-foreground">
                  ID #{selected.id} · 付费 {selected.paid_balance} 分 / 赠送{" "}
                  {selected.bonus_balance} 分
                </div>
              </div>
              <Badge variant="emerald">管理中</Badge>
            </div>

            {msg && (
              <div
                className={`rounded-md border px-3 py-2 text-xs ${
                  msg.ok
                    ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                    : "border-destructive/30 bg-destructive/10 text-destructive"
                }`}
              >
                {msg.text}
              </div>
            )}

            {/* 1. Bind / Update Email */}
            <div className="space-y-1.5">
              <label className="text-xs font-semibold flex items-center gap-1.5">
                <Mail className="h-3.5 w-3.5 text-emerald-500" />
                绑定 / 修改邮箱
              </label>
              <div className="flex gap-2">
                <Input
                  value={newEmail}
                  onChange={(e) => setNewEmail(e.target.value)}
                  placeholder="user@example.com"
                  className="h-8 text-xs"
                />
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  onClick={() =>
                    runAction("修改邮箱", "/api/admin/users/email", {
                      username: selected.username,
                      email: newEmail,
                    })
                  }
                >
                  保存
                </Button>
              </div>
            </div>

            {/* 2. Reset Password */}
            <div className="space-y-1.5">
              <label className="text-xs font-semibold flex items-center gap-1.5">
                <KeyRound className="h-3.5 w-3.5 text-amber-500" />
                重置登录密码（至少 8 位）
              </label>
              <div className="flex gap-2">
                <Input
                  type="text"
                  value={newPassword}
                  onChange={(e) => setNewPassword(e.target.value)}
                  placeholder="输入新密码（至少 8 位）"
                  className="h-8 text-xs font-mono"
                />
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy || newPassword.length < 8}
                  onClick={() =>
                    runAction("重置密码", "/api/admin/users/reset-password", {
                      username: selected.username,
                      new_password: newPassword,
                    })
                  }
                >
                  重置
                </Button>
              </div>
            </div>

            {/* 3. Manual Points Adjustment */}
            <div className="space-y-1.5 border-t pt-3">
              <label className="text-xs font-semibold flex items-center gap-1.5">
                <PlusCircle className="h-3.5 w-3.5 text-emerald-500" />
                手动补发 / 扣减积分（可填负数）
              </label>
              <div className="grid grid-cols-2 gap-2">
                <div>
                  <span className="text-[10px] text-muted-foreground">付费积分变动</span>
                  <Input
                    type="number"
                    value={paidDelta}
                    onChange={(e) => setPaidDelta(e.target.value)}
                    className="h-8 text-xs font-mono"
                  />
                </div>
                <div>
                  <span className="text-[10px] text-muted-foreground">赠送积分变动</span>
                  <Input
                    type="number"
                    value={bonusDelta}
                    onChange={(e) => setBonusDelta(e.target.value)}
                    className="h-8 text-xs font-mono"
                  />
                </div>
              </div>
              <Button
                size="sm"
                variant="emerald"
                className="w-full"
                disabled={busy}
                onClick={() =>
                  runAction("调整积分", "/api/admin/users/adjust-points", {
                    username: selected.username,
                    paid_delta: Number(paidDelta) || 0,
                    bonus_delta: Number(bonusDelta) || 0,
                  })
                }
              >
                执行积分补发 / 调整
              </Button>
            </div>

            {/* 4. Manual Subscription Card Issuance */}
            <div className="space-y-2 border-t pt-3">
              <label className="text-xs font-semibold flex items-center gap-1.5">
                <CreditCard className="h-3.5 w-3.5 text-amber-500" />
                手动补发会员订阅卡（黑/金/银/白卡）
              </label>
              <select
                value={grantProduct}
                onChange={(e) => {
                  const code = e.target.value;
                  setGrantProduct(code);
                  const found = SUB_PRESETS.find((x) => x.code === code);
                  if (found) {
                    setGrantDays(String(found.days));
                    setGrantGb(String(found.gb));
                  }
                }}
                className="h-8 w-full rounded-md border border-input bg-background px-2 text-xs"
              >
                {SUB_PRESETS.map((p) => (
                  <option key={p.code} value={p.code}>
                    {p.label}
                  </option>
                ))}
              </select>
              <div className="grid grid-cols-2 gap-2">
                <div>
                  <span className="text-[10px] text-muted-foreground">有效天数</span>
                  <Input
                    type="number"
                    value={grantDays}
                    onChange={(e) => setGrantDays(e.target.value)}
                    className="h-8 text-xs font-mono"
                  />
                </div>
                <div>
                  <span className="text-[10px] text-muted-foreground">月流量 (GB)</span>
                  <Input
                    type="number"
                    value={grantGb}
                    onChange={(e) => setGrantGb(e.target.value)}
                    className="h-8 text-xs font-mono"
                  />
                </div>
              </div>
              <Input
                value={grantSubUrl}
                onChange={(e) => setGrantSubUrl(e.target.value)}
                placeholder="可选：绑定订阅链接 https://lain42.top/panel/api/sub/..."
                className="h-8 text-xs font-mono"
              />
              <Button
                size="sm"
                variant="emerald"
                className="w-full"
                disabled={busy}
                onClick={() =>
                  runAction("补发订阅卡", "/api/admin/users/grant-subscription", {
                    username: selected.username,
                    product: grantProduct,
                    duration_days: Number(grantDays) || 30,
                    traffic_gb: Number(grantGb) || 220,
                    sub_url: grantSubUrl,
                  })
                }
              >
                立即签发金属会员卡
              </Button>
            </div>
          </>
        )}
      </Card>
    </div>
  );
}
