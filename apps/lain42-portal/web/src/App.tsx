import { Suspense, lazy, useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowLeftRight,
  BarChart3,
  Cpu,
  Download,
  Globe,
  GraduationCap,
  KeyRound,
  LayoutDashboard,
  Loader2,
  Menu,
  PanelLeftClose,
  PanelLeftOpen,
  ShoppingBag,
  Sparkles,
  User,
  Wallet,
  Wrench,
  X,
} from "lucide-react";
import { apiFetch, getToken, setToken, type MeResponse } from "./lib/api";
import { useTheme } from "./lib/theme";
import { Badge, Button } from "./components/ui/primitives";

const HomePage = lazy(() =>
  import("./pages/HomePage").then((m) => ({ default: m.HomePage }))
);
const VpnPage = lazy(() =>
  import("./pages/VpnPage").then((m) => ({ default: m.VpnPage }))
);
const ShopPage = lazy(() =>
  import("./pages/ShopPage").then((m) => ({ default: m.ShopPage }))
);
const DownloadsPage = lazy(() =>
  import("./pages/DownloadsPage").then((m) => ({ default: m.DownloadsPage }))
);
const ModelsPage = lazy(() =>
  import("./pages/ModelsPage").then((m) => ({ default: m.ModelsPage }))
);
const AccountPage = lazy(() =>
  import("./pages/AccountPage").then((m) => ({ default: m.AccountPage }))
);
const ToolsPage = lazy(() =>
  import("./pages/ToolsPage").then((m) => ({ default: m.ToolsPage }))
);
const AccountPoolPage = lazy(() =>
  import("./pages/AccountPoolPage").then((m) => ({ default: m.AccountPoolPage }))
);
const MyAgentPage = lazy(() =>
  import("./pages/MyAgentPage").then((m) => ({ default: m.MyAgentPage }))
);
const ApiKeysPage = lazy(() =>
  import("./pages/ApiKeysPage").then((m) => ({ default: m.ApiKeysPage }))
);
const OperationsPage = lazy(() =>
  import("./pages/OperationsPage").then((m) => ({ default: m.OperationsPage }))
);
const CampusServicePage = lazy(() =>
  import("./pages/CampusServicePage").then((m) => ({
    default: m.CampusServicePage,
  }))
);

interface NavItem {
  id: string;
  path: string;
  label: string;
  badge?: string;
  icon: React.ReactNode;
}

const NAV_ITEMS: NavItem[] = [
  { id: "home", path: "/", label: "总览面板", icon: <LayoutDashboard className="h-4 w-4" /> },
  { id: "vpn", path: "/vpn", label: "极速专线", icon: <Globe className="h-4 w-4" /> },
  { id: "shop", path: "/shop", label: "积分商城", icon: <ShoppingBag className="h-4 w-4" /> },
  { id: "downloads", path: "/downloads", label: "免费下载库", badge: "1000+", icon: <Download className="h-4 w-4" /> },
  { id: "models", path: "/models", label: "AI 模型中转", icon: <Cpu className="h-4 w-4" /> },
  { id: "account-pool", path: "/account-pool", label: "我的账号池", icon: <Wallet className="h-4 w-4" /> },
  { id: "my-agent", path: "/my-agent", label: "我的 Agent", icon: <Sparkles className="h-4 w-4" /> },
  { id: "api-keys", path: "/api-keys", label: "API Key", icon: <KeyRound className="h-4 w-4" /> },
  { id: "account", path: "/account", label: "账户与工单", icon: <User className="h-4 w-4" /> },
  { id: "tool", path: "/tool", label: "开发者工具", icon: <Wrench className="h-4 w-4" /> },
  { id: "campus", path: "/campus", label: "校园服务", icon: <GraduationCap className="h-4 w-4" /> },
  { id: "admin", path: "/admin", label: "运营管理", icon: <BarChart3 className="h-4 w-4" /> },
];

const PUBLIC_BASE = (import.meta.env.VITE_PUBLIC_BASE || "").replace(/\/$/, "");

function withBase(path: string): string {
  if (!PUBLIC_BASE) return path;
  if (path === PUBLIC_BASE || path.startsWith(`${PUBLIC_BASE}/`)) return path;
  return `${PUBLIC_BASE}${path}`;
}

function pathToTab(pathname: string): string {
  const raw = PUBLIC_BASE && pathname.startsWith(PUBLIC_BASE)
    ? pathname.slice(PUBLIC_BASE.length) || "/"
    : pathname;
  const clean = raw.replace(/\/+$/, "") || "/";
  const found = NAV_ITEMS.find((item) => item.path === clean);
  if (found) return found.id;
  if (clean === "/campus-dock") return "campus";
  if (clean.startsWith("/downloads")) return "downloads";
  return "home";
}

