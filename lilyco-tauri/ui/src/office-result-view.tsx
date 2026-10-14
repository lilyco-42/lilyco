import { useState } from "react";
import { Button } from "./components/ui/button";
import type { OfficeValue } from "./office-bridge";

export function OfficeResultView({ value, depth = 0 }: { value: OfficeValue; depth?: number }) {
  const [limit, setLimit] = useState(40);
  if (value === null) return <span className="text-muted-foreground">未记录</span>;
  if (typeof value === "boolean") return <span>{value ? "是" : "否"}</span>;
  if (typeof value !== "object") return <span className="whitespace-pre-wrap break-words [overflow-wrap:anywhere]">{String(value)}</span>;
  if (depth > 5) return <details><summary className="cursor-pointer text-muted-foreground">展开深层数据</summary><pre className="overflow-auto whitespace-pre-wrap break-all">{JSON.stringify(value, null, 2)}</pre></details>;
  const count = Array.isArray(value) ? value.length : Object.keys(value).length;
  const entries: [string, OfficeValue][] = Array.isArray(value) ? value.slice(0, limit).map((item, index) => [String(index + 1), item]) : Object.entries(value).slice(0, limit);
  if (count === 0) return <span className="text-muted-foreground">没有记录</span>;
  return <div className="min-w-0 divide-y divide-border">
    {entries.map(([key, item]) => <div key={key} className="grid min-w-0 gap-2 py-3 sm:grid-cols-[minmax(90px,150px)_minmax(0,1fr)]">
      <span className="break-words text-sm font-medium text-muted-foreground">{key}</span>
      <div className="min-w-0 text-sm leading-7"><OfficeResultView value={item} depth={depth + 1} /></div>
    </div>)}
    {count > limit && <Button variant="outline" className="my-3 min-h-10" onClick={() => setLimit(previous => previous + 40)}>继续显示 · 还有 {count - limit} 项</Button>}
  </div>;
}
