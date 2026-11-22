import { useEffect, useState } from "react";
import { Check, Copy, Cpu, Search, Terminal, Zap } from "lucide-react";
import { apiFetch, type AiModel } from "../lib/api";
import { Badge, Button, Card, Input } from "../components/ui/primitives";

const OPENAI_SNIPPET = `import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://dev.lain42.top/relay/v1",
  apiKey: "sk-lain42-your-token",
});

const res = await client.chat.completions.create({
  model: "claude-sonnet-4-5",
  messages: [{ role: "user", content: "Hello Lain42!" }],
});
console.log(res.choices[0].message.content);`;

const ANTHROPIC_SNIPPET = `export ANTHROPIC_BASE_URL="https://dev.lain42.top/relay"
export ANTHROPIC_AUTH_TOKEN="sk-lain42-your-token"

# Claude Code / Anthropic SDK 直接生效
claude`;

export function ModelsPage() {
  const [models, setModels] = useState<AiModel[]>([]);
  const [query, setQuery] = useState("");
  const [provider, setProvider] = useState("all");
  const [copiedId, setCopiedId] = useState("");

  useEffect(() => {
    apiFetch<{ models?: AiModel[] } | AiModel[]>("/api/models")
      .then((res) => {
        const list = Array.isArray(res) ? res : res.models || [];
        setModels(list);
      })
      .catch(() => {});
  }, []);

  const providers = [
    "all",
    ...Array.from(new Set(models.map((m) => m.provider).filter(Boolean))),
  ];

  const filtered = models.filter((m) => {
    if (provider !== "all" && m.provider !== provider) return false;
    if (!query.trim()) return true;
    const q = query.trim().toLowerCase();
    return (
      m.id.toLowerCase().includes(q) ||
      m.display_name.toLowerCase().includes(q) ||
      m.provider.toLowerCase().includes(q) ||
      m.category.toLowerCase().includes(q)
    );
  });

  function copyText(id: string, text: string) {
    navigator.clipboard.writeText(text).then(() => {
      setCopiedId(id);
      setTimeout(() => setCopiedId(""), 1800);
    });
  }

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight flex items-center gap-2">
            <Cpu className="h-6 w-6 text-emerald-500" />
            AI 模型价格与聚合中转
          </h1>
          <p className="text-sm text-muted-foreground mt-1">
            OpenAI / Anthropic 双协议原生兼容 · 单一 API Key 畅享全模型 · 按量透明扣费
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Badge variant="emerald">OpenAI /relay/v1</Badge>
          <Badge variant="outline">Anthropic /relay</Badge>
        </div>
      </div>

      {/* Quick Start Snippets */}
      <div className="grid gap-4 lg:grid-cols-2">
        <Card className="p-4 space-y-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Terminal className="h-4 w-4 text-emerald-500" />
              OpenAI SDK / 通用客户端接入
            </div>
            <Button
              size="sm"
              variant="outline"
              onClick={() => copyText("openai", OPENAI_SNIPPET)}
            >
              {copiedId === "openai" ? (
                <>
                  <Check className="h-3.5 w-3.5 text-emerald-500" />
                  已复制
                </>
              ) : (
                <>
                  <Copy className="h-3.5 w-3.5" />
                  复制代码
                </>
              )}
            </Button>
          </div>
          <pre className="rounded-lg bg-muted/70 p-3 text-xs font-mono overflow-x-auto leading-relaxed">
            {OPENAI_SNIPPET}
          </pre>
        </Card>

        <Card className="p-4 space-y-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Zap className="h-4 w-4 text-amber-500" />
              Claude Code / Anthropic 原生接入
            </div>
            <Button
              size="sm"
              variant="outline"
              onClick={() => copyText("anthropic", ANTHROPIC_SNIPPET)}
            >
              {copiedId === "anthropic" ? (
                <>
                  <Check className="h-3.5 w-3.5 text-emerald-500" />
                  已复制
                </>
              ) : (
                <>
                  <Copy className="h-3.5 w-3.5" />
                  复制命令
                </>
              )}
            </Button>
          </div>
          <pre className="rounded-lg bg-muted/70 p-3 text-xs font-mono overflow-x-auto leading-relaxed">
            {ANTHROPIC_SNIPPET}
          </pre>
        </Card>
      </div>

      {/* Filters */}
      <Card className="p-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex flex-wrap gap-1.5">
          {providers.map((p) => (
            <Button
              key={p}
              size="sm"
              variant={provider === p ? "default" : "ghost"}
              onClick={() => setProvider(p)}
            >
              {p === "all" ? `全部厂商 (${models.length})` : p}
            </Button>
          ))}
        </div>
        <div className="relative w-full sm:w-64">
          <Search className="h-4 w-4 text-muted-foreground absolute left-2.5 top-2.5" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索模型 ID 或名称..."
            className="pl-8"
          />
        </div>
      </Card>

      {/* Models Table */}
      <Card className="overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="border-b bg-muted/40 text-xs text-muted-foreground">
              <tr>
                <th className="py-3 px-4 text-left font-medium">模型 ID</th>
                <th className="py-3 px-4 text-left font-medium">厂商 / 分组</th>
                <th className="py-3 px-4 text-right font-medium">输入 (积分/M)</th>
                <th className="py-3 px-4 text-right font-medium">输出 (积分/M)</th>
                <th className="py-3 px-4 text-right font-medium">延迟</th>
                <th className="py-3 px-4 text-center font-medium">状态</th>
              </tr>
            </thead>
            <tbody className="divide-y">
              {filtered.map((m) => (
                <tr key={m.id} className="hover:bg-muted/30 transition-colors">
                  <td className="py-3 px-4">
                    <div className="flex items-center gap-2">
                      <button
                        type="button"
                        onClick={() => copyText(m.id, m.id)}
                        className="font-mono text-xs font-medium text-foreground hover:text-emerald-500 transition-colors text-left"
                        title="点击复制模型 ID"
                      >
                        {m.id}
                      </button>
                      {copiedId === m.id && (
                        <span className="text-[10px] text-emerald-500">已复制</span>
                      )}
                    </div>
                    {m.note && (
                      <p className="text-xs text-muted-foreground mt-0.5">{m.note}</p>
                    )}
                  </td>
                  <td className="py-3 px-4">
                    <div className="flex items-center gap-1.5">
                      <Badge variant="outline">{m.provider}</Badge>
                      <span className="text-xs text-muted-foreground">
                        {m.group_name || m.category}
                      </span>
                    </div>
                  </td>
                  <td className="py-3 px-4 text-right font-mono text-xs">
                    {m.input_credits_per_m}
                  </td>
                  <td className="py-3 px-4 text-right font-mono text-xs">
                    {m.output_credits_per_m}
                  </td>
                  <td className="py-3 px-4 text-right font-mono text-xs text-muted-foreground">
                    {m.latency_ms > 0 ? `${m.latency_ms} ms` : "—"}
                  </td>
                  <td className="py-3 px-4 text-center">
                    <Badge variant={m.available ? "emerald" : "amber"}>
                      {m.available ? "可用" : "维护"}
                    </Badge>
                  </td>
                </tr>
              ))}
              {filtered.length === 0 && (
                <tr>
                  <td
                    colSpan={6}
                    className="py-10 text-center text-sm text-muted-foreground"
                  >
                    暂无匹配的模型数据
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  );
}
