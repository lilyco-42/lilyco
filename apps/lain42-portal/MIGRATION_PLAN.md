# Lain42 商业级 React + shadcn/ui 重制与全功能移植路线图（7 天落地）

## 架构决策（严格遵循 `/dev` 六大铁律）
- **前后端硬隔离**：
  - **后端（Rust / `apps/lain42-portal`）**：零 UI 拼接，只提供强类型 JSON API（`/api/*` + `/studio/api/*`）与 Turso SQLite 持久化 + 双钱包硬隔离（充值积分 vs 赠送积分）。
  - **前端（`apps/lain42-portal/web/`）**：**React 19 + TypeScript + Vite + Tailwind CSS v4 + shadcn/ui（Radix UI + Lucide Icons + Recharts）**，对标 `new-api` 商业级控制台交互审美（左侧可折叠导航 + 顶部 Command/余额状态栏 + 卡片式仪表盘 + 深浅主题切换 + 全响应式四视口）。
- **平滑灰度切换**：
  - 老站 `https://lain42.top` 顶部已上线「抢先进入新版 `dev.lain42.top` / 留在经典版 `lain42.top`」双入口切换条，用户零感知平滑过渡。
  - `dev.lain42.top` 完整移植并验收通过后，再将主域名流量无停机切至新版。

---

## 全功能移植核对清单（`lain42.top` → `dev.lain42.top`）

| 模块 | 老站 `lain42.top` 功能 | `dev.lain42.top` 后端 API | 新版 React + shadcn/ui 页面 | 状态 |
| :--- | :--- | :--- | :--- | :--- |
| **首页 & 公告** | 节点提速公告、双入口切换、核心入口卡片 | `GET /api/announcements` `POST /api/announcements` | `HomePage`（Hero + 实时节点吞吐图表 + 公告栏） | 后端已就绪，UI 升级中 |
| **代理订阅与线路** | 15元(100GB)/30元(220GB)/季/年卡、Clash/Karing 订阅、实测吞吐展示、使用教程 | `GET /studio/api/sub/:token/:fmt` `GET /api/proxy/quality` | `VpnPage`（套餐对比表 + 节点测速卡片 + 全平台向导） | 需补 `/api/proxy/quality` 透传 |
| **数字商品商城 & 支付** | 充值积分（爱发电/支付宝/USDT/Stripe/Creem）、购买代理/软件、卡密兑换、双钱包隔离 | `POST /studio/api/points/purchase` `POST /studio/api/redeem` 支付回调 | `ShopPage`（Tab 分类 + 强类型收银台 Dialog + 实时订单轮询） | 后端已就绪，UI 升级中 |
| **AI API 模型与控制台** | 模型列表、实时价格、可用状态、API Key 签发、OpenAI/Anthropic 兼容文档 | `GET /api/models` `/relay/v1/*` `/relay/*` | `ModelsPage` + `DocsPage`（带代码高亮与一键复制） | 后端已就绪，UI 升级中 |
| **下载中心（538 项）** | 126 云枢自研 + 412 高星开源，六大分类、搜索、夸克免费直链、SEO 详情页 | `GET /api/downloads` `/go/download/:slug` `/sitemap.xml` | `DownloadsPage`（虚拟列表/分页 + 分类筛选 + 详情抽屉） | 后端已就绪，UI 升级中 |
| **我的账户 & 售后工单** | 登录/注册、双钱包余额、我的订阅/授权码、订单流水、售后工单 | `GET /studio/api/me` `GET /studio/api/account/orders` `POST /studio/api/contact` | `AccountPage`（Dashboard 统计卡 + 订阅一键复制 + 工单表单） | 需补 `POST /studio/api/contact` |
| **WASM 本地工具箱** | JSON/Base64/SHA-256/时间戳/Token 估算 | 纯本地浏览器运行 | `ToolsPage`（shadcn Tabs + CodeMirror/Textarea） | UI 升级中 |

---

## 7 天落地排期
- **Day 1（今日）**：
  1. 老站 `lain42.top` 上线「进入新版 `dev.lain42.top` / 留在经典版」入口切换条；
  2. 补齐 `apps/lain42-portal` 剩余缺失 API（`/studio/api/contact` 工单、`/studio/api/proxy/quality` 实时测速接口）；
  3. 初始化 `apps/lain42-portal/web/`（Vite + React + TypeScript + Tailwind + shadcn/ui 商业组件库骨架）并与 Rust Axum `--static-dir` 打通 SPA + SSR SEO 混合路由。
- **Day 2–3**：完成核心 4 大商业级 Console 页面（`Dashboard/Home`、`Shop & Checkout`、`VPN & Speed Monitor`、`Account & Subscriptions`）。
- **Day 4–5**：完成 `Models & AI Docs`、`Downloads (538 items)`、`Tools` 与深/浅主题、中英文切换、移动端 Drawer 导航。
- **Day 6**：四视口（390px / 768px / 1280px / 1440px）Chromium 实测、支付/订阅/双钱包端到端回归测试。
- **Day 7**：全量数据核对后，开放主域名 `lain42.top` 平滑切换。
