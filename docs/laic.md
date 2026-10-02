# laic — Alice in Cradle 全格式域（PixelLiner 资源完全逆向）

> crate `lilyco-aic` / bin `laic`。面向游戏 mod 开发：Alice in Cradle（PixelLiner 引擎）的
> 角色表 / 贴图 / 立绘换装容器，四端（CLI / TUI / Web / MCP）同 registry。
> 回答四类问题：**有哪些姿势**、**某姿势每帧每层的变换与组层树**（mod 抄姿势的全部字段）、
> **哪个角色的表里有某个姿势**、**贴图长什么样**（解码出 PNG）。

## 命令（全 T0 只读）

| 命令 | 作用 |
|---|---|
| `poses` | 列出角色表全部姿势；`--pose` glob 过滤；`--full` 附带每帧图层明细 + **组层树**（`tree`）；文件级附 **矢量图数据**（`vectors`） |
| `frame` | 导出指定姿势的帧 × 图层变换 + 每帧**组层树**：`name / kind / group / alpha / x / y / zmx / zmy / rot_r / blend_variable / img / img_size / tree` |
| `find-pose` | 跨整棵目录的所有表搜姿势名（glob、含别名匹配），报告「谁有这个姿势」 |
| `tex` | 列出 `*.texture_0.dat` 包内全部 Texture2D（尺寸/格式/存储方式）；`--out DIR` 解码出 PNG（RGBA32 直读，DXT1/BC1、DXT5/BC3 走 `texture2ddecoder`），内联与 `.resS` 资源流双路 |
| `mpcc` | 解析 `mobpcc/*.mpcc.bytes` 头部：容器 `name` / `chr_name` / 调色板标志 / 剩余载荷 |

`poses` / `frame` 的 `root` 三种都吃：单个 `.pxls` 文件、UnityFS 包裹的 `.pxls.dat`（自动解包）、
或整棵目录（递归找 `*.pxls.dat` / `*.pxls` / `*.pxls.bytes`，自动跳过 `*.texture_0.dat`）。

## 覆盖面（游戏 StreamingAssets 实测）

- **128/128 张 pxls 表**全部解析零错误（Enemies 29 / EvImg 40 / MapChars 17 / MapChips 18 /
  PxlNoel 12 / PxlCane 6 / Pxl 4 / Fis 1 / mgm_bun 1）。
- **125 张 texture_0.dat**：UnityFS → v22 SerializedFile → Texture2D（typetree 驱动解析），
  DXT1/RGBA32 解码出 PNG（noel 4096×4096 主贴图 + 法线贴图实测通过）。
- **组层树**：`fineLinks` 语义重建（组层 k8 认领文件序前方 N 条目，嵌套组消耗子树足迹），
  138 层的 `mapchip_grazia` 帧实测 138↔138 全对。
- **矢量**：`%IMGV_SECTION%` 逐条解析（多边形顶点，z bit0 = 新子路径）。
- **5 张 mobpcc**：头部全解（NOEL__darknoel 等）。

## 典型用法

```bash
# 全库搜 gun 姿势（答案：只有 Enemies/honeycomb 有 gun / gun2stand，玩家 noel 表没有）
laic find-pose D:/gal/.../StreamingAssets --name "gun*" --json

# 抄小兵 gun 姿势第 0 帧的图层变换 + 组层树（mod 姿势移植的数据源）
laic frame --root .../Enemies/honeycomb.pxls.dat --pose gun --index 0 --json

# noel 表全部 183 个姿势的清单（--full 再加每帧图层 + 组层树）
laic poses --root .../PxlNoel/noel.pxls.dat --json

# 导出 noel 的主贴图（DXT1, resS 资源流）与法线贴图（RGBA32 内联）为 PNG
laic tex --root .../PxlNoel/noel.pxls.bytes.texture_0.dat --out D:/tmp/tex --json

# 立绘换装容器清单（darknoel 等）
laic mpcc --root .../StreamingAssets/mobpcc --json
```

CLI 注意：默认 pretty 模式**只打印 <500 字节的结果**，大结果必须带 `--json`。

## 解析器规格（自研，逆向自 PixelLiner 反编译 + UnityPy 交叉验证）

### pxls 容器

- `u32 0x7741A8FF + "PXLS"` 头，随后若干 `14 字节节名 + u32 大端长度 + 载荷`
  （`%IMGS_SECTION%` / `%PACK_SECTION%` / `%IMGV_SECTION%` / `%POSE_SECTION%` / `%PTCL_SECTION%`），
  自封闭可整读。
