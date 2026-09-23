# lbin — 二进制 / 容器结构查看使用文档

`lilyco-binfmt` 是 lilyco 框架的**第二个纯只读域二进制**：一个 `lbin` 挂「看一个文件到底是什么结构」
这一整域的 4 条命令，照 `lilyco-files` 的样板获得 **CLI / TUI / Web / MCP** 四端 + AI 可调用。

- 仓库：`https://github.com/lilyco-42/lilyco`
- 二进制名：`lbin`
- 依赖：`object`（符号层）+ 自己写的字节走查（识别 / 成员表 / 分区图），**不执行、不解压、不写盘**
- 安全级：**4 条命令全是 T0 只读**，MCP 的 `DenyElevated` 策略下照样放行

---

## 安装

```bash
cargo binstall lilyco-binfmt            # 暂时只会回退源码（该 crate 未发布，见 docs/INTEGRATION.md §0）
cargo install --path lilyco-binfmt      # 源码编译
cargo run -p lilyco-binfmt -- identify --path /usr/bin/ls --json
```

---

## 命令一览

| 命令 | 安全级 | 干什么 |
|---|---|---|
| `identify` | **T0** 只读 | 靠文件自己的字节说清是什么族、什么格式，并摊开它头部自报的字段 |
| `entries` | **T0** 只读 | 列压缩包/归档的成员表：ZIP（含 APK/JAR/DOCX/EPUB）、tar、ar（含 .deb） |
| `regions` | **T0** 只读 | 把整个文件按区上色：头部 / 表 / 代码 / 数据 / 只读 / 元数据 / 空闲 / 尾部叠加 |
| `symbols` | **T0** 只读 | 按分析器的读法看一个目标文件：节表 + `.symtab` 与 `.dynsym` + 地址到名字 |
| `office-info` | **T0** 只读 | 这份办公文件是什么（OOXML / ODF / 复合文档 / RTF）、谁写的、有没有宏与加密 |
| `office-text` | **T0** 只读 | 文件里写了什么：docx 按段、pptx 按页与备注、xlsx 按格子、odt/rtf 各按自己的段落口径；docx 还读批注 / 脚注 / 尾注 / 页眉 / 页脚那几个部件，每条带 `from`/`part`/`author`/`date` |
| `office-meta` | **T0** 只读 | 文档属性那份账：`docProps/*`、ODF 的 `meta.xml`、遗留格式的 OLE 属性集 |
| `office-doc` | **T0** 只读 | Word 与 ODT 的结构：段落/标题层级/样式/表格行列/超链接/脚注尾注批注/修订计数；.odt 那份还连带把生产者自己写在 `meta.xml` 的段落数页数一起交出来 |
| `office-sheet` | **T0** 只读 | 表格的结构：每张表（含隐藏的）与范围、格子与公式、合并格、命名区域、外链；xlsx 每个格子还带**数字格式**（`s=` 是 `cellXfs` 的下标）与换算出来的日期；.ods 走它自己那套（值类型 + 重复计数累加出的格子位置） |
| `office-slide` | **T0** 只读 | 演示文稿的结构：放映顺序、每页标题与备注、版式与母版、尺寸与媒体 |
| `office-package` | **T0** 只读 | 包自证：关系指着不存在的部件、部件没声明内容类型、解压过不了自己的 CRC-32 |
| `office-objects` | **T0** 只读 | 正文之外装了什么：图片、嵌入对象、字体、自定义 XML + 该留心的宏/外链/加密/签名 |

十二条命令都要 `--path`（`must_exist`）。规模控制两个开关：
`--max-bytes`（默认 `identify` 1 MiB、`regions` 32 MiB、`entries`/`symbols` 与 `office-*` 64 MiB；
**写 0 = 不设上限**）与 `--limit`（`entries` 默认 200、`symbols` 默认 128、
`office-sheet`/`office-package` 200、其余 office 命令 100；`regions` 上限固定 256 区；
`office-text` 还有一个 `--max-chars`，默认 20000）。
`--json` 出结构化结果，人读格式在结果超过 500 字节时只报进度——**给脚本和 AI 用请加 `--json`**。

> Web / MCP 端不带这些参数时传进来的是 0（`#[arg(default)]` 只在 CLI 生效）。这两个 0 的含义**不一样**，
> 而且不能混：`--max-bytes 0` = 不设上限（`read.rs::read_blob` 里定的），`--limit 0` 与
> `--max-chars 0` = 按上面的缺省值处理（`opack::take_limit` 里定的）。把 0 当成「只列一条」
> 会交出被悄悄砍短的答案，四端还会给出长短不一的同一张表；反过来把「不设上限」当成缺省值
> 会让一个 8 GB 的文件直接进内存。`office_text.rs` 与 `opack.rs` 的测试各钉了一条。

---

## 快速上手

```bash
lbin identify --path app.apk --json          # 是 APK 还是别的？头字段怎么说？
lbin entries  --path app.apk --limit 200     # 中央目录列出的成员
lbin regions  --path a.out --json            # 哪些字节是代码/数据/表，哪些没人指
lbin symbols  --path /usr/bin/ls --json      # 节表 + 两张符号表 + 地址→名字
lbin --schema                                # 注册表清单（四端同源的那份）
```

`identify` 的两档置信度是**这域的关键设计**：