export function App() {
  const [tab, setTab] = useState<string>(() => pathToTab(window.location.pathname));
  const [collapsed, setCollapsed] = useState(false);
  const [mobileOpen, setMobileOpen] = useState(false);
  const theme = useTheme();
  const [me, setMe] = useState<MeResponse | null>(null);
  const lastTracked = useRef<string | undefined>(undefined);
  useEffect(() => {
    if (lastTracked.current === tab) return;
    lastTracked.current = tab;
    if (tab === "admin") return;
    void apiFetch("/api/analytics/visit", {
      method: "POST", body: JSON.stringify({ page: tab }), keepalive: true,
    }).catch(() => {});
  }, [tab]);

  const refreshMe = useCallback(() => {
    if (!getToken()) {
      setMe(null);
      return;
    }
    apiFetch<MeResponse>("/studio/api/me")
      .then((res) => setMe(res))
      .catch(() => {
        setToken("");
        setMe(null);
      });
  }, []);

  useEffect(() => {
    refreshMe();
  }, [refreshMe]);

  useEffect(() => {
    const onPop = () => setTab(pathToTab(window.location.pathname));
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  function navigate(nextTab: string) {
    const item = NAV_ITEMS.find((n) => n.id === nextTab) || NAV_ITEMS[0];
    setTab(item.id);
    setMobileOpen(false);
    const nextPath = withBase(item.path);
    if (window.location.pathname !== nextPath) {
      window.history.pushState({}, "", nextPath);
    }
    window.scrollTo({ top: 0, behavior: "smooth" });
  }

  const activeNav = NAV_ITEMS.find((n) => n.id === tab) || NAV_ITEMS[0];

  const SidebarContent = ({ isMobile = false }: { isMobile?: boolean }) => (
    <div className="flex h-full flex-col">
      {/* Brand */}
      <div className="flex h-14 items-center justify-between border-b px-4">
        <button
          type="button"
          onClick={() => navigate("home")}
          className="flex items-center gap-2.5 text-left cursor-pointer"
        >
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-emerald-600 text-white font-bold shadow-xs">
            L
          </div>
          {(!collapsed || isMobile) && (
            <div className="leading-tight">
              <div className="font-bold text-sm tracking-tight flex items-center gap-1.5">
                Lain42
                <Badge variant="emerald" className="px-1.5 py-0 text-[10px]">
                  v2
                </Badge>
              </div>
              <div className="text-[11px] text-muted-foreground">
                极速专线 · AI 中转
              </div>
            </div>
          )}
        </button>
        {isMobile && (
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setMobileOpen(false)}
          >
            <X className="h-4 w-4" />
          </Button>
        )}
      </div>

      {/* Nav Items */}
      <nav className="flex-1 space-y-1 p-2.5 overflow-y-auto">
        {NAV_ITEMS.filter(
          (item) => item.id !== "admin" || me?.is_admin
        ).map((item) => {
          const active = tab === item.id;
          return (
            <button
              key={item.id}
              type="button"
              onClick={() => navigate(item.id)}
              title={collapsed && !isMobile ? item.label : undefined}
              className={`flex w-full items-center justify-between rounded-lg px-3 py-2 text-sm font-medium transition-colors cursor-pointer ${
                active
                  ? "bg-emerald-500/12 text-emerald-600 dark:text-emerald-400"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground"
              }`}
            >
              <span className="flex items-center gap-2.5 truncate">
                {item.icon}
                {(!collapsed || isMobile) && <span>{item.label}</span>}
              </span>
              {(!collapsed || isMobile) && item.badge && (
                <span className="rounded-full bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-mono text-emerald-600 dark:text-emerald-400">
                  {item.badge}
                </span>
              )}
            </button>
          );
        })}
      </nav>

      {/* Footer Classic Switch */}
      <div className="border-t p-3 space-y-2">
        <a
          href="https://lain42.top/"
          className="flex items-center justify-center gap-2 rounded-lg border bg-muted/40 px-3 py-2 text-xs text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
        >
          <ArrowLeftRight className="h-3.5 w-3.5 shrink-0" />
          {(!collapsed || isMobile) && <span>切换回经典版 lain42.top</span>}
        </a>
      </div>
    </div>
  );

  return (
    <div className="min-h-screen bg-background text-foreground flex">
      {/* Desktop Sidebar */}
      <aside
        className={`hidden md:flex flex-col border-r bg-card shrink-0 transition-all duration-200 sticky top-0 h-screen ${
          collapsed ? "w-16" : "w-60"
        }`}
      >
        <SidebarContent />
      </aside>

      {/* Mobile Drawer */}
      {mobileOpen && (
        <div className="fixed inset-0 z-50 md:hidden flex">
          <div
            className="fixed inset-0 bg-black/50 backdrop-blur-xs"
            onClick={() => setMobileOpen(false)}
          />
          <aside className="relative z-10 w-64 max-w-[80vw] bg-card border-r h-full shadow-xl">
            <SidebarContent isMobile />
          </aside>
        </div>
      )}

      {/* Main Column */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Top Header */}
        <header className="sticky top-0 z-30 flex h-14 items-center justify-between border-b bg-background/85 backdrop-blur-md px-4 lg:px-6">
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="icon"
              className="md:hidden"
              onClick={() => setMobileOpen(true)}
            >
              <Menu className="h-5 w-5" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="hidden md:inline-flex"
              onClick={() => setCollapsed((c) => !c)}
              title={collapsed ? "展开侧边栏" : "收起侧边栏"}
            >
              {collapsed ? (
                <PanelLeftOpen className="h-4 w-4" />
              ) : (
                <PanelLeftClose className="h-4 w-4" />
              )}
            </Button>
            <div className="flex items-center gap-2 text-sm font-medium">
              <span className="text-muted-foreground hidden sm:inline">
                Lain42 控制台
              </span>
              <span className="text-muted-foreground hidden sm:inline">/</span>
              <span>{activeNav.label}</span>
            </div>
          </div>

          <div className="flex items-center gap-2">
            {me ? (
              <button
                type="button"
                onClick={() => navigate("account")}
                className="flex items-center gap-2 rounded-full border bg-card px-3 py-1 text-xs hover:bg-accent transition-colors cursor-pointer"
              >
                <Wallet className="h-3.5 w-3.5 text-emerald-500" />
                <span className="font-mono font-semibold text-emerald-600 dark:text-emerald-400">
                  {me.points.paid_balance}
                </span>
                <span className="text-muted-foreground hidden sm:inline">
                  付费 / {me.points.bonus_balance} 赠
                </span>
                <span className="border-l pl-2 font-medium">{me.username}</span>
              </button>
            ) : (
              <Button
                size="sm"
                variant="emerald"
                onClick={() => navigate("account")}
              >
                <Sparkles className="h-3.5 w-3.5" />
                登录 / 注册
              </Button>
            )}

            <select
              aria-label="主题"
              className="h-11 max-w-full shrink-0 rounded-md border border-border bg-background px-2 text-base text-foreground focus-visible:ring-2 focus-visible:ring-ring sm:text-sm"
              value={theme.mode}
              onChange={(event) => theme.select(event.target.value)}
            >
              <option value="system">跟随系统</option>
              <option value="light">浅色</option>
              <option value="dark">深色</option>
            </select>
          </div>
        </header>

        {/* Main Content */}
        <main className="flex-1 p-4 lg:p-6 max-w-6xl w-full mx-auto">
          <Suspense
            fallback={
              <div className="flex items-center justify-center gap-2 py-20 text-sm text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin text-emerald-500" />
                <span>正在加载模块...</span>
              </div>
            }
          >
            {tab === "home" && <HomePage onNavigate={navigate} />}
            {tab === "vpn" && (
              <VpnPage
                me={me}
                onRefreshMe={refreshMe}
                onNavigate={navigate}
              />
            )}
            {tab === "shop" && (
              <ShopPage
                me={me}
                onRefreshMe={refreshMe}
                onNeedAuth={() => navigate("account")}
              />
            )}
            {tab === "downloads" && <DownloadsPage />}
            {tab === "models" && <ModelsPage />}
            {tab === "account-pool" && (
              <AccountPoolPage me={me} onNeedAuth={() => navigate("account")} />
            )}
            {tab === "my-agent" && (
              <MyAgentPage
                me={me}
                onNeedAuth={() => navigate("account")}
                onOpenApiKeys={() => navigate("api-keys")}
              />
            )}
            {tab === "api-keys" && (
              <ApiKeysPage me={me} onNeedAuth={() => navigate("account")} />
            )}
            {tab === "account" && (
              <AccountPage me={me} onRefreshMe={refreshMe} />
            )}
            {tab === "tool" && (
              <ToolsPage
                onOpenOperations={
                  me?.is_admin ? () => navigate("admin") : undefined
                }
              />
            )}
            {tab === "campus" && <CampusServicePage />}
            {tab === "admin" && (
              <OperationsPage me={me} onNeedAuth={() => navigate("account")} />
            )}
          </Suspense>
        </main>

        {/* Footer */}
        <footer className="border-t py-4 px-6 text-xs text-muted-foreground flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <div>
            © {new Date().getFullYear()} Lain42 Cloud · {(import.meta.env.VITE_BRAND_HOST || (import.meta.env.VITE_PUBLIC_BASE ? "lain42.top" : "dev.lain42.top"))} 新一代商业化控制台
          </div>
          <div className="flex items-center gap-4">
            <a
              href="https://sh.lain42.top/"
              target="_blank"
              rel="noreferrer"
              className="hover:text-foreground transition-colors"
            >
              节点一键部署镜像 (sh.lain42.top)
            </a>
            <a
              href="https://lain42.top/"
              className="hover:text-foreground transition-colors"
            >
              经典版 lain42.top
            </a>
          </div>
        </footer>
      </div>
    </div>
  );
}
