# 第 5 章 · 前端工作台

> **本章你会学到**
> 1. 全栈模板默认生成什么；
> 2. 在 React / Vue / Svelte 之间选择；
> 3. 用 `web-next/` 预览新前端而不覆盖现有前端；
> 4. 词条外置与端口调整。

---

## 5.1 全栈项目

```sh
lilyco-rapid new notes-full            # 默认 --preset fullstack --ui react
cd notes-full
lilyco-rapid dev
```

生成物比 `api` 预设多：

```text
notes-full/
├── web/                # Vite 前端
│   ├── package.json
│   ├── pnpm-lock.yaml
│   ├── src/
│   │   ├── App.tsx
│   │   ├── components/Workbench.tsx
│   │   ├── lib/api.ts
│   │   └── strings*.toml
│   └── vite.config.ts
├── ops/Caddyfile
└── compose.yaml        # 全栈：Caddy + API + 数据库
```

默认端口：API `8421`，Vite `5173`。修改 Vite 端口：

```sh
RAPID_WEB_PORT=5174 lilyco-rapid dev
```

全栈 `dev` 会并行执行：数据库启动、Rust 编译、pnpm 安装与 Vite 启动。首次启动较慢，后续走缓存。

---

## 5.2 选择框架

| 选项 | 说明 |
|---|---|
| `--ui react` | 默认；`--ui-kit shadcn` 使用 shadcn 按钮模板 |
| `--ui vue` | Vue + 原生组件 |
| `--ui svelte` | Svelte + 原生组件 |
| `--ui-kit native` | React 原生控件，不引入 shadcn |

```sh
lilyco-rapid new vue-notes --ui vue --db postgres --db-driver seaorm --gateway pingap
```

API 预设（`--preset api`）不生成前端，可以直接被其它框架消费：读取 `/openapi.json` 生成客户端。

---

## 5.3 预览新前端

已有项目换框架时，不要直接覆盖 `web/`：

```sh
# 在 web-next/ 生成新前端
lilyco-rapid generate frontend --ui svelte --dir notes-full

# 用原 API、数据库与记录，预览 web-next
lilyco-rapid dev --preview-ui --dir notes-full
```

确认交互无误后，再把自定义页面迁入新框架，最后用它替换部署用的 `web/`。原 `web/` 始终保留，避免不可逆覆盖。

---

## 5.4 词条与配置外置

前端可见文案在 `web/src/strings*.toml`，按浏览器语言选择。新增界面时必须同步简体中文与英语词条，并保持插值参数一致。

后端可见错误也应是可本地化的键，而不是直接拼接的自然语言。前端只做展示与基础消毒，业务校验仍以后端为准。

---

## 5.5 后端零 UI

`server/` 不渲染页面、不拼接 HTML。工作台的页面、路由与组件全部在 `web/`。需要新增页面时改前端，并消费已经存在的强类型 API；接口不够就先在 `server/src/app.rs` 增加端点与 schema。

---

## 本章验收

```sh
test -f notes-full/web/package.json
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:5173   # 200（dev 运行时）
test -f notes-full/web/src/strings.toml
```

再做一次框架预览：

```sh
lilyco-rapid generate frontend --ui vue --dir notes-full
test -d notes-full/web-next
```

> 下一章：把项目部署到 Compose，或换成 Kiri 运行时。