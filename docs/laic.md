# laic — Alice in Cradle 全格式域（PixelLiner 资源完全逆向）

> crate `lilyco-aic` / bin `laic`。面向游戏 mod 开发：Alice in Cradle（PixelLiner 引擎）的
> 角色表 / 贴图 / 立绘换装容器，四端（CLI / TUI / Web / MCP）同 registry。
> 回答七类问题：**有哪些姿势**、**某姿势每帧每层的变换与组层树**（mod 抄姿势的全部字段）、
> **哪个角色的表里有某个姿势**、**贴图长什么样**（解码出 PNG）、**每个 sprite 长什么样**
> （按 UV 裁切，以姿势层名命名）、**某姿势画出来什么样**（合成 PNG / GIF / 精灵表）、
> **改完怎么写回去**（pxls 写回 UnityFS）。

## 命令（10 条：8 条 T0 只读 + `repack` / `pack` T1 需确认）

| 命令 | 作用 |
|---|---|
| `poses` | 列出角色表全部姿势；`--pose` glob 过滤；`--full` 附带每帧图层明细 + **组层树**（`tree`）；文件级附 **矢量图数据**（`vectors`） |
| `frame` | 导出指定姿势的帧 × 图层变换 + 每帧**组层树**：`name / kind / group / alpha / x / y / zmx / zmy / rot_r / blend_variable / img / img_size / tree` |
| `find-pose` | 跨整棵目录的所有表搜姿势名（glob、含别名匹配），报告「谁有这个姿势」 |
| `tex` | 列出 `*.texture_0.dat` 包内全部 Texture2D（尺寸/格式/存储方式）；`--out DIR` 解码出 PNG（Unity `TextureFormat` 全表：12=DXT5/BC3、25=BC7、28/29=Crunch 等，块格式走 `texture2ddecoder`），内联与 `.resS` 资源流双路；每项附 `format_size_ok`（`m_CompleteImageSize` 与格式表交叉验证，认错格式立刻现形） |
| `mpcc` | 解析 `mobpcc/*.mpcc.bytes` 立绘换装/改色表（T0）：容器 `name` / `chr_name`，每个部件 key 的操作链 —— `hsv`（`h` 原值 + 角度、`s`/`v` 原值 + 百分比、`flags`）与 `tone_curve`（通道数 + 每通道点数，`--full` 附 0..255 关键点）。部件表**保序**（游戏写回走字典插入序）。每份文件**逐字节回环**自证 `roundtrip_ok`，`dropped` 报出游戏「读了就丢」的条目数 |
| `sprites` | 按图集 UV 裁切导出每个 sprite 的 PNG（T0）：外部贴图按 asset 名后缀 `_<i>` 配对（`noel_t` 的对象表是倒的，按顺序取会把主图/部件图对调），文件名取姿势层名；PARTS 图集（UV 为 0）继承前一图集的 UV 表 |
| `render` | 把姿势帧合成为 PNG / GIF / 精灵表（T0）：变换语义照抄 `PxlMeshDrawer.makeMesh → RotaGraph`（中心定位、zmx/zmy 缩放、`-rotR` 旋转、alpha/100 混合、Point 点采样，`rotR==0` 的奇偶 +0.5 修正也复刻）；`--parts` 改用 PARTS 图集（texture_1）合成，撕破/换装变体的精确预览 |
| `animate` | **播放头状态机**（T0，对应 pixelliner4j 的 `FrameAnimator`）：`--ticks N` 推进 1/60 s 刻度，`--frame NAME` / `--index N` 直接定位，报 `position / stepped / looped_count / ticks_per_loop`；加 `--out` 可把落点那一帧渲出来（`--parts` 同 render）。越界的 `loopTo` **夹到 0**（Java 原版会下标越界崩）并报 `loop_to_clamped` |
| `pack` | **T1**：图集重排并导出自带贴图的 `.pxl`（对应 pixelliner4j `writePackSection` + `Algorithm.packRectangles`，`PxlsKiller` 靠它产出 `.pxl`）。闭环的最后一块：`sprites` 裁 → 改 PNG → `pack --replace` 装回去。默认 dry-run，写后逐像素自检 |
| `repack` | **T1**：把改过的 pxls 写回（`--rename OLD=NEW`、`--set-alpha LAYER=0..100`）：裸 pxls 直写；UnityFS 则就地替换 TextAsset 正文并修 TypelessData 长度前缀、SerializedFile 头 `file_size`、对象表（`byte_size` 重算对齐、后续 `byte_start` 平移）与 blocks info/节点表；默认 dry-run，写后自检（重读→重解析→重序列化逐字节比对） |

