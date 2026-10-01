# lpxls — PixelLiner 角色资源域（.pxls 姿势 / 图层 / 结构数据提取）

> 面向游戏 mod 开发：Alice in Cradle（PixelLiner 引擎）的角色 `.pxls` 表。
> 回答三类问题：**有哪些姿势**、**某姿势每帧每层的变换数据**（mod 抄姿势要用的全部字段）、
> **哪个角色的表里有某个姿势**（跨表搜姿势存在性）。

## 命令（全 T0 只读）

| 命令 | 作用 |
|---|---|
| `poses` | 列出角色表全部姿势：标题 / 尺寸 / 方向序列 / 帧数 / 别名；`--pose` glob 过滤；`--full` 附带每帧图层明细 |
| `frame` | 导出指定姿势的帧 × 图层变换：`name / kind / group / alpha / x / y / zmx / zmy / rot_r / blend_variable / img / img_size` |
| `find-pose` | 跨整棵目录的所有表搜姿势名（glob、含别名匹配），报告「谁有这个姿势」 |

`root` 三种都吃：单个 `.pxls` 文件、UnityFS 包裹的 `.pxls.dat`（自动解包）、或整棵目录
（递归找 `*.pxls.dat` / `*.pxls` / `*.pxls.bytes`，自动跳过 `*.texture_0.dat` 贴图分片）。

## 典型用法

```bash
# 全库搜 gun 姿势（答案：只有 Enemies/honeycomb 有 gun / gun2stand，玩家 noel 表没有）
lpxls find-pose D:/gal/.../StreamingAssets --name "gun*" --json

# 抄小兵 gun 姿势第 0 帧的图层变换（mod 姿势移植的数据源）
lpxls frame --root .../Enemies/honeycomb.pxls.dat --pose gun --index 0 --json

# noel 表全部 183 个姿势的清单
lpxls poses --root .../PxlNoel/noel.pxls.dat --json
```

CLI 注意：默认 pretty 模式**只打印 <500 字节的结果**，大结果必须带 `--json`。

## 解析器规格（自研，逆向自 PixelLiner 反编译）

- **pxls 容器**：`u32 0x7741A8FF + "PXLS"` 头，随后若干 `14 字节节名 + u32 大端长度 + 载荷`
  （`%IMGx_SECTION%` / `%POSE_SECTION%` / `%PTCL_SECTION%`），自封闭可整读。
- **字节序：大端**。`ByteReader.readInt/readShort` 手工逆序 + 小端 `BitConverter` 的组合
  等价于磁盘大端（这是最容易踩错的点，UnityPy/游戏机端都对得上）。
- **字符串**：`u16 长度 + utf-8`；**子流**：`u32 长度 + 字节`。
- 姿势树：`PxlPose`（头子流 + 主流上 8 方向序列）→ `PxlSequence`（头子流 + 主流上帧）
  → `PxlFrame`（头子流 + 主流上图层）→ `PxlLayer`（主流直读；`type==8` 为组层，
  只吃 `u32 preserve_contain_layers`）。
- 图层数值语义（照游戏运行时反推）：`x,y = i16/10`；`alpha = i16/100` 截 0..100；
  `zmx,zmy,rot_r` 是 f64；图层随后的 `u16 + u32 + byte + byte` 是保留字段。
- **UnityFS 解包**：header version ≥ 7 读完 flags 要 **16 字节对齐**（Switch 补丁格式变化）；
  blocks info（LZ4/LZ4HC，`lz4_flex`）+ 数据块（LZ4 或 **LZMA**——Unity 的 LZMA1 无
  size 头，补全 alone 头 13 字节后用 `lzma-rs` 解，size 用块表里的精确值）；
  解包后按 8 字节签名定位 pxls（容错前 3 个误命中），不解析 SerializedFile 对象表
  （pxls 自封闭；将来做写回才需要对象表切片）。

## 已知限制（v1）

- `%IMGV_SECTION%`（矢量图数据）不逐条解析，只计条目数。
- 嵌入 PNG（`%IMGS_SECTION%` type 0/8）不解析像素尺寸；尺寸以图集 `%PACK_SECTION%`
  的 UV 表为准（已回填到 `img_size`）。
- 组层（type 8）的子层归属按文件顺序平铺，未重建树（帧内 layer 顺序即绘制顺序）。
- 只读。姿势写回（把 honeycomb 的 gun 移植进 noel 表）是二期：需要 SerializedFile
  对象表定位 TextAsset 的 m_Script 精确切片。
