## Examples

### Image Compressor (`lilyco-example`)

```bash
cd lilyco-example
cargo run -- --input photo.jpg --quality 50 --format webp
cargo run -- --input photo.jpg --dry-run --json
cargo run -- --schema
```

See `lilyco-example/src/main.rs` for the full source (~230 lines).

### Grep (`lilyco-grep`)

Simple recursive grep — the DSH ecosystem test vehicle:

```bash
cargo run -p lilyco-grep -- --pattern hello --path src
cargo run -p lilyco-grep -- --pattern TODO --path . --ignore-case --count
cargo run -p lilyco-grep -- --pattern hello --path src --json-stream   # AI 消费
cargo run -p lilyco-grep -- --pattern hello --path src --mcp           # MCP 服务器
```

### Brush (`lilyco-brush`)

brush（bash 兼容 shell）用 lilyco 重写 —— 给 AI 的 shell 工具：

```bash
lbrush --command "x=1; echo $x; ls | wc -l"        # CLI
lbrush --command "sleep 5" --timeout-secs 1        # 超时 kill
lbrush --command "ls" --json-stream                # AI 消费（JSONL）
lbrush --mcp                                      # MCP 服务器
```

- 每次调用全新 shell（`--no-config`），非零退出码不是工具错误（结构化返回 `exit_code`）
- DSH 接入：`lilyco-brush/dsh/cordis.patch.yml`（dsh-mcp-client stdio 直连，模型看到 `mcp__lbrush__Brush`）
- CI 产出 `lbrush-windows` artifact，本机不装 Rust 也能拿二进制

#### Android（Termux）

lbrush 跨平台，Android 用 headless 构建（CLI + MCP）：

```bash
# CI artifact：lbrush-android-arm64；或本地交叉编译：
cargo build -p lilyco-brush --no-default-features --features android --target aarch64-linux-android
```

- headless 关掉 TUI/Web 后端（crossterm 的 `cfg(unix)` 不含 android；`lilyco` facade 按 feature 门控）
- shell 解析自动命中 Termux bash（`/data/data/com.termux/files/usr/bin/bash`）
- Termux 里直接跑：`./lbrush --command "echo hi"`，或 `./lbrush --mcp` 给 AI agent 连

### Vision Toolkit (`lilyco-vision`)

DSH Vision Toolkit 的 Rust 重写 —— 8 个本地视觉操作，`Registry` 注册 + `--mcp` 给 DSH 提供视觉：

```bash
lvision --list                                     # 打印全部工具 schema
lvision --mcp                                      # MCP 服务器（8 个原生工具）
```

