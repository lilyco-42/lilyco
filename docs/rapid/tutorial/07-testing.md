# 第 7 章 · 测试与 CI

> **本章你会学到**
> 1. 主 workspace 的 HTTP 测试覆盖什么；
> 2. 用 `/openapi.json` 做契约断言；
> 3. 在主 workspace 里跑 Rapid 的测试；
> 4. 把格式、Clippy、测试放进 CI。

---

## 7.1 框架测试

`crates/rapid-server/tests/http.rs` 覆盖：

- 健康检查与就绪检查；
- 列表、创建、读取、替换、删除；
- 缺失字段与类型错误；
- 分页游标；
- 存储重启后的数据持久化。

在 Lilyco 仓库根目录运行：

```sh
cargo test -p rapid-server
```

生成项目本身不包含框架测试；你可以在 `server/tests/` 下按同样方式添加业务测试。测试使用临时 SQLite，不碰项目的 `data/rapid.sqlite3`。

---

## 7.2 契约测试

新增端点后，至少断言两件事：

```rust
#[tokio::test]
async fn hello_is_documented() {
    let app = app_server::router(/* 测试配置 */).await.unwrap();
    let response = app
        .oneshot(Request::get("/openapi.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let spec: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
            .unwrap();
    assert!(spec["paths"]["/api/hello"]["get"].is_object());
}
```

原则：

- **断言 schema，不只断言状态码**：端点存在但文档缺失，前端仍然不可用。
- **断言错误形状**：4xx 响应必须能解析出结构化 `error`，而不是 HTML 或空体。
- 测试失败时先修实现，不要削弱断言。

---

## 7.3 主 workspace 验证

在 Lilyco 仓库根目录：

```sh
cargo test -p rapid-core -p rapid-server -p lilyco-rapid
cargo test -p lilyco-no_std --features std
cargo fmt --all -- --check
cargo clippy -p rapid-core -p rapid-server -p lilyco-rapid -p lilyco-no_std --all-targets
```

`lilyco-rapid` 的测试会实际生成项目并运行 `cargo metadata`，验证模板资源路径在任意目录下都成立。修改 `templates/` 后必须跑它。

---

## 7.4 CI

主仓库的 `.github/workflows/ci.yml` 执行：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo test -p lilyco --features full
```

Rapid 的四个 crate 已纳入 workspace，因此上面的命令自然覆盖它们。

生成的独立项目可在自己的 CI 里复用同样命令；数据库相关测试需要 Compose 服务时，用 `docker compose up -d db` 起依赖，测试结束再 `down -v`。

---

## 7.5 发布前最小清单

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace
lilyco-rapid new smoke --preset api --dir /tmp/rapid-smoke
cd /tmp/rapid-smoke/smoke && cargo metadata --locked --no-deps >/dev/null
```

最后一条验证「CLI 生成的产物在仓库外也能锁定依赖并解析 workspace」。

---

## 本章验收

```sh
cd notes && cargo test -p rapid-server
cd <lilyco 仓库根>
cargo test -p rapid-core -p rapid-server -p lilyco-rapid
```

两条退出码均为 0。

> 下一章：处理端口、pnpm、容器与连接失败。