import { Crop, ScanLine, Scaling, Palette, Layers, Eraser, PenTool, Monitor } from 'lucide-react';

export const visionTools = [
  { id: 'crop', title: '裁剪与放大', description: '框选需要的像素区域，保留 PNG 透明度。', icon: Crop, defaults: { region: '0,0,64,64', scale: 1 } },
  { id: 'resize', title: '调整尺寸', description: '设置宽高，单边填 0 可以保持原比例。', icon: Scaling, defaults: { width: 1024, height: 0 } },
  { id: 'dominant-colors', title: '主色提取', description: '提取配色，或比较指定候选色与图片的匹配程度。', icon: Palette, defaults: { top: 5, tolerance: 8, region: '', candidates: '' } },
  { id: 'pixel-diff', title: '图片对比', description: '比较两张图片，生成差异热图与区域评分。', icon: Layers, defaults: { grid: 6, top: 5 } },
  { id: 'extract-foreground', title: '纯色背景提取', description: '从边缘去除连通背景。适合简单底色，不是 AI 抠图。', icon: Eraser, defaults: { mode: 'color', exclude_tol: 35, region: '', exclude_color: '' } },
  { id: 'trace', title: '位图转 SVG', description: '用 vtracer 生成可编辑的矢量路径。', icon: PenTool, defaults: { scale: 0, color: true, polygon: false } },
  { id: 'image-info', title: '图片信息', description: '查看原图格式、尺寸和文件大小。', icon: ScanLine, defaults: {} },
  { id: 'html-screenshot', title: 'HTML 截图', description: '截取自己的本地 HTML，需要已安装 Chrome 或 Edge。', icon: Monitor, defaults: { width: 1280, height: 800 } },
] as const;
export type VisionTool = typeof visionTools[number]['id'];
export type VisionParams = Record<string, string | number | boolean>;
export type VisionJob = { id: string; tool: VisionTool; state: 'queued' | 'running' | 'done' | 'failed' | 'cancelled';
  error: string | null; result: Record<string, unknown> | null; files: { name: string; size: number }[];
  progress: Record<string, unknown> | null };

export async function visionRequest<T>(path: string, options?: RequestInit): Promise<T> {
  const response = await fetch('/api/' + path, options);
  const value = await response.json();
  if (!response.ok) throw new Error(typeof value.detail === 'string' ? value.detail : '文件或参数无效，请核对后重试');
  return value as T;
}
export const visionFileUrl = (job: VisionJob, name: string) => `/api/jobs/${job.id}/files/${encodeURIComponent(name)}`;
