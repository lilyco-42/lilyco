export type BlockKind = 'hero' | 'stats' | 'app-card' | 'logs' | 'form' | 'empty'
export type Tone = 'neutral' | 'brand' | 'warning' | 'danger'
export type BlockValue = string | boolean
export type BlockSpec = { kind: BlockKind; props: Record<string, BlockValue> }
export type Field = {
  key: string
  label: string
  type: 'text' | 'textarea' | 'select' | 'toggle'
  options?: string[]
}
export type BlockDefinition = {
  kind: BlockKind
  name: string
  description: string
  category: string
  fields: Field[]
  defaults: Record<string, BlockValue>
}

const tone: Field = { key: 'tone', label: '色调', type: 'select', options: ['neutral', 'brand', 'warning', 'danger'] }

export const blocks: BlockDefinition[] = [
  {
    kind: 'hero',
    name: '页头',
    description: '标题、说明和一个状态徽标',
    category: '布局',
    fields: [
      { key: 'title', label: '标题', type: 'text' },
      { key: 'description', label: '说明', type: 'textarea' },
      { key: 'badge', label: '徽标', type: 'text' },
      tone,
    ],
    defaults: { title: '本地优先的工具工作台', description: '只在回环地址运行，所有状态都来自真实进程。', badge: 'LOCAL ONLY', tone: 'brand' },
  },
  {
    kind: 'stats',
    name: '指标卡',
    description: '单个可读指标与注释，不编造数值',
    category: '数据',
    fields: [
      { key: 'label', label: '指标名', type: 'text' },
      { key: 'value', label: '数值', type: 'text' },
      { key: 'hint', label: '注释', type: 'text' },
      tone,
    ],
    defaults: { label: '已注册工具', value: '2', hint: '来自 /api/tools', tone: 'neutral' },
  },
  {
    kind: 'app-card',
    name: '应用卡',
    description: '应用名、版本、运行状态与操作按钮',
    category: '业务',
    fields: [
      { key: 'name', label: '应用名', type: 'text' },
      { key: 'version', label: '版本', type: 'text' },
      { key: 'description', label: '说明', type: 'textarea' },
      { key: 'running', label: '运行中', type: 'toggle' },
      tone,
    ],
    defaults: { name: 'lilyco-manager', version: 'v0.3.2', description: '生命周期管理、日志与 MCP 聚合。', running: true, tone: 'neutral' },
  },
  {
    kind: 'logs',
    name: '日志面板',
    description: '有界滚动日志，等宽字体',
    category: '反馈',
    fields: [
      { key: 'title', label: '标题', type: 'text' },
      { key: 'lines', label: '日志行', type: 'textarea' },
      tone,
    ],
    defaults: { title: '最近输出', lines: '[info] manager listening on 127.0.0.1\n[info] 2 tools registered\n[warn] demo app not installed', tone: 'neutral' },
  },
  {
    kind: 'form',
    name: '表单块',
    description: '标签、输入框与主操作',
    category: '交互',
    fields: [
      { key: 'title', label: '标题', type: 'text' },
      { key: 'label', label: '字段标签', type: 'text' },
      { key: 'placeholder', label: '占位文本', type: 'text' },
      { key: 'button', label: '按钮文本', type: 'text' },
      tone,
    ],
    defaults: { title: '过滤工具', label: '关键词', placeholder: '输入工具名或描述', button: '应用过滤', tone: 'brand' },
  },
  {
    kind: 'empty',
    name: '空状态',
    description: '空列表说明与下一步动作',
    category: '反馈',
    fields: [
      { key: 'title', label: '标题', type: 'text' },
      { key: 'description', label: '说明', type: 'textarea' },
      { key: 'action', label: '动作文本', type: 'text' },
      tone,
    ],
    defaults: { title: '还没有应用', description: '把 lilyco.app.json 放进仓库根目录后刷新。', action: '重新扫描', tone: 'warning' },
  },
]

export function definitionOf(kind: BlockKind) {
  return blocks.find(block => block.kind === kind)!
}

export function createSpec(kind: BlockKind): BlockSpec {
  return { kind, props: { ...definitionOf(kind).defaults } }
}

export function validateSpec(input: unknown): { spec?: BlockSpec; error?: string } {
  if (!input || typeof input !== 'object' || Array.isArray(input)) return { error: '顶层必须是对象，形如 {"kind":"hero","props":{}}' }
  const value = input as { kind?: unknown; props?: unknown }
  const definition = blocks.find(block => block.kind === value.kind)
  if (!definition) return { error: `未知积木类型：${String(value.kind)}` }
  if (!value.props || typeof value.props !== 'object' || Array.isArray(value.props)) return { error: 'props 必须是对象' }
  const props: Record<string, BlockValue> = { ...definition.defaults }
  for (const field of definition.fields) {
    const raw = (value.props as Record<string, unknown>)[field.key]
    if (raw === undefined) continue
    if (field.type === 'toggle') {
      if (typeof raw !== 'boolean') return { error: `${field.label} 必须是布尔值` }
      props[field.key] = raw
    } else {
      if (typeof raw !== 'string') return { error: `${field.label} 必须是字符串` }
      if (field.type === 'select' && field.options && !field.options.includes(raw)) {
        return { error: `${field.label} 只能是 ${field.options.join(' / ')}` }
      }
      props[field.key] = raw
    }
  }
  return { spec: { kind: definition.kind, props } }
}
