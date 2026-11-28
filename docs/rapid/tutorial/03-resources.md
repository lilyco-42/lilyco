# 第 3 章 · 资源与校验

> **本章你会学到**
> 1. 用 `generate resource` 生成一个资源；
> 2. 分清「生成器支持的字段类型」与「你可以手写的 Rust 类型」；
> 3. 在 `core/resources/` 里补充校验；
> 4. 用 `?after=` 做游标分页，理解未知字段为什么会被拒绝。

---

## 3.1 生成资源

```sh
cd notes
lilyco-rapid generate resource notes \
  --field title:string \
  --field priority:i32 \
  --field archived:bool
```

生成器当前支持三种字段类型：

| `--field` 类型 | Rust 类型 | OpenAPI |
|---|---|---|
| `string` | `String` | `string` |
| `i32` | `i32` | `integer` |
| `bool` | `bool` | `boolean` |

字段数量没有硬限制，但分页大小与文本上限在 `Rapid.toml`：

```toml
[resources]
max_text_bytes = 200
page_size = 40
```

`max_text_bytes` 限制字符串字段长度；超限返回结构化错误。不要把它写死在业务代码里，不同部署环境可能需要不同上限。

---

## 3.2 生成物

新增文件：

```text
core/resources/notes.rs
```

`core/build.rs` 会自动注册 `resources/` 下的模块。新资源出现在：

- `/api/notes`：列表、新建
- `/api/notes/{id}`：读取、替换、删除
- `/openapi.json`：完整 schema
- 工作台：Notes 分组与 API reference

`dev` 会自动重编译，刷新即可用。

---

## 3.3 写校验

打开 `core/resources/notes.rs`，在资源定义旁补充校验：

```rust
impl Note {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.title.trim().is_empty() {
            return Err("title 不能为空");
        }
        if self.title.len() > 200 {
            return Err("title 超过 200 字节");
        }
        Ok(())
    }
}
```

> `core` 用 `lilyco-no_std` 的表达方式，可在无操作系统目标编译。校验逻辑不要依赖文件系统、网络或系统时间；需要这些能力时放到 `server/`。

校验错误的语义应与第 2 章的 `ApiError` 一致：字段名 + 可读原因。

---

## 3.4 强类型输入

Rapid 的写请求要求完整 JSON：

```sh
curl -s -X POST http://127.0.0.1:8421/api/notes \
  -H 'content-type: application/json' \
  -d '{"title":"第一条","priority":1,"archived":false}'
```

故意多传一个字段：

```sh
curl -s -X POST http://127.0.0.1:8421/api/notes \
  -H 'content-type: application/json' \
  -d '{"title":"x","priority":1,"archived":false,"extra":true}'
```

应返回校验错误而不是静默忽略——这是「零信任输入」的直接体现。同理，`"priority":"high"` 会在反序列化阶段失败。

---

## 3.5 游标分页

列表响应：

```json
{"items":[{"id":40,"title":"..."}],"next":41}
```

下一页：

```sh
curl -s 'http://127.0.0.1:8421/api/notes?after=41'
```

`next` 为空表示结束。游标基于自增 ID，不依赖 `offset`，避免翻页时数据漂移。

---

## 本章验收

```sh
curl -s http://127.0.0.1:8421/api/notes
curl -s http://127.0.0.1:8421/openapi.json | grep -q '"/api/notes"'
```

再手动提交一个缺字段请求，确认返回 4xx 且响应体包含顶层 `code` / `message`（字段错误还有 `field`）。

> 下一章：把 SQLite 换成 PostgreSQL，或切换到 Turso / MongoDB / SurrealDB。