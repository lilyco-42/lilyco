import { useState } from "react";
import { Binary, Clock, Code2, Hash, Sparkles, Wrench } from "lucide-react";
import { Badge, Button, Card, Input } from "../components/ui/primitives";

type ToolTab = "json" | "base64" | "sha256" | "timestamp" | "tokens";

export function ToolsPage() {
  const [tab, setTab] = useState<ToolTab>("json");

  // JSON
  const [jsonInput, setJsonInput] = useState('{"service":"lain42","speed_mbps":101.6,"protocols":["AnyTLS","Hysteria2","VLESS-Reality"]}');
  const [jsonOutput, setJsonOutput] = useState("");
  const [jsonErr, setJsonErr] = useState("");

  // Base64
  const [b64Input, setB64Input] = useState("https://dev.lain42.top/");
  const [b64Output, setB64Output] = useState("");

  // SHA-256
  const [hashInput, setHashInput] = useState("lain42.top");
  const [hashOutput, setHashOutput] = useState("");

  // Timestamp
  const [tsInput, setTsInput] = useState(() => String(Math.floor(Date.now() / 1000)));
  const [tsResult, setTsResult] = useState("");

  // Token Estimator
  const [tokenText, setTokenText] = useState("欢迎使用 Lain42 新一代商业化控制台，支持 AnyTLS 与 Hysteria2 极速节点。");

  function formatJson(indent: number) {
    try {
      const parsed = JSON.parse(jsonInput);
      setJsonOutput(JSON.stringify(parsed, null, indent));
      setJsonErr("");
    } catch (e) {
      setJsonErr(e instanceof Error ? e.message : "JSON 解析错误");
    }
  }

  function encodeB64() {
    try {
      const bytes = new TextEncoder().encode(b64Input);
      let bin = "";
      bytes.forEach((b) => (bin += String.fromCharCode(b)));
      setB64Output(btoa(bin));
    } catch {
      setB64Output("编码失败");
    }
  }

  function decodeB64() {
    try {
      const bin = atob(b64Input.trim());
      const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
      setB64Output(new TextDecoder().decode(bytes));
    } catch {
      setB64Output("解码失败：请检查是否为合法 Base64 字符串");
    }
  }

  async function calcSha256() {
    const data = new TextEncoder().encode(hashInput);
    const digest = await crypto.subtle.digest("SHA-256", data);
    const hex = Array.from(new Uint8Array(digest))
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
    setHashOutput(hex);
  }

  function convertTimestamp() {
    const raw = tsInput.trim();
    if (/^\d+$/.test(raw)) {
      const ms = raw.length <= 10 ? Number(raw) * 1000 : Number(raw);
      const d = new Date(ms);
      setTsResult(
        `本地时间: ${d.toLocaleString("zh-CN")}\nISO UTC: ${d.toISOString()}`
      );
    } else {
      const d = new Date(raw);
      if (Number.isNaN(d.getTime())) {
        setTsResult("无法解析的时间格式");
      } else {
        setTsResult(
          `秒级时间戳: ${Math.floor(d.getTime() / 1000)}\n毫秒时间戳: ${d.getTime()}`
        );
      }
    }
  }

  const cjkCount = (tokenText.match(/[\u4e00-\u9fff]/g) || []).length;
  const nonCjkCount = tokenText.length - cjkCount;
  const approxTokens = Math.ceil(cjkCount * 0.65 + nonCjkCount * 0.28);

  const tabs: { id: ToolTab; label: string; icon: React.ReactNode }[] = [
    { id: "json", label: "JSON 格式化", icon: <Code2 className="h-4 w-4" /> },
    { id: "base64", label: "Base64 编解码", icon: <Binary className="h-4 w-4" /> },
    { id: "sha256", label: "SHA-256 哈希", icon: <Hash className="h-4 w-4" /> },
    { id: "timestamp", label: "时间戳转换", icon: <Clock className="h-4 w-4" /> },
    { id: "tokens", label: "LLM Token 估算", icon: <Sparkles className="h-4 w-4" /> },
  ];

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight flex items-center gap-2">
            <Wrench className="h-6 w-6 text-emerald-500" />
            开发者本地工具箱
          </h1>
          <p className="text-sm text-muted-foreground mt-1">
            100% 浏览器本地计算，不上传任何敏感数据，极速零延迟
          </p>
        </div>
        <Badge variant="emerald">本地零上传</Badge>
      </div>

      <div className="flex flex-wrap gap-2">
        {tabs.map((t) => (
          <Button
            key={t.id}
            size="sm"
            variant={tab === t.id ? "default" : "outline"}
            onClick={() => setTab(t.id)}
          >
            {t.icon}
            {t.label}
          </Button>
        ))}
      </div>

      {tab === "json" && (
        <Card className="p-5 space-y-4">
          <textarea
            rows={6}
            value={jsonInput}
            onChange={(e) => setJsonInput(e.target.value)}
            className="w-full rounded-md border border-input bg-background p-3 font-mono text-xs"
            placeholder="输入 JSON 字符串..."
          />
          <div className="flex gap-2">
            <Button size="sm" variant="emerald" onClick={() => formatJson(2)}>
              格式化 (2 空格)
            </Button>
            <Button size="sm" variant="outline" onClick={() => formatJson(0)}>
              压缩单行
            </Button>
          </div>
          {jsonErr && <p className="text-xs text-destructive">{jsonErr}</p>}
          {jsonOutput && (
            <pre className="rounded-lg bg-muted/70 p-3 text-xs font-mono overflow-x-auto">
              {jsonOutput}
            </pre>
          )}
        </Card>
      )}

      {tab === "base64" && (
        <Card className="p-5 space-y-4">
          <textarea
            rows={4}
            value={b64Input}
            onChange={(e) => setB64Input(e.target.value)}
            className="w-full rounded-md border border-input bg-background p-3 font-mono text-xs"
          />
          <div className="flex gap-2">
            <Button size="sm" variant="emerald" onClick={encodeB64}>
              UTF-8 → Base64 编码
            </Button>
            <Button size="sm" variant="outline" onClick={decodeB64}>
              Base64 → UTF-8 解码
            </Button>
          </div>
          {b64Output && (
            <pre className="rounded-lg bg-muted/70 p-3 text-xs font-mono break-all whitespace-pre-wrap">
              {b64Output}
            </pre>
          )}
        </Card>
      )}

      {tab === "sha256" && (
        <Card className="p-5 space-y-4">
          <textarea
            rows={3}
            value={hashInput}
            onChange={(e) => setHashInput(e.target.value)}
            className="w-full rounded-md border border-input bg-background p-3 font-mono text-xs"
          />
          <Button size="sm" variant="emerald" onClick={calcSha256}>
            计算 SHA-256 摘要
          </Button>
          {hashOutput && (
            <div className="rounded-lg bg-muted/70 p-3 text-xs font-mono break-all">
              {hashOutput}
            </div>
          )}
        </Card>
      )}

      {tab === "timestamp" && (
        <Card className="p-5 space-y-4">
          <div className="flex flex-col gap-2 sm:flex-row">
            <Input
              value={tsInput}
              onChange={(e) => setTsInput(e.target.value)}
              placeholder="输入 Unix 时间戳或 ISO 时间字符串"
              className="font-mono"
            />
            <Button size="sm" variant="emerald" onClick={convertTimestamp}>
              转换
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => setTsInput(String(Math.floor(Date.now() / 1000)))}
            >
              当前时间
            </Button>
          </div>
          {tsResult && (
            <pre className="rounded-lg bg-muted/70 p-3 text-xs font-mono">
              {tsResult}
            </pre>
          )}
        </Card>
      )}

      {tab === "tokens" && (
        <Card className="p-5 space-y-4">
          <textarea
            rows={6}
            value={tokenText}
            onChange={(e) => setTokenText(e.target.value)}
            className="w-full rounded-md border border-input bg-background p-3 text-sm"
            placeholder="粘贴任意中文/英文/代码文本以估算 Token 数量..."
          />
          <div className="grid gap-3 sm:grid-cols-3">
            <div className="rounded-lg border p-3">
              <div className="text-xs text-muted-foreground">总字符数</div>
              <div className="text-xl font-bold font-mono">{tokenText.length}</div>
            </div>
            <div className="rounded-lg border p-3">
              <div className="text-xs text-muted-foreground">中文字符数</div>
              <div className="text-xl font-bold font-mono">{cjkCount}</div>
            </div>
            <div className="rounded-lg border border-emerald-500/30 p-3">
              <div className="text-xs text-muted-foreground">预估 Tokens (GPT/Claude)</div>
              <div className="text-xl font-bold font-mono text-emerald-600 dark:text-emerald-400">
                ~{approxTokens}
              </div>
            </div>
          </div>
        </Card>
      )}
    </div>
  );
}