| 工具 | 功能 | 依赖 |
|---|---|---|
| `ImageInfo` | 尺寸 / 格式 / 大小 | image |
| `Crop` | 像素框裁剪 + 缩放（LANCZOS） | image |
| `Resize` | 等比缩放 | image |
| `DominantColors` | 主色提取（贪心聚类 + 容差） | image（自研算法） |
| `PixelDiff` | 网格级像素差异排行 + 热力图 | image（自研算法） |
| `ExtractForeground` | 前景抠图（边界泛洪，透明 PNG） | image（自研算法） |
| `Trace` | 位图矢量化 → SVG | [vtracer](https://github.com/visioncortex/vtracer) |
| `HtmlScreenshot` | HTML → PNG（headless Chrome/Edge） | 浏览器 |

- 全部本地计算，无 Python 运行时（原版是 Pillow+numpy+vtracer 的 uv 环境）
- DSH 接入：`lilyco-vision/dsh/cordis.patch.yml`（模型看到 `mcp__lvision__*` 工具）
- CI 产物：`lvision-windows` / `lvision-android-arm64`
- 服务类工具（glance/ground/detect/OCR）依赖外部视觉服务，v1 不做

### FFmpeg Transcode (`lilyco-ffmpeg`)

`lffmpeg` —— ffmpeg 包装：转码 / 缩放 / 裁剪，实时进度 + 取消，四端 + AI 可调：

```bash
cargo install --path lilyco-ffmpeg                   # 源码安装（binstall 现在还拿不到，见下）
lffmpeg --input a.mp4 --output b.mp4 --codec h265 --crf 28   # CLI
lffmpeg --input a.mp4 --output b.mp4 --width 1280            # 缩放（高度自动等比）
lffmpeg --input a.mp4 --output clip.mp4 --start 10 --duration 5  # 裁剪
lffmpeg --input a.mp4 --output b.mp4 --json-stream          # AI 消费（JSONL 事件流）
lffmpeg --mcp                                              # MCP 服务器
```

- 实时进度：解析 ffmpeg `-progress pipe:1`；`ffprobe`（可选）算完成百分比，缺失降级不确定进度
- 取消：CLI `Ctrl-C`、TUI `Ctrl-C`/`c`/`q`/`Esc`，kill 正在运行的 ffmpeg
- 非零退出不是工具错误：结构化返回 `exit_code` + stderr 摘要
- **cargo binstall 现在还达不到**：`[package.metadata.binstall]` 写的是 `pkg-url = ".../v{ version }/{ name }-{ target }{ binary-ext }"`，而 binstall 的 `{ name }` 展开成**包名**（`lilyco-ffmpeg`）；打 tag 时 CI 发布的资产却按**二进制名**命名（`lffmpeg-x86_64-pc-windows-msvc.exe` / `lffmpeg-aarch64-linux-android`），URL 对不上 → 只会 404 再回退源码编译。改名放 CI 的 `Stage release files`（多拷一份包名命名的资产）最省事，两种改法与出处见 [`docs/INTEGRATION.md`](docs/INTEGRATION.md) §0
- 完整用法：见 [`docs/lffmpeg.md`](docs/lffmpeg.md)
- 依赖系统 `ffmpeg`（必须在 PATH 上）

### 办公文件与容器结构 (`lilyco-binfmt`)

`lbin` —— 办公文件处理是它的主业，二进制/容器结构是同一套读法的另一半：**12 条命令全 T0 只读**
（不执行、不写盘；读 docx 的正文必须在内存里解压，解压结果一律要过该部件自己声明的 CRC-32）：

```bash
cargo install --path lilyco-binfmt                    # 从源码装（该 crate 还没上 crates.io，`cargo binstall` 要等发布 + 预编译资产）
lbin office-info --path 预算.docx --json              # 这是什么、谁写的、有没有宏/加密/外链
lbin office-text --path 预算.docx                     # 文件里写了什么（docx/doc/xlsx/pptx/odt/rtf）
lbin office-meta --path 季度报告.pptx --json          # 文档属性：docProps / meta.xml / OLE 属性集
lbin office-doc --path 预算.docx                      # 段落、标题层级、表格、超链接、批注、修订
lbin office-sheet --path 预算表.xlsx                  # 每张表（含隐藏的）、范围、公式、命名区域
lbin office-slide --path 评审.pptx                    # 放映顺序、每页标题与备注、版式与母版
lbin office-package --path 预算.docx --json           # 包自证：关系断头、部件没声明类型、CRC 没过
lbin office-objects --path 预算.docx --json           # 嵌入物、外链、宏与加密这些要留心的东西
lbin identify --path app.apk --json                   # 什么族什么格式 + 头部自报的字段
lbin entries  --path app.apk --limit 200              # 中央目录列出的成员（ZIP/tar/ar）
lbin regions  --path a.out --json                     # 头/表/代码/数据/空闲/尾部叠加
lbin symbols  --path /usr/bin/ls --json               # 节表 + .symtab/.dynsym + 地址→名字
lbin --mcp                                            # MCP 服务器：tools/list 一次返回十二条
```

- 认格式**看部件名与流名，不看文件后缀**：改了后缀的 docx 照样报 docx；`docm` 靠 `vbaProject.bin` 认
- 覆盖 OOXML（docx/docm/xlsx/xlsm/pptx/pptm）、ODF（odt/ods/odp）、遗留复合文档（doc/xls/ppt）、RTF：
  `.doc` 走 FIB→piece 表、`.xls` 走 BIFF8 记录（格子按 BOUNDSHEET 自报的偏移归位到每张表）、
  `.ppt` 走 PowerPoint 97 记录树取文本原子，RTF 的属性走 `\info` 群与 `\*\userprops`；
  `.ppt` 的按页归位还没做，
  做不到的部分照实留空并说明原因，不给一份看着像「没有」的答案

- 置信度分两档：`signature`（开头字节定死）与 `structural`（表刚好铺进文件才敢这么说）——`0xCAFEBABE` 既是 Java class 也是通用二进制，靠架构表落点判别
- 每个答案带自证：`entries` 报 `checks[]`（文件自报的数目/长度对不上就直说），`regions` 保证 `claimed + unreferenced + loaded_unaddressed == 读进来的字节数`
- `gap`（绿色）只表示「没有表点到这里」，不是「改这里安全」——校验和与签名不留痕迹
- 认不出签名、或该族的表还没实现，就 `mapped: false` + 原因，不编区间
- 完整用法：见 [`docs/binfmt.md`](docs/binfmt.md)

### Alice in Cradle 全格式逆向 (`lilyco-aic`)

![laic render — PixelLiner 姿势帧合成](docs/laic/hero.png)

> `laic render` 的输出：`%POSE_SECTION%` 的图层变换 + `%PACK_SECTION%` 的 UV +
> UnityFS 里的 Texture2D，三者合成一张 PNG。从左起：**站立 / 行走起手 / 行走中段 / 跳跃**。

`laic` —— PixelLiner 引擎游戏资源（角色表 / 贴图 / 立绘换装容器）的**完全逆向 + 写回**。
**10 条命令：8 条 T0 只读，`repack` / `pack` 两条 T1**（默认 dry-run，只报计划不写盘；
MCP 自动化面默认拒绝，人类在环才放行）：

```bash
laic find-pose D:/gal/.../StreamingAssets --name "gun*" --json    # 谁的表里有 gun 姿势？
laic poses   .../PxlNoel/noel.pxls.dat --json                      # 姿势清单（--full 加每帧图层 + 组层树 + 矢量）
laic frame   .../Enemies/honeycomb.pxls.dat --pose gun --index 0   # 抄姿势要的全部字段：name/kind/group/alpha/x/y/zmx/zmy/rot_r/img/img_size/tree
laic tex     .../PxlNoel --out D:/tmp/tex --json                   # 贴图清单 + 解码出 PNG（每项带 format_size_ok 自证）
laic mpcc    .../StreamingAssets/mobpcc --json                     # 立绘换装/改色表：每个部件的 HSV + 色调曲线操作（roundtrip_ok 自证解析完整）
laic sprites .../PxlNoel/noel.pxls.dat --out D:/tmp/sp             # 按 UV 裁出每个 sprite，以姿势层名命名
laic render  .../PxlNoel/noel.pxls.dat --pose 'walk' --out D:/tmp/r --anim --sheet   # 合成 PNG / 每方向 GIF / 精灵表
laic animate .../PxlNoel/noel.pxls.dat --pose walk --ticks 40      # 播放头：推进 40 个 tick 后落在哪一帧
laic repack  .../PxlNoel/noel.pxls.dat --out D:/tmp/out --rename old=new --json      # 改名写回（先看计划，--apply 才写）
laic pack    .../PxlNoel/noel.pxls.dat --out D:/tmp/pxl --replace D:/tmp/sp_edits    # 图集重排 + 替换，产出 .pxl
laic --mcp                                                         # MCP：tools/list 一次返回十条
```

#### 效果一览

**动画** —— `laic render --pose walk --anim`（`%POSE_SECTION%` 的 `crf60` 帧时长驱动，12 帧循环）：

![laic render --anim](docs/laic/walk.gif)

**贴图** —— `laic tex` / `sprites --atlas` 解出的 4096×4096 主贴图（DXT5/BC3，`.resS` 资源流）：

![noel atlas 0](docs/laic/atlas.png)

**sprite** —— `laic sprites` 按 UV 裁出的其中 48 个（noel 双图集共 3010 个，以姿势层名命名）：

![laic sprites](docs/laic/sprites.png)

**PARTS 变体** —— `render --parts`：同一帧改用第二条图集 PARTS（texture_1）的像素合成，
撕破 / 换装差分一目了然（几何不变，只换像素）：

![render --parts](docs/laic/parts.png)

**改完装回去** —— `sprites` 裁 → 改 PNG → `pack --replace` 重排图集 → `.pxl`，
再渲染出来验证：只有被替换的那一个 sprite 变了，其余**逐像素不变**：

![pack --replace](docs/laic/replace.png)

#### 安装

```bash
# ① 源码安装（今天唯一可用的方式，要 Rust stable 工具链；
#    laic 分支合进 main 之前，先 git checkout laic）
git clone https://github.com/lilyco-42/lilyco && cd lilyco
git checkout laic
cargo install --path lilyco-aic

# ② 只要二进制：cargo build --release -p lilyco-aic，拿 ./target/release/laic
#    安卓 Termux：--no-default-features --features android（纯 Rust，CLI + MCP）
```

数据面：命令吃单个 `.pxls`、UnityFS 包裹的 `.pxls.dat`（自动解包）、或整棵目录
（递归找 `*.pxls.dat` / `*.pxls` / `*.pxls.bytes`）。示例里的路径换成你自己的
`AliceInCradle_Data/StreamingAssets` 即可。

#### 怎么用（四条管线，数据源都是游戏 `AliceInCradle_Data/StreamingAssets` 目录）

```bash
# ① 看：先定位姿势，再决定抄什么
laic find-pose .../StreamingAssets --name "gun*" --json
laic frame .../Enemies/honeycomb.pxls.dat --pose gun --json > gun.json   # 全方向全帧的图层变换

# ② 出：裁 sprite 做 mod 素材，渲染预览确认长相对
laic sprites .../PxlNoel/noel.pxls.dat --out ./sp --pose 'big*'
laic render  .../PxlNoel/noel.pxls.dat --pose 'walk' --out ./r --anim --sheet

# ③ 动：问「这段动画一个循环多少 tick、从第几帧回绕」
laic animate .../PxlNoel/noel.pxls.dat --pose walk --json
#   -> { loop_to, ticks_per_loop, frame_count, position, stepped, looped_count }
laic animate .../PxlNoel/noel.pxls.dat --pose walk --ticks 130 --out ./r   # 推进 130 tick 并把落点帧渲出来

# ④ 写：先看计划，确认无误再落盘（MCP 面直接拒绝，只能人敲）
laic repack .../PxlNoel/noel.pxls.dat --out ./out --rename gun=gun_new --json
laic repack .../PxlNoel/noel.pxls.dat --out ./out --rename gun=gun_new --apply
```

#### 完整闭环：改一个 sprite 并装回去

这是 laic 相对另外两个开源实现的**独有**能力（`pack` = T1，默认 dry-run）：

```bash
# 1) 裁出全部 sprite（名字就是后面查替换图的主键）
laic sprites .../PxlNoel/noel.pxls.dat --out ./sp --json

# 2) 用任意图像编辑器改 ./sp/noel/Layer.EDIba8e8_78944574514.png（同尺寸重着色最安全）

# 3) 重排图集并写回：产出自带贴图的 .pxl，游戏直接可读，不用再外挂 texture_0.dat
laic pack .../PxlNoel/noel.pxls.dat --out ./pxl --replace ./sp --atlas --apply --json
#    -> replaced: 1（只有目标 sprite 被替换）
#       unmatched_replacements: []（目录里没被认领的 PNG，名字写错会报在这里）
#       verified: true（写后重读重解析，3010 个 sprite 逐像素比对）

# 4) 验证：渲染重排后的表，与原表渲染结果对比
laic render ./pxl/noel.pxl --pose stand --dir 0 --frame 2 --scale 2 --out ./check
```

> ⚠️ **替换图别按层名乱放**：一个表里可能有几百个 sprite 的层名都叫 `Layer`
> （noel 表里是 **618 个**：导出时去重成 1 个 `Layer.png` + 617 个 `Layer.<img-key>.png`）。
> `pack --replace` 严格按这套导出名查找，**一张图只能被一个 sprite 认领**，
> 认领不到的文件会原样报在 `unmatched_replacements` 里，不会静默丢。

#### 比 pixelliner4j + AICPxlsUnpacker 强在哪

pixelliner4j（Java）和 AICPxlsUnpacker（Python）是同领域的两个开源实现，laic 读完了它们的
全部源码逐项对齐，**功能覆盖 ≥ 二者之和**，且独占 Unity 侧解码与写回：

| 能力 | pixelliner4j | AICPxlsUnpacker | laic |
|---|---|---|---|
| 解析 pxls 六节 | ✓ 结构化 | ✗ 字节模式硬找 | ✓ 结构化 + 128 表逐字节回环回归 |
| 贴图来源 | ✗ 要外部 PNG | ✗ 要现成 `texture_0.png` | ✓ **UnityFS + SerializedFile v22 + Texture2D 全解码** |
| 按 UV 裁 sprite | ✓ 内存里 | ✓ 坐标靠硬找 | ✓ 落盘，名字取**姿势层名** |
| 合成一帧 | ✓ Java2D（只在其 GUI 里） | ✗ | ✓ 落盘 PNG / GIF / 精灵表 |
| 播放头状态机 | ✓ `FrameAnimator` | ✗ | ✓ `animate` |
| 图集重排 → `.pxl` | ✓ `PxlsKiller` | ✗ | ✓ `pack`（占用率 92% vs 20%） |
| 写回 UnityFS | ✗ | ✗ | ✓ `repack` |
| 双图集 NORMAL + PARTS | ✗ 压成一张 | ✗ | ✓ 继承语义保留，`--parts` 可预览 |
| 保留 IMGS / IMGV / PTCL | ✗ 只写 PACK + POSE | ✗ | ✓ 全节保留 |
| margin 留白 / UV w/h | ✗ / 重算 | ✗ | ✓ 保留，渲染尺寸零漂移 |
| 四端 CLI / TUI / Web / MCP | ✗ | ✗ | ✓ 同 registry |

（pixelliner4j 的两个 Swing GUI —— `PixelPreviewer` 动画预览器与 `PxlsKiller` 批量转档器 ——
刻意不搬：laic 的 GUI 由 Web 端承担，输出落盘，headless 可跑。）

- 读侧全覆盖（游戏 StreamingAssets 实测）：**128/128 张 pxls 表**零错误解析；
  **125 个 texture 包 / 133 张 Texture2D**，122 张解码出 PNG（DXT5/BC3、DXT1/BC1、BC7、
  Crunch 走 `texture2ddecoder`，其余内建解包，Unity 像素底行在前统一翻成顶左原点）
- 格式认错立刻现形：Unity `TextureFormat` 全表内建（**12 = DXT5/BC3，不是 DXT1**），
  每项用 `m_CompleteImageSize` 与格式表交叉验证（`format_size_ok`），块格式还有先验长度守卫
- 图集按 asset 名后缀 `_<i>` 配对，**不按对象表顺序**：`noel_t` 的对象表是倒的
  （`texture_1` 排在 `texture_0` 前面），按顺序取会把主图 / 部件图对调——有像素级回归测试钉住
- 写侧（`repack` 改姿势名 / 图层 alpha，`pack` 重排图集）：裸 pxls 直写；UnityFS 包则就地替换
  TextAsset 正文，并修 TypelessData 长度前缀、SerializedFile 头 `file_size`、对象表
  （`byte_size` 重算 4 字节对齐、后续对象平移）与 blocks info / 节点表；写后自检
  （重读 → 重解析 → 逐像素比对），不过就报错
- 装箱占用率实测（noel 图集 0，1505 个 sprite）：默认 shelf **91.9%**（边长 4096，画布再裁到
  实际用量 4089×2090）；pixelliner4j 的 `Algorithm.packRectangles` 只有 **20.4%**（要 8192 边长），
  保留为 `--packer guillotine` 仅作对照
- 立绘换装容器（`mpcc`）完整解析：`name` / `chr_name` + 每个部件的 HSV（色相角度 / 饱和度 /
  明度百分比）与色调曲线（每通道关键点），部件表**按字典插入序保序**（游戏写回依赖它）。
  照抄游戏两处「读了就丢」的语义：操作类型 0、以及操作数为 0 的部件，字节照吃但条目丢弃，
  `dropped` 如实报出丢了多少。每个文件都**逐字节回环**自证（`roundtrip_ok`）——
  真机 5 份容器（24–250 B）全部 `true`、零错误
- 做不到的部分照实说：`.cmd` 事件脚本是另一子系统（未覆盖）；IMGV 矢量只解析不参与绘制
- 输出默认 pretty 模式**只打印 <500 字节的结果**，大结果一律加 `--json`
  （`mpcc` 5 份容器就有 13 KB，不加 `--json` 屏幕上只会剩一行 `Done in 1ms`）
- `laic --schema` 看机器契约；`--tui` 进选择页、`--gui` 进 Web 控制台、`--mcp` 起 MCP 服务器，
  四端同一 registry
- 完整用法与逆向笔记：见 [`docs/laic.md`](docs/laic.md)；命令说明也可看
  [`lilyco-aic/README.md`](lilyco-aic/README.md)

> 上方示例图渲染自游戏 *Alice in Cradle* 的资源文件，仅用于演示本工具的输出效果，
> 素材版权归原作者所有；本仓库不随附任何游戏资源。

### Transcode (TUI demo)

```rust
use lilyco_macros::{App, ValueEnum};
use lilyco_core::prelude::*;
use std::path::PathBuf;

#[derive(ValueEnum)]
enum Codec { H264, H265, Av1 }

#[derive(App)]
#[app(about = "Transcode video files")]
struct Transcode {
    #[arg(about = "Input file", must_exist = true)]
    input: PathBuf,

    #[arg(about = "Codec", default = "h264")]
    codec: Codec,

    #[arg(about = "Quality 0-51", default = 23, range = 0..=51)]
    quality: u8,
}
```

### Ultra UI (`lilyco-ultra-ui-example`)

```bash
cargo run -p lilyco-ultra-ui-example
# Open http://localhost:9090 in your browser
```

Edit the JSON spec in the browser; the React UI updates in real time.

---