- `signature` —— 开头几个字节就定死了（ELF/PE/PNG/…）。
- `structural` —— 魔数之外还得让某张表刚好铺进文件才敢这么说。
  例：`0xCAFEBABE` 同时是 Java class 与通用二进制的魔数，只有当架构表每一项的
  偏移与长度都落在文件内、且条数 ≤ 64 时，才叫它 universal binary。

`entries` 每条答案都带 `checks: [{claim, ok, note}]`：文件自报的东西要自己圆得回来。
中央目录说 3 条而实际只有 1 条，就报 `ok: false` 并说清差在哪，而不是默默少报。

`regions` 的 `totals` 必须自洽：`claimed + unreferenced + loaded_unaddressed == 读进来的字节数`。
`gap`（绿色）只表示「没有表也没有加载段点到这里」，**不是「改这里安全」**——校验和、签名、
自读取的程序都不会在表里留下痕迹。没实现的族一律 `mapped: false` + 原因，不编区间。

---

## 覆盖范围（说清楚边界）

| 命令 | 现在能用 | 现在不用 |
|---|---|---|
| `identify` | ELF 32/64、PE、DOS/MZ、Mach-O（thin + fat）、DEX、ZIP/APK、tar、ar、PNG/JPEG/GIF/BMP/WebP/RIFF、TIFF、PDF、WebAssembly、Java class、SQLite、CAB、7z、RAR、xar、EBML/Matroska、ISO BMFF（按 brand）、gzip/bzip2/xz/zstd/LZ4、WOFF/WOFF2、RPM | 内容级解析（只做结构） |
| `entries` | ZIP 家族（方法/CRC-32/两个尺寸/局部头偏移）、tar（typeflag/mode/uid/gid/mtime + 八位校验和自证）、ar（含 `.deb` 的 `` ` ``+换行命名） | 压缩流的解压 |
| `regions` | ELF（节 + 加载段 → 区分对齐填充与真空闲）、PE（段表 + 证书目录）、Mach-O（节表/符号表/间接符号表/重定位，端序由头的字节排列决定）、PNG（块表 + 真算的 CRC） | 其他族给 `mapped: false` + 原因 |
| `symbols` | 走 `object`：ELF/PE/Mach-O/COFF，节表 + `.symtab`/`.dynsym` + 地址到名字索引 | 反汇编、控制流（那是另一个域） |
| `office-*` | OOXML（docx/docm/xlsx/xlsm/pptx/pptm）、ODF（odt/ods/odp）、MS-CFB 遗留（doc/xls/ppt）、RTF；OPC 的关系表与内容类型；OLE 属性集；RTF 的 `\info` 群与 `\*\userprops`；`.doc` 的 piece 表；`.xls` 的 BIFF8 记录（格子按 BOUNDSHEET 偏移归位到每张表）；`.ppt` 的 PowerPoint 97 记录树（文本原子 0x0FA0 / 0x0FA8 / 0x0FBA） | `.ppt` 的按页归位（要 SlideContainer 与 SlidePersistAtom 配对）、宏内容的解析（只检测宏部件）、密码学验证（签名只看有没有，不验签） |

office 这一摊的证据制度在 [`lilyco-binfmt/tests/fixtures/office/README.md`](../lilyco-binfmt/tests/fixtures/office/README.md)：
15 份 fixture 全部由**独立生产者**写出（python-docx / openpyxl / python-pptx / Pillow / LibreOffice），
Rust 测试里的每个期望值都来自第二读者（`scripts/acceptance/office_reader.py` + `lyco_rtf.py` +
`lyco_legacy.py`，只用 Python 标准库）对同一批文件的读取；CI 的 `apps` job 还会把编出来的 `lbin`
与那位读者逐字段对账（`office_probe.py`），不一致就红。

Mach-O 的两处坑已经用真文件钉住（`lilyco-binfmt/src/regions.rs` 的测试）：
节名/段名是**定长 16 字节**，`__compact_unwind`、`__gcc_except_tab` 正好占满、没有结尾符；
`S_ZEROFILL`（`__bss`/`__common`）的 `offset` 是 0，在文件里没有字节，不能画成数据区。

---

## 四端与扩展

```rust
// lilyco-binfmt/src/main.rs —— 与 lilyco-files 同形状
let reg = build_registry_with_policy(policy_for(backend));
lilyco::run_registry_with("lbin", reg, backend)
```

- MCP：`lbin --mcp` → `tools/list` 返回 12 个工具，参数由 `CommandSchema::validate_args` 统一校验
  （缺 `path` 直接被拒，错误信息里带字段名）。
- Web：`lbin --web` → 路径输入框旁边有「…」按钮，点了弹**系统文件选择框**，选完自动回填路径
  （框架能力，见 `readme.md` 的 `/pick`；不用浏览器 `<input type=file>` 是因为它给不出真实路径）。
  对话框可能被压在浏览器后面（Windows 不让后台进程抢前台），所以它会闪任务栏，
  页面上也会写着「看任务栏」，选完提示自动消失。
- 加一条命令：新模块 + `#[derive(App)]` → `main.rs` 的 `for c in [...]` 里加一行，四端自动获得。
- 加一个能画区的族：`src/regions.rs` 加 `xxx_spans()` 并在 `run_regions` 的 match 里加一臂，
  同时补一条「图必须铺满读进来的字节」的测试。
