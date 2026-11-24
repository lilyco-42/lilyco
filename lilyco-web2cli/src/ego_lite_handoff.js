const { createHash } = await import('node:crypto')
const { writeFile } = await import('node:fs/promises')

const target = new URL(process.env.LWEB2CLI_EGO_URL)
const digest = createHash('sha256').update(target.href).digest('hex').slice(0, 16)
const name = 'Lilyco Web2CLI ' + target.host.slice(0, 40) + ' ' + digest
const delegated = 'agentDelegatedToUser'

const matches = (await taskSpaces.list()).filter(space => space.name === name)
if (matches.length > 1) throw new Error('找到多个同名 ego-lite 任务，请保留一个后重试')
const existing = matches[0]

function waiting(taskId, reason) {
  return { status: 'waiting_for_human', task_id: taskId, reason }
}

async function handOff(taskId) {
  const result = await taskSpaces.handOff(taskId)
  if (!result.done) throw new Error('ego-lite 未能把任务交还给操作者')
}

async function start() {
  if (existing?.ownership === delegated) {
    return waiting(existing.id, '请在 ego-lite 操作页面，完成后运行 --ego-lite resume')
  }
  if (existing) throw new Error('同一 URL 已有任务在运行或由他人控制')

  const task = await taskSpaces.new(name)
  try {
    await browser.openOrReuseTab(target.href, { match: 'exact', wait: false })
  } finally {
    await handOff(task.id)
  }
  return waiting(task.id, '请在 ego-lite 操作页面，完成后运行 --ego-lite resume')
}

async function readPage(taskId) {
  const tabs = (await browser.listTabs({ includeChrome: false })).filter(tab => {
    try {
      return new URL(tab.url).origin === target.origin
    } catch {
      return false
    }
  })
  if (tabs.length !== 1) {
    return waiting(taskId, '请让此任务空间只保留一个目标站点页签')
  }
  await browser.switchTab(tabs[0].targetId)
  const ready = await page.waitForFunction(() => document.readyState === 'complete', {
    timeout: 8000,
  })
  if (!ready) return waiting(taskId, '目标页面仍在加载')

  const result = await page.evaluate((expectedOrigin) => {
    if (location.origin !== expectedOrigin) return { reason: '页面已离开目标站点' }
    if (/^(Just a moment[.!…]*|Attention Required! \| Cloudflare)$/i.test(document.title.trim())
        || document.querySelector('#challenge-form, #cf-challenge-running, #cf-error-details')) {
      return { reason: '页面仍在显示验证或拦截提示，请先手动处理' }
    }
    const status = performance.getEntriesByType('navigation')[0]?.responseStatus ?? 0
    if (status < 200 || status >= 300) {
      return { reason: status ? '页面返回 HTTP ' + status : '浏览器未提供主文档 HTTP 状态' }
    }
    if (!['text/html', 'application/xhtml+xml'].includes(document.contentType)) {
      return { reason: '当前页不是可发现命令的 HTML 文档' }
    }
    const root = document.documentElement.cloneNode(true)
    for (const hidden of root.querySelectorAll('input[type="hidden" i]')) hidden.remove()
    for (const input of root.querySelectorAll('input[value]')) input.removeAttribute('value')
    const url = new URL(location.href)
    url.search = ''
    url.hash = ''
    return { url: url.href, html: root.outerHTML }
  }, target.origin)

  if (result.reason) return waiting(taskId, result.reason)
  return { status: 'page', url: result.url, html: result.html }
}

async function resume() {
  if (existing?.ownership !== delegated) {
    throw new Error('没有相应的人工交接任务，请先对同一 URL 运行 --ego-lite handoff')
  }
  // 此处只在操作者显式选择 resume 后执行。读取结束后将页签交还。
  await taskSpaces.takeOver(existing.id)
  try {
    return await readPage(existing.id)
  } finally {
    await handOff(existing.id)
  }
}

const result = process.env.LWEB2CLI_EGO_MODE === 'handoff' ? await start() : await resume()
await writeFile(process.env.LWEB2CLI_EGO_OUT, JSON.stringify(result))
