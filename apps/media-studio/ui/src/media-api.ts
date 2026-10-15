export type MediaTool = 'compress' | 'convert' | 'resize' | 'trim';
export type MediaParams = Record<string, number | string>;
export interface MediaMetadata {
  width: number; height: number; duration: number; codec: string; bytes: number;
}
export interface MediaSource {
  id: string; name: string; media: MediaMetadata; preview: string;
}
export interface MediaJob {
  id: string; session: string; tool: MediaTool;
  state: 'queued' | 'running' | 'done' | 'failed' | 'cancelled';
  files: {name: string; size: number}[]; error: string | null;
  progress: {progress?: number; total?: number; message?: string} | null;
  result: {media?: MediaMetadata; [key: string]: unknown} | null;
}
export async function mediaRequest<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch('/api/' + path, init);
  const data = await response.json();
  if (!response.ok) throw new Error(typeof data.detail === 'string' ? data.detail : '请求未完成，请检查输入后重试');
  return data as T;
}
export function mediaFileUrl(job: MediaJob, name: string) {
  return `/api/jobs/${job.id}/files/${encodeURIComponent(name)}`;
}
export function mediaSize(bytes: number) { return `${(bytes / 1024 / 1024).toFixed(1)} MB`; }
export function mediaTime(seconds: number) {
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${Math.floor(seconds % 60).toString().padStart(2, '0')}`;
}