`poses` / `frame` 的 `root` 三种都吃：单个 `.pxls` 文件、UnityFS 包裹的 `.pxls.dat`（自动解包）、
或整棵目录（递归找 `*.pxls.dat` / `*.pxls` / `*.pxls.bytes`，自动跳过 `*.texture_0.dat`）。

## 覆盖面（游戏 StreamingAssets 实测）

- **128/128 张 pxls 表**全部解析零错误（Enemies 29 / EvImg 40 / MapChars 17 / MapChips 18 /
  PxlNoel 12 / PxlCane 6 / Pxl 4 / Fis 1 / mgm_bun 1）。
- **125 张 texture_0.dat / 133 张 Texture2D**：UnityFS → v22 SerializedFile → Texture2D
  （typetree 驱动解析），122 张解码出 PNG（DXT5/BC3 71 张、BC7 39 张、RGB565/RGBA32/RGB24，
  含 noel 4096×4096 主贴图 + 法线贴图）。11 张事件图是 DXT5Crunched（`decode_unity_crunch`
  直解，`astc-decode` 依赖已删）。
- **8 张双图集表**（`noel` 系列等）的主包各装 2 张 Texture2D：图集 `i` 按 asset 名后缀
  `_<i>` 取图，`noel_t` 的对象表是倒的（`texture_1` 在前），有专门的回归测试钉住。
- **组层树**：`fineLinks` 语义重建（组层 k8 认领文件序前方 N 条目，嵌套组消耗子树足迹），
  138 层的 `mapchip_grazia` 帧实测 138↔138 全对。
- **矢量**：`%IMGV_SECTION%` 逐条解析（多边形顶点，z bit0 = 新子路径）。
- **5 张 mobpcc**：头部全解（NOEL__darknoel 等）。
- **pxls 写回**：`parse → serialize` 全表逐字节回环；UnityFS 写回修 4 处
  （TypelessData 长度前缀 / 头 `file_size` / 对象表 / blocks info+节点表）。
  🔴 目标对象的 `byte_size` 必须**重算** `align4(长度字段 + 正文)`，不能 `+= 正文差` ——
  填充可增可减（368227→368228 的 1 字节填充，正文 +9 后变 0），写错 Unity 就读不回去。
  写回结果经 UnityPy 复核：两个对象都能干净读出，不再报
  `Expected to read N, but only read N+2`。

## 典型用法

```bash
# 全库搜 gun 姿势（答案：只有 Enemies/honeycomb 有 gun / gun2stand，玩家 noel 表没有）
laic find-pose D:/gal/.../StreamingAssets --name "gun*" --json

# 抄小兵 gun 姿势第 0 帧的图层变换 + 组层树（mod 姿势移植的数据源）
laic frame --root .../Enemies/honeycomb.pxls.dat --pose gun --index 0 --json

# noel 表全部 183 个姿势的清单（--full 再加每帧图层 + 组层树）
laic poses --root .../PxlNoel/noel.pxls.dat --json

# 导出 noel 的主贴图（DXT5, resS 资源流）与法线贴图（RGBA32 内联）为 PNG
laic tex --root .../PxlNoel/noel.pxls.bytes.texture_0.dat --out D:/tmp/tex --json

# 按 UV 裁出 noel 全部 sprite（以姿势层名命名）
laic sprites .../PxlNoel/noel.pxls.dat --out D:/tmp/sp --json

# 渲染 noel 的 big 系姿势（含 GIF + 精灵表）
laic render .../PxlNoel/noel.pxls.dat --pose 'big*' --out D:/tmp/r --anim --sheet --json

# 播放头推进 40 个 tick（1/60 s）后落在哪一帧
laic animate .../PxlNoel/noel.pxls.dat --pose stand --dir 0 --ticks 40 --json

# 改完 sprite 再装回图集，产出自带贴图的 .pxl（默认 dry-run，--apply 才写）
laic pack .../PxlNoel/noel.pxls.dat --out D:/tmp/pxl --replace D:/tmp/sp_edits --apply --json

# 把姿势改名写回（默认 dry-run，只报计划）
laic repack .../PxlNoel/noel.pxls.dat --out D:/tmp/out --rename old=new --json

# 立绘换装容器清单（darknoel 等）
laic mpcc --root .../StreamingAssets/mobpcc --json
```

