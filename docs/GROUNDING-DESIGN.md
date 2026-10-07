# lilyco-grounding 设计（vision 眼睛：看屏出坐标）

> 定位：`bitnet-ask` 是嘴（本地推理），`grounding` 是眼（看屏定位），
> 脑是 Qwen3-0.6B，手脚是 `android layout` + `input tap`。MCP 优先、视觉兜底。

## 命令形态（Registry/MCP 同构）

```rust
#[derive(App)]
#[app(name = "grounding", about = "看屏定位 UI 元素", run = "run_grounding")]
pub struct Grounding {
    /// 截图路径（`screen capture` 产物）
    image: PathBuf,
    /// 找什么（如 "Chrome 图标"）
    description: String,
    /// 视觉权重（默认 ~/.lilyco/models/uground/UGround-V1-2B.Q2_K.gguf）
    model: Option<PathBuf>,
    /// 投影权重（默认 …/UGround-V1-2B.mmproj-fp16.gguf）
    mmproj: Option<PathBuf>,
}
```

## 执行（零新增原生依赖，shell 出外部 `llama-mtmd-cli`）

```
llama-mtmd-cli -m {model} --mmproj {mmproj} --image {image} \
  -p "{UGround官方prompt}\nDescription: {description}\nAnswer:" --temp 0 -n 16
```

- prompt 用 UGround 官方模板（`hf models card osunlp/UGround-V1-2B` 有全文），
  输出 `(x,y)` 为 0-1000 系 → 像素 `(x/1000*W, y/1000*H)`，随图尺寸一起返回。
- 缺二进制/缺权重：仿 `model_missing_error` 风格给指引
  （去哪下、放哪、`--model`/`--mmproj` 怎么指定）。

## 校验（core::executor 唯一宿主 + validate_args）

- 参数校验走 `CommandSchema::validate_args`（image 须存在、description 非空）。
- 回归用例（fake CLI stub 回固定 `(605, 788)` + 真机回归）：
  期望点落在 Chrome 框 `[577,1873][750,2068]` 内（已验：真值中心 `[663,1970]`，误差 ~80px）。
- 进度/遥测沿用 `bitnet.tps` 命名习惯：`grounding.ms` + `grounding.xy`。

## 版本与发布

- 新 crate `lilyco-grounding 0.1.0`，改动进 `docs/CODEGRAPH.md`（§1 版本表 + 调用链）。
- 全绿门槛：`cargo fmt --check` + `clippy --workspace --all-targets` + `cargo test --workspace`。
