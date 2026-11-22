import { useCallback, useEffect, useState } from "react";
import {
  ArrowLeftRight,
  BarChart3,
  Cpu,
  Download,
  Globe,
  LayoutDashboard,
  Menu,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  ShoppingBag,
  Sparkles,
  Sun,
  User,
  Wallet,
  Wrench,
  X,
} from "lucide-react";
import { apiFetch, getToken, setToken, type MeResponse } from "./lib/api";
import { Badge, Button } from "./components/ui/primitives";
import { HomePage } from "./pages/HomePage";
import { VpnPage } from "./pages/VpnPage";
import { ShopPage } from "./pages/ShopPage";
import { DownloadsPage } from "./pages/DownloadsPage";
import { ModelsPage } from "./pages/ModelsPage";
import { AccountPage } from "./pages/AccountPage";
import { ToolsPage } from "./pages/ToolsPage";
import { AdminAnalyticsPage } from "./pages/AdminAnalyticsPage";

interface NavItem {
  id: string;
  path: string;
  label: string;
  badge?: string;
  icon: React.ReactNode;
}

const NAV_ITEMS: NavItem[] = [
  { id: "home", path: "/", label: "总览面板", icon: <LayoutDashboard className="h-4 w-4" /> },
  { id: "vpn", path: "/vpn", label: "极速专线", badge: "12.7MB/s", icon: <Globe className="h-4 w-4" /> },
  { id: "shop", path: "/shop", label: "积分商城", icon: <ShoppingBag className="h-4 w-4" /> },
  { id: "downloads", path: "/downloads", label: "免费下载库", badge: "1000+", icon: <Download className="h-4 w-4" /> },
  { id: "models", path: "/models", label: "AI 模型中转", icon: <Cpu className="h-4 w-4" /> },
  { id: "account", path: "/account", label: "账户与工单", icon: <User className="h-4 w-4" /> },
  { id: "tool", path: "/tool", label: "开发者工具", icon: <Wrench className="h-4 w-4" /> },
  { id: "admin", path: "/admin", label: "数据看板", badge: "Admin", icon: <BarChart3 className="h-4 w-4" /> },
];

function pathToTab(pathname: string): string {
  const clean = pathname.replace(/\/+$/, "") || "/";
  const found = NAV_ITEMS.find((item) => item.path === clean);
  if (found) return found.id;
  if (clean.startsWith("/downloads")) return "downloads";
  return "home";
}

export function App() {
  const [tab, setTab] = useState<string>(() => pathToTab(window.location.pathname));
  const [collapsed, setCollapsed] = useState(false);
  const [mobileOpen, setMobileOpen] = useState(false);
  const [dark, setDark] = useState<boolean>(() => {
    const saved = localStorage.getItem("lain42_theme");
    if (saved) return saved === "dark";
    return true;
  });
  const [me, setMe] = useState<MeResponse | null>(null);

  useEffect(() => {
    const root = document.documentElement;
    if (dark) root.classList.add("dark");
    else root.classList.remove("dark");
    localStorage.setItem("lain42_theme", dark ? "dark" : "light");
  }, [dark]);

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
    if (window.location.pathname !== item.path) {
      window.history.pushState({}, "", item.path);
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
          (item) =>
            item.id !== "admin" ||
            (me &&
              (me.username.toLowerCase() === "lilyco42ovo" ||
                me.username.toLowerCase() === "admin"))
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

            <Button
              variant="ghost"
              size="icon"
              onClick={() => setDark((d) => !d)}
              title={dark ? "切换浅色模式" : "切换深色模式"}
            >
              {dark ? (
                <Sun className="h-4 w-4 text-amber-400" />
              ) : (
                <Moon className="h-4 w-4" />
              )}
            </Button>
          </div>
        </header>

        {/* Main Content */}
        <main className="flex-1 p-4 lg:p-6 max-w-6xl w-full mx-auto">
          {tab === "home" && <HomePage onNavigate={navigate} />}
          {tab === "vpn" && <VpnPage onNavigate={navigate} />}
          {tab === "shop" && (
            <ShopPage
              me={me}
              onRefreshMe={refreshMe}
              onNeedAuth={() => navigate("account")}
            />
          )}
          {tab === "downloads" && <DownloadsPage />}
          {tab === "models" && <ModelsPage />}
          {tab === "account" && (
            <AccountPage me={me} onRefreshMe={refreshMe} />
          )}
          {tab === "tool" && <ToolsPage />}
          {tab === "admin" && (
            <AdminAnalyticsPage me={me} onNeedAuth={() => navigate("account")} />
          )}
        </main>

        {/* Footer */}
        <footer className="border-t py-4 px-6 text-xs text-muted-foreground flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <div>
            © {new Date().getFullYear()} Lain42 Cloud · dev.lain42.top 新一代商业化控制台
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