CLI 注意：默认 pretty 模式**只打印 <500 字节的结果**，大结果必须带 `--json`。

## 图集重排与替换（`pack`，T1）

闭环：`sprites` 按 UV 裁出每个 sprite → 改 PNG → `pack --replace` 重排回图集 → 产出自带
贴图的 `.pxl`（游戏可直接读，不用再外挂 `texture_0.dat`）。

### 装箱：别用 pixelliner4j 那个算法当默认

边长从起始值（默认取原图集尺寸向上取到 2 的幂）起翻倍直到放下全部，再把画布**裁到实际
用量**。两种布局，`shelf` 是默认：

| 算法 | 需要的边长 | 实际用量 | 占用率 |
|---|---|---|---|
| `guillotine`（pixelliner4j `Algorithm.packRectangles` 逐字移植） | 8192 | 8192×4462 | **20.4%** |
| `shelf`（默认，按高降序逐行） | 4096 | 4089×2090 | **91.9%** |
| skyline bottom-left（仅测过，未实现） | 4096 | 4096×2126 | 90.2% |

数据在 `noel` 图集 0（1505 个 sprite，内容 7.46 M px，含 margin 预留 7.86 M px）上实测。
游戏本体那张是 4096×4096 / 44% —— PixelLiner 的正式打包器远强于 pixelliner4j 的
`Algorithm`（后者只是重排用的凑数实现，二叉节点树把 `right` 子节点高度钉死成第一个图块
的高度，横向一路铺到底才换行）。`--packer guillotine` 保留仅作对照/复现。

### 替换图怎么对上 sprite（🔴 踩过的坑）

`noel` 里有 **617 个** key 的层名都叫 `Layer`（导出为 `Layer.EDI*.png`），所以**只按层名
匹配是错的**：一张 `Layer.png` 会被贴到完全不相干的 sprite 上，尺寸一变还会把继承 UV 的
PARTS 图集整个撑错位（实测 26×36 被撑成 60×153）。现在复刻 `sprites` 的去重命名
（`claim_stem`：同名层只有第一个能拿裸名，后面的都带 key），按下序查找，且**一张图只能被
一个 key 认领**：

1. `sprites` 导出的名字（`Layer.EDIba8e8_x.png` / `Layer.a1.png` / `EDIxxx.png`）
2. `<层名>.<key>[.a<i>]`
3. `<key>[.a<i>]`
4. `<层名>[.a<i>]`（兜底，同一次运行只兑现一次）

目录里没被认领的 PNG 原样报在 `unmatched_replacements` —— 名字写错时不能静默丢。
`--pose` 只用于复现 `sprites --pose` 的命名，不影响裁切范围。

### 改尺寸 vs 双图集

同尺寸重着色零风险：rect 不变，PARTS 图集继续用它自己的图。**改尺寸**时 PARTS 图集共用
同一张 UV 表，rect 必须与来源逐块一致，于是：没给 `.a<i>.png` 就跟着换成同一张
（`carried_from_source_atlas`）；给了但尺寸不符就裁/补到 rect（`fitted_to_inherited_rect`）。

### 实测

