export interface PropSchema {
  type?: string
  description?: string
  default?: string | number | boolean
  enum?: string[]
  minimum?: number
  maximum?: number
}
export interface ToolSchema {
  name: string
  description?: string
  inputSchema?: {
    type: string
    required?: string[]
    properties?: Record<string, PropSchema>
  }
}
export interface RunRecord {
  id: string
  name: string
  at: string
  ok: boolean
  preview: string | null
}
export interface RunResult {
  run: RunRecord
  result: {
    content: Array<{type: string, text?: string}>
    isError?: boolean
  }
}
