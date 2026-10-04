# laic — Alice in Cradle 全格式逆向

PixelLiner 引擎游戏资源（角色表 / 贴图 / 立绘换装容器）的**完全逆向 + 写回**，
内含 UnityFS 容器解析与 Unity Texture2D 全格式解码。四端同一 registry：CLI / TUI / Web / MCP。

![laic render — PixelLiner 姿势帧合成](https://raw.githubusercontent.com/lilyco-42/lilyco/laic/docs/laic/hero.png)

> `laic render` 的输出：`%POSE_SECTION%` 的图层变换 + `%PACK_SECTION%` 的 UV +
> UnityFS 里的 Texture2D 三者合成一张 PNG。从左起：站立 / 行走起手 / 行走中段 / 跳跃。

## 安装

```bash
git clone https://github.com/lilyco-42/lilyco && cd lilyco
git checkout laic            # laic 分支合进 main 之前先切过来
cargo install --path lilyco-aic
```

数据源是游戏目录下的 `AliceInCradle_Data/StreamingAssets`。10 条命令：
**8 条 T0 只读，`repack` / `pack` 两条 T1**（默认 dry-run，只报计划不写盘；MCP 自动化面默认拒绝）。

## 效果一览

**动画** —— `laic render --pose walk --anim`（`crf60` 帧时长驱动，12 帧循环）：

![laic render --anim](https://raw.githubusercontent.com/lilyco-42/lilyco/laic/docs/laic/walk.gif)

**贴图** —— `laic tex` / `sprites --atlas` 解出的 4096×4096 主贴图（DXT5/BC3，`.resS` 资源流）：

![noel atlas 0](https://raw.githubusercontent.com/lilyco-42/lilyco/laic/docs/laic/atlas.png)

**sprite** —— `laic sprites` 按 UV 裁出的其中 48 个（noel 双图集共 3010 个，以姿势层名命名）：

![laic sprites](https://raw.githubusercontent.com/lilyco-42/lilyco/laic/docs/laic/sprites.png)

**PARTS 变体** —— `render --parts`：同一帧改用第二条图集 PARTS（texture_1）的像素合成，
几何不变、只换像素：

![render --parts](https://raw.githubusercontent.com/lilyco-42/lilyco/laic/docs/laic/parts.png)

**改完装回去** —— `sprites` 裁 → 改 PNG → `pack --replace` 重排图集 → `.pxl`，
再渲染验证：只有被替换的那一个 sprite 变了，其余逐像素不变：

![pack --replace](https://raw.githubusercontent.com/lilyco-42/lilyco/laic/docs/laic/replace.png)

## 四条管线

```bash
# ① 看：谁有这个姿势、每帧每层的变换是什么（mod 抄姿势的数据源）
laic find-pose .../StreamingAssets --name "gun*" --json
laic frame .../Enemies/honeycomb.pxls.dat --pose gun --json > gun.json

# ② 出：裁 sprite 做素材，渲染确认长相对
laic sprites .../PxlNoel/noel.pxls.dat --out ./sp --pose 'big*'
laic render  .../PxlNoel/noel.pxls.dat --pose 'walk' --out ./r --anim --sheet

# ③ 动：这段动画一个循环多少 tick、从哪一帧回绕
laic animate .../PxlNoel/noel.pxls.dat --pose walk --json
laic animate .../PxlNoel/noel.pxls.dat --pose walk --ticks 130 --out ./r

# ④ 写：先看计划，--apply 才落盘（MCP 面直接拒绝，只能人敲）
laic repack .../PxlNoel/noel.pxls.dat --out ./out --rename gun=gun_new --json
laic pack   .../PxlNoel/noel.pxls.dat --out ./pxl --replace ./sp_edits --apply --json
```

输出默认 pretty 模式**只打印 <500 字节的结果**，大结果一律加 `--json`。
`laic --schema` 看机器契约；`--tui` / `--gui` / `--mcp` 进另外三端。

> ⚠️ `pack --replace` 的替换图要按 `sprites` 的**导出名**放：一个表里可能有几百个 sprite 的
> 层名都叫 `Layer`（noel 表里是 **618 个**：导出时去重成 1 个 `Layer.png` + 617 个 `Layer.<img-key>.png`）。
> 认领不到的文件会报在 `unmatched_replacements` 里，不会静默丢。

## 为什么不用 pixelliner4j / AICPxlsUnpacker

| | pixelliner4j (Java) | AICPxlsUnpacker (Py) | laic |
|---|---|---|---|
| 贴图来源 | 要外部 PNG | 要现成 `texture_0.png` | **UnityFS + SerializedFile v22 + Texture2D 全解码** |
| 解析 pxls 六节 | 结构化 | 字节模式硬找 | 结构化 + 128 表逐字节回环回归 |
| 合成一帧 | Java2D（只在其 GUI 里） | ✗ | PNG / GIF / 精灵表落盘 |
| 播放头状态机 | `FrameAnimator` | ✗ | `animate` |
| 图集重排 → `.pxl` | `PxlsKiller` | ✗ | `pack`（占用率 92% vs 20%） |
| 写回 UnityFS | ✗ | ✗ | `repack` |
| 双图集 NORMAL + PARTS | 压成一张 | ✗ | 继承语义保留，`--parts` 可预览 |
| 保留 IMGS / IMGV / PTCL | 只写 PACK + POSE | ✗ | 全节保留 |

## 覆盖面（游戏 StreamingAssets 实测）

- **128/128 张 pxls 表**零错误解析；`parse → serialize` 全表逐字节回环
- **125 个 texture 包 / 133 张 Texture2D**，122 张解码出 PNG
  （DXT5/BC3 71、BC7 39、DXT5Crunched 11、RGBA32 / RGB565 / RGB24 各若干）
- Unity `TextureFormat` 全表内建（**12 = DXT5/BC3，不是 DXT1**），每项用 `m_CompleteImageSize`
  交叉验证（`format_size_ok`），格式认错立刻现形
- 做不到的照实说：`.cmd` 事件脚本未覆盖；IMGV 矢量只解析不绘制；MPCC 深层调色板只报剩余字节数

完整用法、逆向笔记与实测数据：见仓库 [`docs/laic.md`](https://github.com/lilyco-42/lilyco/blob/laic/docs/laic.md)。

## License

MIT OR Apache-2.0

> 示例图渲染自游戏 *Alice in Cradle* 的资源文件，仅用于演示本工具输出效果，
> 素材版权归原作者所有；本仓库不随附任何游戏资源。