| 项 | 结果 |
|---|---|
| `pack`（无替换） | shelf 边长 4096，画布裁到 4089×2090，3010 sprite 逐像素自检通过，27 s |
| 重排后 `render` `stand d0 f2 @2x` | 与原表渲染 **md5 逐字节相同**（`3705d8aa…`） |
| `pack --replace` 同尺寸重着色 | atlas 0 `replaced 1` / atlas 1 `replaced 0`（PARTS 保住自己的图），自检通过；渲染差异恰好是 120×306（=60×153 @2x）且**全部变红，其余零变化** |
| `pack --replace` 改尺寸（30×80） | atlas 1 `carried 1 / fitted 1`，自检通过，无报错 |
| `animate` | `stand d0`：`frame_count 12 / loop_to 0 / ticks_per_loop 120`；`--ticks 130` → `position 1, looped 1`（正确回绕）；落点帧渲染与 `render` 同帧 md5 一致 |
| `render --parts` | noel `stand d0 f2 @2x`：几何与默认渲染完全一致（同 pose 同 silhouette），opaque 像素 100% 换成 texture_1 的部件版平色贴图 —— 撕破/换装差分可见 |

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

### Texture2D 解码（atlas.rs）

🔴 **Unity `TextureFormat` 编号必须查权威表**，猜错一个就是全库噪点。本作实际用到的：

| 编号 | 格式 | 本作张数 |
|---|---|---|
| 3 | RGB24 | 1 |
| 4 | RGBA32 | 5 |
| 7 | RGB565 | 6 |
| **12** | **DXT5 / BC3** | **71** |
| 25 | BC7 | 39 |
| **29** | **DXT5Crunched** | **11** |

两个曾经踩过的坑：**12 是 DXT5 不是 DXT1**（DXT1 才是 10）；**29 是 DXT5Crunched 不是 ASTC**
（ASTC 是 48..=55）。早期版本整表错位，结果 71 张主力图集全解成斜纹噪点。

另外三条：

- **行序必须翻转**：Unity 的 `Texture2D` 像素自底向上存（OpenGL 约定），导出 PNG 前要
  `flip_rows`（UnityPy `get_image_from_texture2d(flip=True)` 同）。漏这一步画面上下颠倒。
- **Crunched 体积不可预测**：28/29 的 `m_StreamData.size` 远小于 `w×h`（512×1024 只有 ~100 KB），
  是变长压缩流，只能 `texture2ddecoder::decode_unity_crunch` 直解。
- **体积照妖镜**：`m_CompleteImageSize` 必须等于 `image_byte_size()`（基础层）或
  `image_mip_byte_size()`（含 mip 链，每层宽高减半到 1×1、按块大小 `div_ceil` 取整）。
  133 张真值零例外；对不上就是格式认错了。`laic tex` 用 `size_basis`
  （`base` / `mips` / `compressed` / `mismatch`）把这个判断直接暴露出来。

解码结果与 UnityPy 1.25.3 逐字节一致（DXT5 / BC7 / DXT5Crunched / RGBA32 / RGB565 / RGB24
各档抽样验证过）。

### MPCC（MobPCCContainer.readFromBytesFromFile）

```text
byte   占位（游戏读后丢弃，写回恒 0）
string name          u16 长度 + utf-8
string chr_name      同上
byte   占位（同上）
byte   ARMX          部件条目数；0 = 空调色板
ARMX × { pstring key(部件名, u8 长度) | byte ACC操作数 | ACC × 操作 }
操作类型 1 = HSV       → i16 h, u8 s, u8 v, u8 flags
操作类型 2 = TONECURVE → byte 通道数 N, N × ( byte 点数 M, M × (u8 x, u8 y) )
```

字符串两族同 pxls：`readString` = u16 长度，`readPascalString` = u8 长度。

🔴 **三处必须照抄游戏的怪癖 / 语义，否则后面全部条目错位**：

1. **s / v 读入后 `>=127` 再减 256**（`MobPCCHsv.readFromBytesInner`）。
   于是 `0→0`、`126→126`、`127→-129`、`200→-56`、`255→-1`。
