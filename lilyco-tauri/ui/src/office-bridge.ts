export type OfficeValue = null | boolean | number | string | OfficeValue[] | { [key: string]: OfficeValue };
export interface OfficeCommand { name: string; hint: string }
interface DragEvent { payload: { type: "enter" | "over" | "drop" | "leave"; paths?: string[] } }
interface OfficeBridge {
  core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> };
  window: { getCurrentWindow(): { onDragDropEvent(callback: (event: DragEvent) => void): Promise<() => void> } };
}
declare global { interface Window { __TAURI__?: OfficeBridge } }

export function officeBridge(): OfficeBridge {
  if (!window.__TAURI__) throw new Error("请从 lilyco office 桌面应用打开工作台，浏览器预览无法读取本机文件。");
  return window.__TAURI__;
}

export function chooseOfficeFile() {
  // Dialog's plugin initializes alert/confirm, not a __TAURI__.dialog export.
  // This is the same IPC command used by the official plugin-dialog guest API.
  return officeBridge().core.invoke<string | string[] | null>("plugin:dialog|open", { options: {
    multiple: false, title: "选择要读取的办公文件",
    filters: [
      { name: "办公文件", extensions: ["docx", "docm", "dotx", "doc", "odt", "rtf", "xlsx", "xlsm", "xls", "ods", "csv", "pptx", "ppt", "odp", "pdf"] },
      { name: "全部文件", extensions: ["*"] },
    ],
  } });
}

export function recommendedCommand(path: string): string {
  const extension = path.split(".").pop()?.toLowerCase();
  if (["xlsx", "xlsm", "xls", "ods", "csv"].includes(extension ?? "")) return "office-sheet";
  if (["pptx", "ppt", "odp"].includes(extension ?? "")) return "office-slide";
  if (extension === "pdf") return "office-pdf";
  if (["docx", "docm", "dotx", "doc", "odt", "rtf"].includes(extension ?? "")) return "office-doc";
  return "identify";
}

export function fileName(path: string) { return path.split(/[\\/]/).pop() || path; }

export const commandLabels: Record<string, string> = {
  "office-info": "文件概览", "office-text": "正文文字", "office-meta": "文档属性",
  "office-doc": "段落与表格", "office-sheet": "工作表与单元格", "office-slide": "幻灯片与备注",
  "office-package": "文件包结构", "office-objects": "嵌入内容与宏", "office-pdf": "PDF 结构", "identify": "识别文件格式",
};
