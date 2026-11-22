import { useCallback, useEffect, useState } from "react";
import {
  Activity,
  Check,
  Copy,
  Crown,
  RefreshCw,
  Smartphone,
  Terminal,
  Zap,
} from "lucide-react";
import { Badge, Button, Card } from "./ui/primitives";

export interface MirrorProbeItem {
  id: string;
  name: string;
  scope: string;
  probeUrl: string;
  probeBytes: number;
  baseUrl: string;
  ms: number | null; // null = pending, -1 = failed/timeout
  mbps: number | null; // real measured MB/s throughput
}

export interface GhProxyProbeItem {
  host: string;
  name: string;
  probeUrl: string;
  ms: number | null;
}

const NODE_INDEX_BYTES = 338361;

const INITIAL_MIRRORS: MirrorProbeItem[] = [
  { id: "tuna", name: "清华大学 TUNA", scope: "真高速 github-release + Node + F-Droid", probeUrl: "https://mirrors.tuna.tsinghua.edu.cn/nodejs-release/index.json", probeBytes: NODE_INDEX_BYTES, baseUrl: "https://mirrors.tuna.tsinghua.edu.cn", ms: null, mbps: null },
  { id: "nju", name: "南京大学 NJU", scope: "真高速 github-release(87组织) + Node", probeUrl: "https://mirrors.nju.edu.cn/nodejs-release/index.json", probeBytes: NODE_INDEX_BYTES, baseUrl: "https://mirrors.nju.edu.cn", ms: null, mbps: null },
  { id: "bfsu", name: "北外 BFSU", scope: "真高速 github-release + Node + F-Droid", probeUrl: "https://mirrors.bfsu.edu.cn/nodejs-release/index.json", probeBytes: NODE_INDEX_BYTES, baseUrl: "https://mirrors.bfsu.edu.cn", ms: null, mbps: null },
  { id: "aliyun", name: "阿里云 / npmmirror", scope: "Node / Python / Go / PyPI", probeUrl: "https://registry.npmmirror.com/-/binary/node/latest-v24.x/", probeBytes: 74776, baseUrl: "https://registry.npmmirror.com", ms: null, mbps: null },
  { id: "ustc", name: "中科大 USTC", scope: "校园网 / F-Droid / Rust / Node", probeUrl: "https://mirrors.ustc.edu.cn/node/index.json", probeBytes: NODE_INDEX_BYTES, baseUrl: "https://mirrors.ustc.edu.cn", ms: null, mbps: null },
  { id: "tencent", name: "腾讯云软件源", scope: "Node / Python / Go / PyPI", probeUrl: "https://mirrors.cloud.tencent.com/nodejs-release/index.json", probeBytes: NODE_INDEX_BYTES, baseUrl: "https://mirrors.cloud.tencent.com", ms: null, mbps: null },
  { id: "sjtu", name: "上海交大 SJTUG", scope: "校园网 / Rust / PyPI", probeUrl: "https://mirror.sjtu.edu.cn/favicon.ico", probeBytes: 15000, baseUrl: "https://mirror.sjtu.edu.cn", ms: null, mbps: null },
  { id: "bytedance", name: "字节跳动 rsproxy", scope: "Rust / Cargo 专属高速源", probeUrl: "https://rsproxy.cn/", probeBytes: 12000, baseUrl: "https://rsproxy.cn", ms: null, mbps: null },
];

const INITIAL_GH_PROXIES: GhProxyProbeItem[] = [
  { host: "cdn.gh-proxy.org", name: "GH-Proxy CDN", probeUrl: "https://cdn.gh-proxy.org/", ms: null },
  { host: "gh-proxy.org", name: "GH-Proxy 主线", probeUrl: "https://gh-proxy.org/", ms: null },
  { host: "ghfast.top", name: "GHFast 加速", probeUrl: "https://ghfast.top/", ms: null },
  { host: "wget.la", name: "Wget.la 加速", probeUrl: "https://wget.la/", ms: null },
];

type RecipeKey = "nodejs" | "python" | "rust" | "go" | "fdroid" | "chsrc";