2. **操作类型 0 在 `MobPCC.readFromBytes` 里返回 null 被丢弃**，但那 1 字节照吃。
3. **`ACC 操作数 <= 0` 的部件，`SkltPalette` 直接 continue**（key 已吃、操作数已吃），
   条目丢弃。这两处丢弃都计数上报到 `dropped`，此时字节回环必然不成立（`roundtrip_ok=false`），
   如实报告而不是猜。

部件表存 `Vec` 而非 `BTreeMap`：游戏写回走 `X.objKeys`（字典插入序），换成有序表会重排字节。

真机 5 份容器（`NOEL__231022_013830_2` 空表 / `NOEL__darknoel` 9 部件 250 B 最大 /
`noel__231022_013830` / `sub_i__231022_162602` / `sub_i__mzh`）全部逐字节回环通过、零错误。

## 对标：laic 是 pixelliner4j + AICPxlsUnpacker 的超集

`/lyco` 调研阶段读完两个对照项目全部源码（`/tmp/lyco-cmp/`，共 1863 行 Java + 5710 B Python）
后的结论。**功能覆盖逐项 ≥**，且 laic 独占 Unity 侧解码与写回。

| 能力 | pixelliner4j (Java) | AICPxlsUnpacker (Py) | laic |
|---|---|---|---|
| 解析 pxls 六节 | ✓ 结构化 | ✗ 字节模式硬找 `%PACK_SECTION%`…`0000000200`，28 B 一块、3 字节 BE 坐标 | ✓ 结构化 + 128 表逐字节回环回归 |
| 贴图来源 | ✗ 要外部 PNG，试 5 种命名（`Texture_%s.pxls.texture_%d.png` …） | ✗ 要现成的 `texture_0.png` | ✓ **UnityFS + SerializedFile v22 + Texture2D 全解码**（DXT5/BC7/Crunch/BC4/5/ASTC/RGBA32/RGB565/RGB24/ARGB4444/BGRA32），125 包 133 张实测零错 |
| 按 UV 裁 sprite | ✓ `getImageByKey`（内存） | ✓ 落盘但坐标靠硬找 | ✓ `sprites` 落盘，名字取**姿势层名** |
| 合成一帧 | ✓ Java2D，只在其 GUI 里画 | ✗ | ✓ `render` 落盘 PNG/GIF/精灵表，复刻 `PxlMeshDrawer → RotaGraph` |
| 播放头状态机 | ✓ `FrameAnimator` | ✗ | ✓ `animate`（越界 `loopTo` 夹 0，原版会崩） |
| 图集重排 → `.pxl` | ✓ `writePackSection`（`PxlsKiller` 用它批量转档） | ✗ | ✓ `pack`，见上节 |
| 写回 UnityFS | ✗ | ✗ | ✓ `repack`（修 4 处 + UnityPy 复核） |
| 跨表搜姿势 / MPCC / 矢量 / 粒子 | ✗ | ✗ | ✓ `find-pose` / `mpcc` / IMGV / PTCL |
| 双图集（NORMAL + PARTS） | ✗ 压成一张 | ✗ | ✓ 继承语义保留 |
| 保留 IMGS / IMGV / PTCL | ✗ 只写 PACK+POSE | ✗ | ✓ 全节保留 |
| margin 留白 / UV w/h | ✗ / 重算 | ✗ | ✓ 保留，渲染尺寸零漂移 |
| 四端（CLI/TUI/Web/MCP） | ✗ | ✗ | ✓ 同 registry；写类命令 T1（默认 dry-run，自动化面拒绝） |

**laic 刻意不搬的**：pixelliner4j 的两个 Swing GUI（`PixelPreviewer` 动画预览器 /
`PxlsKiller` 批量转档器）—— 前者只是 EDT 定时器里 `animator.step()` + `renderTo`，
laic 的四端是 CLI/TUI/Web/MCP，GUI 由 Web 端承担，输出靠 `render` 落盘。

## 已知限制

- IMGV 矢量的实际绘制消费还没做（`render` 只画 UV sprite；pixelliner4j 同样忽略该段）。
- 11 张 DXT5Crunched 事件图走 `decode_unity_crunch` 直解；`tex` 仍对未知格式只列清单不强解。
