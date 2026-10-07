## Installation

### From crates.io (published)

```toml
[dependencies]
lilyco = "0.2"            # 推荐：一个依赖搞定四端
# 默认 headless（CLI+MCP）；要 TUI/Web：lilyco = { version = "0.2", features = ["tui", "web"] }
lilyco-core = "0.2"       # derive(App) 宏展开需要
```

> **只写大版本。** `"0.2"` 是 caret 约束，会自动取到最新的 `0.2.x`；
> 精确版本号以页首那排 crates.io 徽章为准 —— 徽章是自动更新的，**不要在文档里写死 patch 号**，写死必然过期。
> 仓库 `main` 一般领先于已发布版本（workspace 里的版本号高于 crates.io 上的最新发布），
> 想用最新代码请走下面的「From git」。

### From git

```toml
[dependencies]
lilyco = { git = "https://github.com/lilyco-42/lilyco", rev = "9f04615905cb66a15cc9bd0859ad1e8f3cb2c7b3" } # git 依赖锁 rev（守则 2）
lilyco-core = { git = "https://github.com/lilyco-42/lilyco" }
```

---

