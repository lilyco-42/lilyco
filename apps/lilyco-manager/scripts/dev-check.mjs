import { existsSync, readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const workflowPath = resolve(root, '../../.github/workflows/lilyco-manager.yml')
const failures = []

function check(condition, message) {
  if (!condition) failures.push(message)
}

function run(args) {
  const result = spawnSync(process.execPath, args, { stdio: 'inherit', cwd: root })
  check(result.status === 0, `command failed: ${process.execPath} ${args.join(' ')}`)
}

for (const file of [
  'README.md',
  'docs/DEVELOPMENT.md',
  'docs/PERFORMANCE.md',
  'docs/MANIFEST.md',
  'docs/TECHNICAL_SPEC.md',
]) {
  check(existsSync(resolve(root, file)), `missing required file: ${file}`)
}
check(existsSync(workflowPath), 'missing manager workflow')

check(existsSync(resolve(root, 'frontend/bun.lock')), 'missing frontend/bun.lock; run bun install in frontend')
if (!existsSync(resolve(root, 'Cargo.lock'))) {
  console.warn('dev-check: Cargo.lock not present yet; cargo will generate it on first build')
}

const packageJson = JSON.parse(readFileSync(resolve(root, 'frontend/package.json'), 'utf8'))
for (const script of ['typecheck', 'build', 'smoke']) {
  check(Boolean(packageJson.scripts?.[script]), `frontend/package.json is missing script: ${script}`)
}

const workflow = readFileSync(workflowPath, 'utf8')
for (const token of ['pull_request:', 'concurrency:', 'macos-latest']) {
  check(workflow.includes(token), `manager workflow is missing ${token}`)
}

run([resolve(root, 'scripts/validate-catalog.mjs')])

if (failures.length) {
  for (const failure of failures) console.error('dev-check:', failure)
  process.exit(1)
}
console.log('dev-check: repository structure, scripts, workflow and catalog are consistent')