function buildRecipeCommand(recipe: RecipeKey, mirrorId: string): { title: string; cmd: string; tip: string } {
  switch (recipe) {
    case "nodejs": {
      const reg =
        mirrorId === "tencent"
          ? "https://mirrors.cloud.tencent.com/npm/"
          : "https://registry.npmmirror.com";
      return {
        title: "Node.js / npm 换源命令",
        cmd: `npm config set registry ${reg}\npnpm config set registry ${reg}`,
        tip: "或运行 chsrc set nodejs 自动测速写入",
      };
    }
    case "python": {
      const idx =
        mirrorId === "tuna"
          ? "https://pypi.tuna.tsinghua.edu.cn/simple"
          : mirrorId === "ustc"
          ? "https://pypi.mirrors.ustc.edu.cn/simple"
          : mirrorId === "tencent"
          ? "https://mirrors.cloud.tencent.com/pypi/simple"
          : mirrorId === "bfsu"
          ? "https://mirrors.bfsu.edu.cn/pypi/web/simple"
          : "https://mirrors.aliyun.com/pypi/simple/";
      return {
        title: "Python / pip 换源命令",
        cmd: `pip config set global.index-url ${idx}`,
        tip: "同时适用于 uv / pdm / poetry",
      };
    }
    case "rust": {
      const base =
        mirrorId === "tuna"
          ? "https://mirrors.tuna.tsinghua.edu.cn/rustup"
          : mirrorId === "ustc"
          ? "https://mirrors.ustc.edu.cn/rust-static"
          : "https://rsproxy.cn";
      return {
        title: "Rust / Cargo 换源环境变量",
        cmd: `export RUSTUP_DIST_SERVER="${base}"\nexport RUSTUP_UPDATE_ROOT="${base}/rustup"`,
        tip: "推荐字节跳动 rsproxy.cn 或中科大 USTC",
      };
    }
    case "go": {
      const goproxy =
        mirrorId === "aliyun"
          ? "https://mirrors.aliyun.com/goproxy/,direct"
          : "https://goproxy.cn,direct";
      return {
        title: "Go / GOPROXY 换源命令",
        cmd: `go env -w GO111MODULE=on\ngo env -w GOPROXY=${goproxy}`,
        tip: "国内拉取 Go 模块秒速完成",
      };
    }
    case "fdroid": {
      const repo =
        mirrorId === "ustc"
          ? "https://mirrors.ustc.edu.cn/fdroid/repo"
          : mirrorId === "bfsu"
          ? "https://mirrors.bfsu.edu.cn/fdroid/repo"
          : mirrorId === "nju"
          ? "https://mirrors.nju.edu.cn/fdroid/repo"
          : "https://mirrors.tuna.tsinghua.edu.cn/fdroid/repo";
      return {
        title: "Android F-Droid 手机端国内镜像源",
        cmd: repo,
        tip: "打开 F-Droid → 设置 → 存储库 → 添加/启用上述国内镜像地址",
      };
    }
    default:
      return {
        title: "chsrc 全平台全语言一键换源 CLI",
        cmd: "# Linux / macOS\ncurl https://chsrc.run/posix | bash\n# Windows PowerShell\nirm https://chsrc.run/windows | iex",
        tip: "安装后直接运行 chsrc set nodejs / python / rust / go / ubuntu",
      };
  }
}

async function probePayload(url: string, knownBytes: number, timeoutMs = 5000): Promise<{ ms: number; mbps: number }> {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), timeoutMs);
  const t0 = performance.now();
  try {
    const resp = await fetch(`${url}${url.includes("?") ? "&" : "?"}_t=${Date.now()}`, {
      mode: "no-cors",
      cache: "no-store",
      signal: controller.signal,
    });
    await resp.blob();
    const elapsedMs = Math.max(1, Math.round(performance.now() - t0));
    const mbps = Number(((knownBytes / 1048576) / (elapsedMs / 1000)).toFixed(2));
    return { ms: elapsedMs, mbps };
  } catch {
    return { ms: -1, mbps: -1 };
  } finally {
    window.clearTimeout(timer);
  }
}

async function probeEndpoint(url: string, timeoutMs = 4000): Promise<number> {
  const res = await probePayload(url, 8192, timeoutMs);
  return res.ms;
}

interface ChsrcWebPanelProps {
  activeMirror: string;
  activeGhProxy: string;
  onSelectMirror: (mirrorId: string) => void;
  onSelectGhProxy: (host: string) => void;
}

