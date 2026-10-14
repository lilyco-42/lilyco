import { useCallback, useEffect, useRef, useState } from "react";
import { chooseOfficeFile, fileName, officeBridge, recommendedCommand, type OfficeCommand, type OfficeValue } from "./office-bridge";

interface OfficeResult { file: string; command: string; value: OfficeValue; elapsed: number }
export function useOfficeWorkspace() {
  const [commands, setCommands] = useState<OfficeCommand[]>([]);
  const [file, setFile] = useState<string | null>(null);
  const [command, setCommand] = useState("office-info");
  const [result, setResult] = useState<OfficeResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [recent, setRecent] = useState<string[]>([]);
  const [dragging, setDragging] = useState(false);
  const locked = useRef(false);

  const selectFile = useCallback((path: string) => {
    if (locked.current) return;
    setFile(path); setCommand(recommendedCommand(path)); setResult(null); setError("");
    setRecent(previous => [path, ...previous.filter(item => item !== path)].slice(0, 8));
  }, []);

  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    try {
      const bridge = officeBridge();
      bridge.core.invoke<OfficeCommand[]>("office_commands").then(items => {
        if (!Array.isArray(items) || items.some(item => typeof item.name !== "string" || typeof item.hint !== "string")) throw new Error("工具清单格式错误，请重新安装当前版本。");
        if (alive) setCommands(items);
      }).catch(failure => { if (alive) setError(String(failure)); });
      bridge.window.getCurrentWindow().onDragDropEvent(event => {
        if (!alive) return;
        setDragging(event.payload.type === "enter" || event.payload.type === "over");
        if (event.payload.type === "drop") {
          setDragging(false);
          if (event.payload.paths?.length === 1) selectFile(event.payload.paths[0]);
          else if (!locked.current) setError("每次请拖入一个文件，读取完毕后可以选择下一份。");
        }
      }).then(stop => { if (alive) unlisten = stop; else stop(); }).catch(failure => { if (alive) setError(`拖入文件不可用，请使用选择文件按钮。${String(failure)}`); });
    } catch (failure) { setError(String(failure)); }
    return () => { alive = false; unlisten?.(); };
  }, [selectFile]);

  async function pickFile() {
    if (locked.current) return;
    try {
      const path = await chooseOfficeFile();
      if (typeof path === "string") selectFile(path);
    } catch (failure) { setError(String(failure)); }
  }

  async function readFile() {
    if (locked.current || !file || !commands.some(item => item.name === command)) return;
    const request = { file, command };
    locked.current = true; setBusy(true); setError(""); setResult(null);
    const started = performance.now();
    try {
      const value = await officeBridge().core.invoke<OfficeValue>("office_run", { command: request.command, path: request.file });
      setResult({ ...request, value, elapsed: performance.now() - started });
    } catch (failure) { setError(`未能读取 ${fileName(request.file)}：${String(failure)}`); }
    finally { locked.current = false; setBusy(false); }
  }

  function clear() {
    if (locked.current) return;
    setFile(null); setResult(null); setError("");
  }
  function chooseCommand(name: string) {
    if (locked.current || !commands.some(item => item.name === name)) return;
    setCommand(name); setResult(null); setError("");
  }
  return { commands, file, command, setCommand: chooseCommand, result, busy, error, recent, dragging, pickFile, readFile, selectFile, clear,
    forget: (path: string) => setRecent(previous => previous.filter(item => item !== path)),
    forgetAll: () => setRecent([]) };
}