- **字节序：大端**。`ByteReader.readInt/readShort` 手工逆序 + 小端 `BitConverter` 的组合
  等价于磁盘大端（这是最容易踩错的点，UnityPy/游戏机端都对得上）。
- **字符串**：`u16 长度 + utf-8`；**子流**：`u32 长度 + 字节`。
- 姿势树：`PxlPose`（头子流 + 主流上 8 方向序列）→ `PxlSequence`（头子流 + 主流上帧）
  → `PxlFrame`（头子流 + 主流上图层）→ `PxlLayer`（主流直读；`type==8` 为组层，
  只吃 `u32 preserve_contain_layers`）。
- 图层数值语义（照游戏运行时反推）：`x,y = i16/10`；`alpha = i16/100` 截 0..100；
  `zmx,zmy,rot_r` 是 f64。
- 图节：IMGS type 0/8 带两张嵌入 PNG（读 IHDR 回填尺寸）；PACK = 图集 UV 表（id, id2, x, y, w, h，
  减 margin×2 为像素尺寸）；IMGV = 多边形顶点 `(f32 x, f32 y, f32 z)`，z bit0 = 新子路径。

### UnityFS

- header version ≥ 7 读完 flags 要 **16 字节对齐**（Switch 补丁格式变化）；
- blocks info（LZ4/LZ4HC `lz4_flex`，或 **LZMA**——Unity 的 LZMA1 无 size 头，
  补全 alone 头 13 字节后用 `lzma-rs` 解）；
- **节点表**（v≥7）：`offset u64 + size u64 + flags u32 + path(null 结尾)`，
  offset 相对解压数据区 —— `archive:/*.resS` 资源流按此切。

### SerializedFile v22（Unity 2022.3.62f2）

- 头 48 字节全大端：16B 占位（metadata_size=0, file_size=0, version=22, data_offset=0）
  + endian 字节(0=LE) + 3 保留 + metadata_size u32 + file_size u64 + data_offset u64 + unknown u64；
- 元数据（LE）：unity 版本串 → target_platform i32 → enable_type_tree → 类型表 → 对象表；
- 类型表：class_id i32 + stripped bool + script_type_index i16 + old_type_hash 16B
  （class_id==114 再加 script_id 16B）→ typetree blob：node_count + stringbuffer_size
  + **32B/节点**（`i16 version, u8 level, u8 type_flags, u32 typeStr, u32 nameStr,
  i32 byteSize, i32 index, i32 metaFlag, u64 refHash`）+ stringbuffer + type_dependencies；
  ⚠️ 第一个 i16 是 **version 不是 level**（UnityPy keys 序），搞错会全盘错位；
- 字符串偏移高位 1 = **CommonString 内置表**（114 条，已内置）；
- 对象表：align4 → path_id i64 → byte_start u64(+data_offset) → byte_size u32 → type_id i32。

### typetree 驱动解析（serialized.rs）

- 对齐**不是猜的**：节点 MetaFlag 第 14 位（0x4000）= AlignBytes，读完后 align4；
  string/vector 的对齐标志挂在其 **Array 包装节点**上。
- **string** = `u32 len(不含 null) + len 字节 + align`（"额外 null" 是 align padding 的误会）；
- **容器节点数**：string / vector = 容器 + Array + size + data **4 节点**；
  **TypelessData 没有 Array 包装**（容器 + size + data **3 节点**）——UnityPy 的展示树里有 Array
  是重建产物，照抄会雪崩；
- 复合类型（GLTextureSettings / StreamingInfo…）递归子字段；数组元素复合时循环解析。

### MPCC（MobPCCContainer.readFromBytesFromFile）

`byte 占位 + string name + string chr_name + byte 调色板标志 + SkltPalette 深层载荷`
（string = u16 长度 + utf-8，pxls 同族；深层 ACC 调色板是二期）。

## 已知限制

- MPCC 深层 ACC 调色板（换装 parts 表）只报剩余字节数，未逐条解析。
- 贴图解码支持 RGBA32 / DXT1 / DXT5；Crunched（16/17）等格式只列清单不解码。
- 只读。姿势写回（把 honeycomb 的 gun 移植进 noel 表）是二期：SerializedFile 对象表 +
  typetree 驱动解析已就位（`read_object` 直接可读 TextAsset 的 m_Script），差对象字节级写回。
