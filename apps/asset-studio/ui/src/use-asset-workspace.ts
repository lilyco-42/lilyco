import { useEffect, useRef, useState } from "react";

export type AssetTool = "aic" | "spine";
type Action = "inspect" | "render" | "sprites" | "textures";
interface Workspace { id: string; count: number; bytes: number; entries: Record<AssetTool, string[]> }
interface Motion { title: string }
interface Result { motions?: Motion[]; files?: { poses?: Motion[] }[]; [key: string]: unknown }
interface Output { name: string; size: number }
interface Job { id: string; state: string; error?: string; result?: Result; files: Output[] }
type Capabilities = Partial<Record<AssetTool, { available: boolean }>>;

async function api<T>(path: string, options: RequestInit = {}): Promise<T> {
  const response = await fetch("/api/" + path, options);
  const data = await response.json();
  if (!response.ok) throw new Error(typeof data.detail === "string" ? data.detail : "请求参数无效，请重新选择文件");
  return data;
}
export function assetFileUrl(job: string, name: string) {
  return `/api/jobs/${job}/files/` + name.split("/").map(encodeURIComponent).join("/");
}
const extensions = /\.(pxls|dat|bytes|png|atlas|json|skel|ress)$/i;
const messageOf = (error: unknown) => error instanceof Error ? error.message : "处理失败，请重试";

export function useAssetWorkspace() {
  const [tool, setTool] = useState<AssetTool>("aic");
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [capabilities, setCapabilities] = useState<Capabilities>({});
  const [entry, setEntry] = useState("");
  const [motions, setMotions] = useState<Motion[]>([]);
  const [motion, setMotion] = useState("*");
  const [direction, setDirection] = useState(0);
  const [scale, setScale] = useState(1);
  const [status, setStatus] = useState("等待选择素材");
  const [error, setError] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [starting, setStarting] = useState(false);
  const [activeJob, setActiveJob] = useState<string | null>(null);
  const [result, setResult] = useState<Job | null>(null);
  const generation = useRef(0);
  const running = useRef(false);
  const poll = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    api<{ tools: Capabilities }>("capabilities").then(data => { if (alive.current) setCapabilities(data.tools); })
      .catch(e => { if (alive.current) { setStatus(messageOf(e)); setError(true); } });
    return () => { alive.current = false; generation.current++; clearTimeout(poll.current); };
  }, []);
  function selectTool(next: AssetTool, selected = workspace) {
    setTool(next); setEntry(selected?.entries[next][0] || ""); setMotions([]); setMotion("*");
  }
  function selectEntry(next: string) { setEntry(next); setMotions([]); setMotion("*"); }
  async function upload(list: FileList | File[]) {
    if (running.current) return;
    const files = [...list].filter(f => extensions.test(f.name));
    if (!files.length) { setStatus("没有找到角色文件或配套贴图"); setError(true); return; }
    if (files.length > 1000 || files.some(f => f.size > 128 * 1024 * 1024) || files.reduce((n, f) => n + f.size, 0) > 512 * 1024 * 1024) {
      setStatus("请选择不超过 1000 个文件，单文件最多 128 MB，总计最多 512 MB"); setError(true); return;
    }
    running.current = true; setUploading(true); setError(false); setStatus("正在加载到本机工作区…");
    const form = new FormData();
    files.forEach(file => form.append("files", file, file.name));
    form.append("paths", JSON.stringify(files.map(f => f.webkitRelativePath || f.name)));
    let next: Workspace | undefined;
    try {
      next = await api<Workspace>("workspaces", { method: "POST", body: form });
      if (workspace) await api("workspaces/" + workspace.id, { method: "DELETE" });
      setWorkspace(next); setResult(null); selectTool(next.entries.aic.length ? "aic" : "spine", next);
      setStatus("文件已加载。先读取动画列表，再选择动作预览。");
    } catch (e) {
      if (next) await api("workspaces/" + next.id, { method: "DELETE" }).catch(() => undefined);
      setStatus(messageOf(e)); setError(true);
    } finally { running.current = false; setUploading(false); }
  }
  async function run(action: Action) {
    if (!workspace || running.current || !entry || !capabilities[tool]?.available) return;
    const thisGeneration = ++generation.current;
    running.current = true; setStarting(true); setError(false); setStatus("引擎正在处理，文件保留在本机…");
    try {
      const job = await api<Job>("jobs", { method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ session: workspace.id, tool, action, entry, motion, direction, scale }) });
      setActiveJob(job.id); setStarting(false);
      const check = async () => {
        try {
          const current = await api<Job>("jobs/" + job.id);
          if (!alive.current || generation.current !== thisGeneration) return;
          if (["done", "failed", "cancelled"].includes(current.state)) {
            setActiveJob(null); running.current = false;
            if (current.state === "done") {
              setResult(current);
              const found = tool === "aic" ? current.result?.files?.flatMap(f => f.poses || []) : current.result?.motions;
              if (action === "inspect" && found?.length) {
                setMotions(found); setMotion(found[0].title); setStatus(`已读取 ${found.length} 个动作，选择一个即可预览。`);
              } else setStatus(`处理完成 · ${current.files.length} 个结果文件`);
            } else { setStatus(current.error || "已取消处理。可以重新选择动作。"); setError(current.state === "failed"); }
          } else poll.current = setTimeout(check, 600);
        } catch (e) {
          if (!alive.current || generation.current !== thisGeneration) return;
          running.current = false; setActiveJob(null); setError(true); setStatus(messageOf(e) + "。可删除工作区后重新选择文件。");
        }
      };
      await check();
    } catch (e) { running.current = false; setStarting(false); setActiveJob(null); setError(true); setStatus(messageOf(e)); }
  }
  async function cancel() {
    if (activeJob) try { await api("jobs/" + activeJob + "/cancel", { method: "POST" }); }
    catch (e) { setError(true); setStatus(messageOf(e)); }
  }
  async function clear() {
    if (!workspace || running.current) return;
    running.current = true; setUploading(true);
    try {
      await api("workspaces/" + workspace.id, { method: "DELETE" });
      generation.current++; clearTimeout(poll.current); setWorkspace(null); setResult(null);
      setEntry(""); setMotions([]); setMotion("*"); setActiveJob(null); setError(false); setStatus("工作区已清理，可以选择新素材。");
    } catch (e) { setStatus(messageOf(e)); setError(true); }
    finally { running.current = false; setUploading(false); }
  }
  return { tool, workspace, capabilities, entry, motions, motion, direction, scale, status, error, uploading,
    activeJob, result, busy: uploading || starting || !!activeJob, selectTool, selectEntry, setMotion, setDirection, setScale, upload, run, cancel, clear };
}
