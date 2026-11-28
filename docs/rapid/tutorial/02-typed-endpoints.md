# 第 2 章 · 类型化端点

> **本章你会学到**
> 1. 在 `server/src/app.rs` 注册一个自定义端点；
> 2. 让请求、响应和 OpenAPI 由同一份 Rust 类型驱动；
> 3. 用 `ApiError` 返回结构化错误；
> 4. 理解查询参数、路径参数与 JSON 请求体的注册方法。

---

## 2.1 先看模板

打开 `notes/server/src/app.rs`。生成项目已经包含一个可调用的例子：

```rust
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct Greeting {
    pub name: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Reply {
    pub message: String,
}

async fn greet(input: Greeting) -> Result<Reply, crate::ApiError> {
    Ok(Reply {
        message: format!("Hello, {}!", input.name),
    })
}

pub fn register(api: &mut crate::Api) -> Result<(), crate::StartupError> {
    api.get_query("/api/hello", greet)
}
```

两个 derive 各有职责：

| derive | 作用 |
|---|---|
| `Deserialize` / `Serialize` | HTTP 解析与输出 |
| `JsonSchema` | 生成 `/openapi.json` 的请求/响应 schema |

删掉 `JsonSchema` 会怎样？端点和 JSON 仍然能跑，但文档里该端点的 schema 丢失。Rapid 的原则是**文档不是手写的**：类型缺失，契约就缺失。

---

## 2.2 注册方法

`Api` 提供按输入形状划分的方法：

| 方法 | 输入 | 示例 |
|---|---|---|
| `get` | 无输入 | 健康检查、静态信息 |
| `get_query` | 查询参数结构体 | `/api/hello?name=Lilyco` |
| `get_path` | 路径参数结构体 | `/api/notes/{id}` |
| `post_json` | JSON 请求体 | 创建资源 |

处理函数统一返回 `Result<T, ApiError>`，`T` 必须实现 `Serialize + JsonSchema`。

---

## 2.3 结构化错误

不要返回 `String` 或 `anyhow::Error` 当作 HTTP 响应。`ApiError` 已实现统一转换，客户端拿到的是稳定形状：

```json
{"code":"invalid_field","message":"title is required","field":"title"}
```

错误体是顶层对象；`field` 只在字段校验失败时出现，其它错误为 `null`。

写业务校验时优先在 `ApiError` 里带上字段名，前端才能把错误定位到具体输入框。第 3 章的资源校验也是同一套错误通道。

---

## 2.4 类型即文档

启动 `dev` 后：

```sh
curl -s http://127.0.0.1:8421/openapi.json | jq '.paths["/api/hello"]'
```

把 `Greeting` 的 `name` 改成 `nickname`，保存，`dev` 会重编译。刷新后再看 `/openapi.json`，请求 schema 应同步变成 `nickname`，无需改任何 YAML。

如果文档没变，检查两件事：

1. 改的是 `server/src/app.rs`，不是 `target/` 里的生成物；
2. `dev` 是否已完成重编译（终端出现新的 `API listening`）。

---

## 2.5 在交互文档里试调用

打开 `http://127.0.0.1:8421/docs`（全栈项目也可从工作台的 API reference 进入），找到 `/api/hello`，输入参数发送。响应与 `curl` 一致。

> 文档页只暴露 GET 试调用；写操作留在业务面板里，避免文档页变成一键写入口。

---

## 本章验收

```sh
curl -s 'http://127.0.0.1:8421/api/hello?name=Lilyco'
# {"message":"Hello, Lilyco!"}

curl -s http://127.0.0.1:8421/openapi.json | grep -q 'Greeting'
```

两条都通过再进入 [第 3 章](03-resources.md)。

> 下一章：用 `generate resource` 一次生成 CRUD，再把校验写进 `core/resources/`。