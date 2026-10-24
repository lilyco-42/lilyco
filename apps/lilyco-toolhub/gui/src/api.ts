import type { ToolSchema } from './types'

const match = /(?:^#|&)token=([^&]+)/.exec(window.location.hash)
const token = match ? decodeURIComponent(match[1]) : sessionStorage.getItem('lilyco-gui-token') || ''
if (token) {
  sessionStorage.setItem('lilyco-gui-token', token)
  history.replaceState(null, '', window.location.pathname)
}

export async function api<T>(path: string, payload?: unknown): Promise<T> {
  const method = payload === undefined ? 'GET' : 'POST'
  const response = await fetch('/api/' + path, {
    method,
    headers: {
      'X-Lilyco-Token': token,
      ...(payload === undefined ? {} : {'Content-Type': 'application/json'}),
    },
    ...(payload === undefined ? {} : {body: JSON.stringify(payload)}),
  })
  const body = await response.json()
  if (!response.ok) throw new Error(body.error || '本机请求失败')
  return body as T
}

export async function loadPreview(id: string): Promise<string> {
  const response = await fetch('/api/preview/' + encodeURIComponent(id), {
    headers: {'X-Lilyco-Token': token},
  })
  if (!response.ok) throw new Error('文件预览不可用')
  return URL.createObjectURL(await response.blob())
}

export function defaults(tool: ToolSchema): Record<string, string | number | boolean | null> {
  return Object.fromEntries(Object.entries(tool.inputSchema?.properties || {}).map(([key, prop]) => [
    key, prop.default ?? (
      prop.type === 'boolean' ? false :
      prop.enum || prop.type === 'number' || prop.type === 'integer' ? null : ''
    )
  ]))
}
