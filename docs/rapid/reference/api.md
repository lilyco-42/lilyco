# HTTP API 与错误

## 基础端点

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/healthz` | 进程存活 |
| GET | `/readyz` | 进程 + 存储就绪 |
| GET | `/openapi.json` | OpenAPI 3.1 文档 |
| GET | `/docs` | 交互文档（GET 可试调用） |

`healthz` 不查数据库；`readyz` 会访问存储。编排系统应分别使用这两个探针。

## 资源端点

对集合 `{collection}`：

| 方法 | 路径 | 语义 |
|---|---|---|
| GET | `/api/{collection}` | 列表，游标分页 |
| POST | `/api/{collection}` | 新建 |
| GET | `/api/{collection}/{id}` | 读取单条 |
| PUT | `/api/{collection}/{id}` | 完整替换 |
| DELETE | `/api/{collection}/{id}` | 删除 |

`PUT` 不是部分更新：缺失字段按请求 schema 报错。需要部分更新时，读取后修改再整体 `PUT`。

## 分页

```json
{"items":[{"id":1}],"next":2}
```

下一页：

```text
GET /api/{collection}?after=2
```

- `after` 是上一页返回的 `next`；
- `next` 为 `null` 表示结束；
- 游标是非负 32 位整数，非法值返回 400。

## 请求

- 写请求必须 `Content-Type: application/json`；
- JSON 必须匹配 `openapi.json` 中的 schema；
- 未声明字段会被拒绝（`deny_unknown_fields` 语义），不会静默忽略；
- 请求体大小受 `[http].max_body_bytes` 限制。

## 错误响应

所有错误返回 JSON，字段在顶层：

```json
{"code":"invalid_field","message":"title is required","field":"title"}
```

| 字段 | 说明 |
|---|---|
| `code` | 稳定机器码 |
| `message` | 英文可读原因 |
| `field` | 出错字段；非字段错误为 `null` |

### 错误码

| HTTP | `code` | 场景 |
|---|---|---|
| 400 | `invalid_id` | 记录 ID 非正整数 |
| 400 | `invalid_query` | 游标或查询参数非法 |
| 400 | `invalid_path` | 路径参数与 schema 不符 |
| 405 | `method_not_allowed` | 方法不在该路径允许列表 |
| 404 | `not_found` | 路径或记录不存在 |
| 413 | `body_too_large` | 超过 `max_body_bytes` |
| 415 | `json_required` | 缺少 JSON Content-Type |
| 422 | `invalid_field` | 业务校验失败（带 `field`） |
| 500 | `storage_failure` | 存储操作失败，看服务端日志 |
| 503 | `storage_busy` | 数据库锁/忙，可重试 |

客户端应按 `code` 分支，而不是匹配 `message` 文本；`message` 可能随版本调整。

## 自定义端点

在 `server/src/app.rs` 注册：

| 方法 | 输入 | 输出 |
|---|---|---|
| `get(path, handler)` | 无 | `T` |
| `get_query(path, handler)` | 查询结构体 | `T` |
| `get_path(path, handler)` | 路径结构体 | `T` |
| `post_json(path, handler)` | JSON 结构体 | `T` |

处理函数返回 `Result<T, ApiError>`。输入输出类型需要 `Deserialize`/`Serialize` 与 `JsonSchema`；缺少 `JsonSchema` 时端点可运行但文档不完整。

## 内容协商

当前只输出 JSON。需要文件下载或流式响应时，在 `server/` 增加自定义 route，不要塞进资源 CRUD。

## 版本与兼容

`/openapi.json` 是客户端契约。删除或改字段属于破坏性变更；新增可选字段应保持旧客户端可用。生成项目升级时，用新旧两版 `/openapi.json` 做 diff。