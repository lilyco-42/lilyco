# lilyco-web2cli (`lweb2cli`)

**Convert any webpage, HTML form, OpenAPI spec, or HAR capture into a Lilyco four-interface app (CLI / TUI / Web / MCP).**

## Capabilities

1. **Zero-Config Universal Webpage CLI**:
   - `lweb2cli open --url https://news.ycombinator.com` — distill any HTML/JSON page into a compact, numbered CLI view (`[1]`, `[2]`, ...)
   - `lweb2cli do --index 1` — follow numbered link `[1]` in the persistent session
   - `lweb2cli find --query rust` / `lweb2cli read --index 1` / `lweb2cli raw --url <url>`
   - `--render` — optional headless Chrome/Chromium/Edge DOM rendering for JS/SPA pages
   - Persistent file-backed Cookie & session jar (`lweb2cli session --op list|clear|seed-cookie`)

2. **Automatic Discovery -> `WebCliSpec` -> Lilyco `Registry`**:
   - `lweb2cli inspect --url https://example.com --save site.webcli.json`
   - Discovers `<form>` controls (`input`, `select`, `textarea`, `checkbox`, `hidden`, `<label>`), static JS API endpoints (`fetch`/`axios`), OpenAPI 3.x / Swagger 2.0 specs, and Chrome DevTools HAR traces (`lweb2cli from-har --file trace.har`).
   - Compiles `WebCliSpec` into a `lilyco_core::Registry`, giving **any website** CLI, TUI, Web UI, and MCP Server surfaces!

3. **Live Serve & Standalone Codegen**:
   - `lweb2cli serve https://news.ycombinator.com --schema`
   - `lweb2cli serve https://news.ycombinator.com form-1 --q rust --json`
   - `lweb2cli serve site.webcli.json --mcp` (or `--tui` / `--gui` with `--features full`)
   - `lweb2cli codegen --source site.webcli.json --out ./my-site-cli` — generate a standalone Rust `lilyco` crate

4. **政务填表 / 企业设立登记闭环（资料分类 + Cookie 导入 + AI 预填 + 人类审核闸门）**:
   - **导入登录态 Cookies**（支持 EditThisCookie/Playwright JSON、Netscape `cookies.txt`、原始 `Cookie:` 请求头）：
     `lweb2cli import-cookies --file cookies.json --session gov`
   - **分类资料库与附件管理**（`company` / `legal_rep` / `finance` / `contact` + 证照文件）：
     `lweb2cli vault --op set --profile xinghuo --category company --key name --value "星火科技有限公司"`
     `lweb2cli vault --op attach --profile xinghuo --category 证照 --key business_license --label "营业执照" --file ./license.pdf`
   - **AI 自动抓取表单并生成待审核草稿（绝不直接提交）**：
     `lweb2cli prepare-submit --source https://zwfw.gov.example/register --profile xinghuo --session gov`
   - **人类逐项核对、补齐并批准**：
     `lweb2cli review --op show --id <draft_id>`
     `lweb2cli review --op patch --id <draft_id> --field applymemo --value "新设有限责任公司"`
     `lweb2cli review --op approve --id <draft_id> --note "已核对无误"`
   - **受控最终提交（含 `multipart/form-data` 材料上传、回执落盘与自动模板沉淀）**：
     `lweb2cli commit-submit --id <draft_id>`

5. **CDP 实时浏览器控制 + 多步骤向导 + 模板沉淀（对标 `agent-browser` / `ego-lite` / `stagehand` / `skyvern`）**:
   - **附着已登录 Chrome 并穿透多层 `iframe` 快照**：
     `lweb2cli cdp --endpoint http://127.0.0.1:9222 --op snapshot`
     `lweb2cli cdp --endpoint http://127.0.0.1:9222 --op sync-cookies --session gov`
   - **多步骤向导填报（`@vault:` 资料库自动取值 + `HumanReviewGate` 自动截图暂停）**：
     `lweb2cli wizard --plan gov-apply.json --profile xinghuo`
     `lweb2cli wizard --plan gov-apply.json --profile xinghuo --start-step 3 --approve-gate`
   - **查看已沉淀的确定性填报模板（下次给新企业填同站点零猜测复放）**：
     `lweb2cli recipe --op list`