export function ChsrcWebPanel({
  activeMirror,
  activeGhProxy,
  onSelectMirror,
  onSelectGhProxy,
}: ChsrcWebPanelProps) {
  const [mirrors, setMirrors] = useState<MirrorProbeItem[]>(INITIAL_MIRRORS);
  const [ghProxies, setGhProxies] = useState<GhProxyProbeItem[]>(INITIAL_GH_PROXIES);
  const [testing, setTesting] = useState(false);
  const [recipe, setRecipe] = useState<RecipeKey>("nodejs");
  const [copied, setCopied] = useState(false);
  const [expanded, setExpanded] = useState(true);

  const runSpeedtest = useCallback(async () => {
    setTesting(true);
    setMirrors(INITIAL_MIRRORS.map((m) => ({ ...m, ms: null })));
    setGhProxies(INITIAL_GH_PROXIES.map((g) => ({ ...g, ms: null })));

    const mirrorResults = await Promise.all(
      INITIAL_MIRRORS.map(async (m) => {
        const res = await probePayload(m.probeUrl, m.probeBytes);
        return { ...m, ms: res.ms, mbps: res.mbps };
      })
    );
    mirrorResults.sort((a, b) => {
      if (a.mbps === null || a.mbps < 0) return 1;
      if (b.mbps === null || b.mbps < 0) return -1;
      return b.mbps - a.mbps;
    });
    setMirrors(mirrorResults);

    const fastestMirror = mirrorResults.find((m) => m.mbps !== null && m.mbps > 0);
    if (fastestMirror) {
      onSelectMirror(fastestMirror.id);
    }

    const proxyResults = await Promise.all(
      INITIAL_GH_PROXIES.map(async (g) => ({
        ...g,
        ms: await probeEndpoint(g.probeUrl),
      }))
    );
    proxyResults.sort((a, b) => {
      if (a.ms === null || a.ms < 0) return 1;
      if (b.ms === null || b.ms < 0) return -1;
      return a.ms - b.ms;
    });
    setGhProxies(proxyResults);

    const fastestProxy = proxyResults.find((g) => g.ms !== null && g.ms > 0);
    if (fastestProxy) {
      onSelectGhProxy(fastestProxy.host);
    }

    setTesting(false);
  }, [onSelectGhProxy, onSelectMirror]);

  useEffect(() => {
    runSpeedtest();
  }, [runSpeedtest]);

  const activeMirrorObj = mirrors.find((m) => m.id === activeMirror);
  const activeProxyObj = ghProxies.find((g) => g.host === activeGhProxy);
  const currentRecipe = buildRecipeCommand(recipe, activeMirror);

  function copyRecipe() {
    navigator.clipboard.writeText(currentRecipe.cmd).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    });
  }

  function latencyBadge(ms: number | null) {
    if (ms === null) return <span className="text-[11px] text-muted-foreground">测速中...</span>;
    if (ms < 0) return <span className="text-[11px] text-destructive">超时</span>;
    const color =
      ms <= 120
        ? "text-emerald-600 dark:text-emerald-400"
        : ms <= 300
        ? "text-amber-600 dark:text-amber-400"
        : "text-orange-500";
    return <span className={`font-mono text-xs font-semibold ${color}`}>{ms} ms</span>;
  }

  return (
    <Card className="p-4 space-y-4 border-emerald-500/30 bg-emerald-500/[0.03]">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-bold flex items-center gap-1.5">
              <Activity className="h-4 w-4 text-emerald-500" />
              chsrc Web · 浏览器实时测速 & 自动切换最快下载源
            </span>
            {activeMirrorObj && (
              <Badge variant="emerald" className="gap-1">
                <Crown className="h-3 w-3" />
                镜像：{activeMirrorObj.name}
                {activeMirrorObj.ms && activeMirrorObj.ms > 0 ? ` (${activeMirrorObj.ms}ms)` : ""}
              </Badge>
            )}
            {activeProxyObj && (
              <Badge variant="outline" className="font-mono text-[11px]">
                GitHub 加速：{activeProxyObj.host}
                {activeProxyObj.ms && activeProxyObj.ms > 0 ? ` (${activeProxyObj.ms}ms)` : ""}
              </Badge>
            )}
          </div>
          <p className="text-xs text-muted-foreground">
            直接从您当前网络并发测速 8 大国内官方镜像站 + 4 组 GitHub 加速节点，自动切至最低延迟源；零本站存储，支持夸克网盘与 Wrangler。
          </p>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          <Button size="sm" variant="emerald" onClick={runSpeedtest} disabled={testing}>
            <RefreshCw className={`h-3.5 w-3.5 ${testing ? "animate-spin" : ""}`} />
            {testing ? "测速中..." : "重新测速并择优"}
          </Button>
          <Button size="sm" variant="outline" onClick={() => setExpanded((e) => !e)}>
            {expanded ? "收起面板" : "展开测速榜"}
          </Button>
        </div>
      </div>

      {expanded && (
        <div className="space-y-4 pt-2 border-t">
          {/* Two-column live speedtest leaderboards */}
          <div className="grid gap-4 lg:grid-cols-3">
            {/* 8 Official China Mirrors */}
            <div className="lg:col-span-2 space-y-2">
              <div className="text-xs font-semibold text-muted-foreground flex items-center justify-between">
                <span>国内官方镜像站实时延迟（点击手动切换）</span>
                <span>桌面 + Android F-Droid</span>
              </div>
              <div className="grid gap-2 sm:grid-cols-2">
                {mirrors.map((m, idx) => {
                  const selected = activeMirror === m.id;
                  return (
                    <button
                      key={m.id}
                      type="button"
                      onClick={() => onSelectMirror(m.id)}
                      className={`flex items-center justify-between rounded-lg border px-3 py-2 text-left transition-colors cursor-pointer ${
                        selected
                          ? "border-emerald-500 bg-emerald-500/10"
                          : "bg-background hover:bg-accent"
                      }`}
                    >
                      <div className="min-w-0">
                        <div className="flex items-center gap-1.5 text-xs font-semibold truncate">
                          {idx === 0 && m.ms !== null && m.ms > 0 && (
                            <Crown className="h-3 w-3 text-amber-500 shrink-0" />
                          )}
                          <span className="truncate">{m.name}</span>
                        </div>
                        <div className="text-[10px] text-muted-foreground truncate">
                          {m.scope}
                        </div>
                      </div>
                      <div className="shrink-0 pl-2 text-right">
                        {m.mbps !== null && m.mbps > 0 ? (
                          <div className="font-mono text-xs font-bold text-emerald-600 dark:text-emerald-400">
                            {m.mbps >= 1 ? `${m.mbps.toFixed(1)} MB/s` : `${Math.round(m.mbps * 1024)} KB/s`}
                          </div>
                        ) : null}
                        <div>{latencyBadge(m.ms)}</div>
                      </div>
                    </button>
                  );
                })}
              </div>
            </div>

            {/* 4 GitHub Release Accelerators */}
            <div className="space-y-2">
              <div className="text-xs font-semibold text-muted-foreground flex items-center justify-between">
                <span>GitHub Release 加速节点</span>
                <span>实时竞速</span>
              </div>
              <div className="space-y-2">
                {ghProxies.map((g, idx) => {
                  const selected = activeGhProxy === g.host;
                  return (
                    <button
                      key={g.host}
                      type="button"
                      onClick={() => onSelectGhProxy(g.host)}
                      className={`flex w-full items-center justify-between rounded-lg border px-3 py-2 text-left transition-colors cursor-pointer ${
                        selected
                          ? "border-emerald-500 bg-emerald-500/10"
                          : "bg-background hover:bg-accent"
                      }`}
                    >
                      <div className="min-w-0">
                        <div className="flex items-center gap-1.5 text-xs font-semibold">
                          {idx === 0 && g.ms !== null && g.ms > 0 && (
                            <Zap className="h-3 w-3 text-emerald-500 shrink-0" />
                          )}
                          <span>{g.name}</span>
                        </div>
                        <div className="font-mono text-[10px] text-muted-foreground truncate">
                          {g.host}
                        </div>
                      </div>
                      <div className="shrink-0 pl-2">{latencyBadge(g.ms)}</div>
                    </button>
                  );
                })}
              </div>
            </div>
          </div>

          {/* Interactive chsrc Web Recipe Generator */}
          <div className="rounded-lg border bg-background p-3.5 space-y-2.5">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex flex-wrap items-center gap-1.5">
                {(
                  [
                    ["nodejs", "Node.js / npm"],
                    ["python", "Python / pip"],
                    ["rust", "Rust / Cargo"],
                    ["go", "Go / GOPROXY"],
                    ["fdroid", "Android F-Droid"],
                    ["chsrc", "chsrc CLI"],
                  ] as [RecipeKey, string][]
                ).map(([k, label]) => (
                  <Button
                    key={k}
                    size="sm"
                    variant={recipe === k ? "default" : "ghost"}
                    className="h-7 px-2.5 text-xs"
                    onClick={() => setRecipe(k)}
                  >
                    {k === "fdroid" ? (
                      <Smartphone className="h-3 w-3" />
                    ) : (
                      <Terminal className="h-3 w-3" />
                    )}
                    {label}
                  </Button>
                ))}
              </div>
              <Button size="sm" variant="outline" className="h-7 text-xs" onClick={copyRecipe}>
                {copied ? (
                  <>
                    <Check className="h-3 w-3 text-emerald-500" /> 已复制
                  </>
                ) : (
                  <>
                    <Copy className="h-3 w-3" /> 复制换源命令
                  </>
                )}
              </Button>
            </div>
            <pre className="rounded bg-muted/70 px-3 py-2 font-mono text-xs overflow-x-auto leading-relaxed">
              {currentRecipe.cmd}
            </pre>
            <div className="text-[11px] text-muted-foreground">
              {currentRecipe.title} · {currentRecipe.tip}
            </div>
          </div>
        </div>
      )}
    </Card>
  );
}
