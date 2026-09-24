# lbin — 二进制 / 容器结构查看使用文档

`lilyco-binfmt` 是 lilyco 框架的**第二个纯只读域二进制**：一个 `lbin` 挂「看一个文件到底是什么结构」
这一整域的 13 条命令，照 `lilyco-files` 的样板获得 **CLI / TUI / Web / MCP** 四端 + AI 可调用。

- 仓库：`https://github.com/lilyco-42/lilyco`
- 二进制名：`lbin`
- 依赖：`object`（符号层）+ 自己写的字节走查（识别 / 成员表 / 分区图），**不执行、不解压、不写盘**
- 安全级：**13 条命令全是 T0 只读**，MCP 的 `DenyElevated` 策略下照样放行

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
| `office-text` | **T0** 只读 | 文件里写了什么：docx 按段、pptx 按页与备注、xlsx 与 **ods** 按格子（表名 + A1 位置 + 值类型）、odt/rtf 各按自己的段落口径；docx 还读批注 / 脚注 / 尾注 / 页眉 / 页脚那几个部件，odt 的页眉页脚在 `styles.xml` 的 master-page 里，odt 的批注嵌在正文段里面，rtf 的页眉与正文混在同一个流里（靠 `\header` 那些目标群分开），每条都带 `from`/`part`/`author`/`date`；rtf 的注是同一件事的第三种存法，也是最坑的一种 —— LibreOffice 把脚注与尾注**都**写成 `{\*\footnote …}` 群，尾注只多一个群里的 `\ftnalt`，而 `\*` 的本意是「不认识后面那个目标群才整群跳过」，一见 `\*` 就跳会把整份文件的注全丢掉（`notes-end.rtf` 就是这条的证据；`{\*\ftnsep\chftnsep}` 那两条是注的排版定义，不是一条注）；rtf 的**批注**又是另一种分法：作者那一格在注之前（`{\*\atnauthor 名字}`），正文在 `{\*\annotation …}` 那一格里，所以两条列表按文件的顺序配、两个条数各交一份（配不上时看得出来），注自己带一个号（与锚区两头 `atrfstart` / `atrfend` 同一个数，`anchor` 就是它）；那一群写的 `atndate` 两份件都对不上同一批字的 docx 里那个 `w:date`（按 epoch 解一个得 2042、一个得 2025），所以 `date` 交 null、原样那串交在 `date_written` 里；第二个作者的中文名「刘奇」是 **LibreOffice 自己的 RTF 导出写不出来**（成了两个问号），而它自己的 docx 导出照抄 —— 两家各按各的文件交；**pdf 也答这一问**（`kind=pages`）：内容流解压、字形码经 `/ToUnicode` 认字、行按文本位置重排（`BT` 复位矩阵、前进量只走 x、`TJ` 那个数符号相反），所以中文标题回来的是字句而不是被排乱的字序；加密的那份不给正文，只说解不出来 |
| `office-meta` | **T0** 只读 | 文档属性那份账：`docProps/*`、ODF 的 `meta.xml`、遗留格式的 OLE 属性集 |
| `office-doc` | **T0** 只读 | Word 与 ODT 的结构：段落/标题层级/样式/表格行列/超链接/脚注尾注批注/修订计数（脚注与尾注部件里那两条分隔符都不算注：`notes-foot.docx` 四条 `w:footnote` 读成两条，`notes-end.docx` 三条 `w:endnote` 读成一条 —— 后一份的部件是 LibreOffice 的 docx 导出器写的；ODF 那侧同笔账走 `text:note-class`，`notes-end.odt` 也是 2 脚注 + 1 尾注）；`revisions` 是那份**修订账**——逐条给类型（插入 / 删除 / 改格式 / 移动）、作者、时间、第几段与那几个字，并把相邻同（类型+作者+时间+段+是否段落标记）的元素合成一条逻辑改动（LibreOffice 写 OOXML 会把一次插入拆成两个 run，元素数与条数因此不同，而它自己导出的 ODF 就是一个 region —— 这条合成规则是拿同一批字的三副账对出来的）；ODF 那侧的删字住在 region 里、插字住在正文那两个标记之间，两处都读；`text:tracked-changes` 里那份段不算正文（那是被删掉的字）；遗留 .doc 给 null，红线条在表流里；`contents` 答「有没有目录、收了几级」—— 三家存法没有共同点所以各报各的：OOXML 是 `w:sdt` + `docPartGallery="Table of Contents"` 那层壳**加上**域指令文字里的 `\o "1-2"`（壳可能整个没有，只剩域指令，两种都找；LibreOffice 还把引号写成 `&quot;`，不还原实体就取不到值），ODF 是 `text:table-of-content`（名字在 `text:name`、级别在 source 的 `outline-level`），而 LibreOffice 会把十级条目模板全写出来 —— 「有几个模板」不等于「收了几级」，两个数都报；第三种写法是 RTF（`toc.rtf`）：那一条流里既没有壳也没有属性，只有 `{\*\fldinst { TOC \\o "1-2" \\h}}` 一条域，开关前面的反斜杠**成对写**（单个会开出控制字），前瞻解一遍之后交出来的 `TOC \o "1-2" \h` 与 OOXML 那条 `instrText` 逐字相同 —— 所以「几级」这把读取器两家共用一把，而 OOXML 专属的 `galleries` / `sdt` 两键在 RTF 那边干脆不出现（不造假）；三份件一起看：`toc.docx` / `toc.odt` / `toc.rtf`；没有目录报 `present: false`，不是缺键；`statistics` 回答「多少字多少页」—— 自己数的与生产者自报的并排（词只按空白切，所以叫 `words_by_space`）；.odt 那份还连带把生产者自己写在 `meta.xml` 的段落数页数一起交出来；**RTF 这一支答的是「一条流能证明什么」**：段就是 `par` 控制字切出来的行，脚注与尾注按目标群分开数（尾注是群里另带 `ftnalt` 的那一条），此外只交 `pictures` / `embedded_objects` / `skipped_destinations` / `note_destinations` / `page_destinations`，表只交**行数与格子数**（就是 `row` / `cell` 这两个控制字的条数，两份件上与 docx、odt 两副账一字不差；`trowd` / `intbl` 两个原始条数与嵌套表的两个另给），而 `tables` 留 null —— 「连续的 trowd 算一张表」这条规则在一张表与两张表的对照件上试过，两张数成了一张，判不住的推断就不报；域的总条数另给 `fields`；**链接也从这里读**：`{\field{\*\fldinst HYPERLINK "地址" }{\fldrslt 显示文字}}`，看到 `field` 时往群里**前瞻**一眼（不推进游标、不改 skip），所以显示文字照旧留在正文里，地址与同一批字的 docx 那一份一字不差（站外与否按地址含不含 `://` 说，这一族没有关系表可查）；**字体与样式同理**：`{\fonttbl…}` 与 `{\stylesheet…}` 仍然整群跳过（一个字不进正文），只是跳之前把里面的定义读一遍 —— `styles` 是「这个样式被几个段落用了」数出来的（样式表自己那 77 条定义不算使用），`font_list` / `style_definitions` / `font_definitions` 交定义本身；试过把那两群改成「认识的群」整群吃掉，那会让 `skipped_destinations` 从 65 掉到 23，所以不改；条目写 `\fcharsetN`（N≠0）而名字含非 ASCII 码位的，交 `name: null` 加那个字符集号（cp1252 硬解会得出「‚l‚r ƒSƒVƒbƒN」这种看着像字的乱码，文件写的是 Shift-JIS）；**标题看样式名**：段属性里的 `\sN` 到样式表里查到名字，名字写成 `heading N`（`heading` 与数字之间至少一个空格、数字后面不能有字、这个词不分大小写）的那一段才算标题，层级就是 N —— 与 docx（`w:pStyle`）和 odt（`outline-level`）那两本账同一个形状，`notes.rtf` 与 `tables.rtf` 各两级、与同一批字的 docx 一字不差，全用 `Normal` 的 `notes-end.rtf` 交回空数组（这一支看过样式表，「没有」不是「没看」）；`\sN` 与 `\csN` 是两个各自的编号空间，字符样式的同号名字不能顶掉段落样式的；**表格的那张网**（`tables[].grid`）走直接孩子，交「这张表自己几行、每行几个格、每格的字与合并」—— 合并两家写法不同：OOXML 用 `w:gridSpan` 且**不写**被合掉的那一格（同一行 2 个 `w:tc`），ODF 用 `number-columns-spanned` 并把被盖住的那一格照样写成空的 `covered-table-cell`（同一行 3 个格）；纵向合并 OOXML 两头都写（`vMerge` 的 restart / continue），ODF 只在起头那格写 `number-rows-spanned="2"`。所以「这一行几个格子」是**存储的数**，不是页面上那张表的数（出处：`tables-merged.docx` 与它的 `.odt`）；`tables[].rows` / `cells` 是另一本账（`descendants` 数的，嵌套表算进来），两本都交；**那张纸**（`page_setup`）是一份三家对照账：docx 与 RTF 写 twips（1/1440 英寸）、odt 写 `21.59cm` 这种自带单位的串，全部换成 **0.01mm 的整数**（同一条整数式子、逢半进一，不用浮点，免得两个读者在最后一位上分家），每条还带 `written`（文件自己写的那一串），一节一条（`notes-hf.docx` 两节就两条）；odt 里那条只写网格设置的占位页布局不是一张纸，也不占序号；RTF 只交文档级那一条 —— 某一节的覆写住在 `{\*\sectx}` 群里，而这一族不判分节归属；五份件的十四对「文件 × 家」换算出来的纸面都是 21590×27940（Letter），而 `notes-hf` 的四边只有两家一致：docx 上下写 1440 twips，LibreOffice 的 odt 与 rtf 两个导出都写 720 / 1.27cm，三个数都报，不挑一个当准；第二份尺寸的对照件：A4 的短边三家都换算成 **21001**（11906 twips / `21.001cm`），也就是**谁都够不着「210×297」那个整数** —— 所以这一支不报纸张叫什么名字（开了容差就得回答「JIS B5 与 ISO B5 差 4mm 算不算同一张」），把数交出去让人自己认；那份件的横排第二节在 docx 与 odt 里各写一条（`orient="landscape"`、宽高对调），而 LibreOffice 的 **RTF 导出整份一个 `\landscape` 都没有** → 那一副只有一条文档默认的纵向；方向只交文件写了的（docx 与 RTF 竖排时干脆不写 → null，odt 明写 `portrait`）；.doc 交 null（节属性在表流里）；**断点按词边界数**（`structure.break_words` 六个固定键：par / line / page / pagebb / pbb / sect，没出现也交 0）—— 子串数会骗人（`\pard` 含 `\par`、`\sectd` 含 `\sect`），而且只在没被跳过的那一层数（页眉里的 `\par` 不是正文的一段）；换页在这一族有三种写法，Word 那条 `w:br w:type="page"` 在 LibreOffice 的 RTF 导出里是 `\pagebb`（七份 RTF 件的 `\page` 全是 0），所以 `page_breaks` 交三种词的和；**ODF 的换页不写在正文里**，写在段落点名的那个自动样式上（`fo:break-before="page"`，两跳，与 .ods 的数据样式同一类）—— 以前这一条数的是 `text:soft-page-break`（渲染时落下的位置，七份件里一个都没有），四份明明换了页的 odt 就一起报成 0 了；现在 `page_breaks` 走样式那一跳、`soft_page_breaks` 保留原来那条数，`notes` 与 `toc` 两份件三家（docx / odt / rtf）都报 1；`\sect` 是收尾符（最后一节不带），所以只交 `section_breaks`，`sections` 仍留 null；RTF 那一支的批注是 `{\*\annotation …}` 那一群的条数（作者与字在 `office-text` 里逐条交），修订与保护仍交回 null（目录也不再属这一类：那条流里的 `TOC` 域数得清，见上面 `contents`）—— null 是「这一支没看」或「判不住」，0 才是「这份文件没有」 |
| `office-sheet` | **T0** 只读 | 表格的结构：每张表（含隐藏的）与范围、格子与公式、合并格、命名区域、外链；xlsx 每个格子还带**数字格式**（`s=` 是 `cellXfs` 的下标）与换算出来的日期；.ods 走它自己那套（值类型 + 重复计数累加出的格子位置）；`protection` 那份账分两层（工作簿的 `workbookProtection` 与每张表的 `sheetProtection`），两种布尔拼法都认 —— openpyxl 写 `sheet="1" formatCells="0"`，LibreOffice 重写同一份东西写 `sheet="true" formatCells="false"` 并把等于默认的省掉，而 LibreOffice 导出 xlsx 时会把 `lockStructure` 丢成一个空元素（fixture 实测）；空的 `<workbookProtection/>` 报成「元素在场、没说锁」；`.xls` 没有「工作簿一层 + 表一层」那种分法：`PROTECT`(0x0012) / `PASSWORD`(0x0013) / `SCENPROTECT`(0x00DD) 三条写在**被锁那张表自己的子流**里，所以按表交账、`records` 留原值（0x00DD 那条没有第二个读者认得，只给数值、不替它编开关名；0x0013 那格是 Excel 的 16 位旧哈希，不是口令）；隐藏行与隐藏列在 .xls 里是第四种存法 —— 藏在 `ROW`(0x0208) 的 `0x20` 位与 `COLINFO` 的第 0 位（LibreOffice 给这条记录用的是老 id 0x007D），那一位是拿「只改行高」与「只藏一行」两份对照件拆开量出来的，四种写法必须报同一个数）；`comments` 那一份要走两跳才找得到部件 —— 批注不在 `sheetN.xml` 里，而在**这张表自己的关系表**指着的那个部件，openpyxl 放 `xl/comments/comment1.xml`（Target 绝对）、LibreOffice 放 `xl/comments1.xml`（Target 是 `../comments1.xml`），作者名还只是 `<authors>` 列表的下标；ODF 的 `office:annotation` 直接坐在格子里面，所以格子的字要把注的子树跳过（`cell-notes.xlsx` / `-lo.xlsx` / `.ods` 三份件按格子配对逐条对）；`.xls` 是第四种存法，**部件都不换**：一条记录给字（正文偏移 10 自报字数，紧跟的第一条 CONTINUE 首字节是编码旗标 —— 0 一格一字节、1 一格两字节，与 BIFF8 的 `fCompressed` 惯例相反），另一条给「哪个格子 + 谁写的」（住在该表子流末尾），两份列表按出现顺序配、每条带 `whole`、两类记录的条数一起交（`cell-notes-many.xls`：一次只改一个变量 —— ASCII 与中文作者名、2 与 3 个字、带换行的字、`AA100` 两位列名、两张表）；这一族不写作者时间，`date` 一律 null，那两个记录号也不替它们编规范名（MS-XLS 把 0x001C 留给 EXTERNSHEET，这里量到的是「格子 + 作者」）；`--csv [--sheet 名字或序号]` 另交一份 RFC4180 铺平的网格（日期给 ISO、没缓存值的公式格给空；`.xls` 也走同一跳 —— 格子的 `ixfe` 是 XF 记录的出现序号，XF 的格式号在正文偏移 2，自定义号的串在 FORMAT 记录里、内置号查那张内置表，日期基准取 DATEMODE，文件没写 DATEMODE 就交回序列数而不是默认 1900）；.ods 每格还带它继承的**数据样式**（`format_kind` / `decimals` / `format_tokens`，样式分在 content.xml 与 styles.xml 两处） |
| `office-slide` | **T0** 只读 | 演示文稿的结构：放映顺序、每页标题与**演讲者备注**、版式与母版、尺寸与媒体；pptx 与 odp 各按自己的层级走（odp 的尺寸要绕 master-page 那一跳）；遗留 `.ppt` 是一棵 PowerPoint 97 记录树，按 `recType 0x03EE` 的容器归页（一页一个，各页的行、原子数与那条记录在流里的偏移都交出来）—— 这条归属是拿同一份文件的 pptx 逐张对出来的，规范文本手上没有所以只报数值，`SlideContainer`（11 个）那种名字不拿来当页数 |
| `office-package` | **T0** 只读 | 包自证：关系指着不存在的部件、部件没声明内容类型、解压过不了自己的 CRC-32 |
| `office-objects` | **T0** 只读 | 正文之外装了什么：图片、嵌入对象、字体、自定义 XML + 该留心的宏/外链/加密/签名；ODF 没有 OPC 关系表，引用按 `xlink:href` 逐条扫（带 scheme 的才算站外），宏那一条只判 OOXML/复合文档 —— 手上没有含 Basic 库的 ODF 样本 |
| `office-pdf` | **T0** 只读 | PDF 这张对象表：几页（`/Count` 与真的 `/Type/Page` 对账，`/Pages` 是树节点不算页）、每页尺寸与旋转（**页上不写就沿 `/Parent` 继承**）、Info 元数据（PDF 字符串那三件事：转义、八进制、嵌套括号；`<FEFF…>` 是 UTF-16BE）、tagged 标志、字体清单（含 `/ToUnicode` 有没有、这个对象是不是藏在对象流里）、图片清单、以及会自己动的东西（`/JavaScript`、`/Launch`、`/SubmitForm`、`/AcroForm` 字段、`/EmbeddedFiles` 附件、`/OpenAction`、页 `/AA`）。对象靠扫 `N G obj` 认，**不追交叉引用表**，但补了两层：`/Type /ObjStm` 里打包的对象与「没有 `trailer` 这个词、trailer 的键住在 `/Type /XRef` 流字典里」的文件。加密的 PDF 只报结构与加密参数，**不解密**，元数据与 `/Lang` 宁可给 null 也不交乱码。还交「去哪儿」这三份账：`outline` 书签树（`/Outlines` 的 `/First` → `/Next`，子层再 `/First`；每条给标题、层级、目标页对象与它是第几页、根上自报的 `/Count`，`/Count` 的符号就是展开/折叠）、`links`（每张 `/Subtype /Link` 注记分三类：站外 URI、页到页、以及 `/Launch` 那种哪儿也不去的）、`permissions`（`/Encrypt` 的 `/P`，**带符号**的整数，位号从 3 起，R3 才有 9~12 位，R2 的那几位给 null）；加密件里数字与名字不是密文，所以页号与权限位照报，标题与 URI 给 null。`--text` 另交一份正文：按 `/Kids` 的页序、字形码经每张字体的 `/ToUnicode`（`bfchar` 与两种写法的 `bfrange`）、行按位置重排，交回 `{ order_from_page_tree, chars, lines, pages, text }`；**位置这一层有三条规则**，错一条就是「字都认得、顺序全乱」：`BT` 把文本矩阵复位、字形前进只加在 x 上、`TJ` 数组里的数把笔往**反**方向推。不含：表单字段值、签名校验、加密文件的正文（内容流是密文）、图形流里的字与 CID 字体的宽度（在 `/W` 里，不跟） |

十三条命令都要 `--path`（`must_exist`）。规模控制两个开关：
`--max-bytes`（默认 `identify` 1 MiB、`regions` 32 MiB、`entries`/`symbols` 与 `office-*` 64 MiB；
**写 0 = 不设上限**）与 `--limit`（`entries` 默认 200、`symbols` 默认 128、
`office-sheet`/`office-package`/`office-pdf` 200、其余 office 命令 100；`regions` 上限固定 256 区；
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
16 份 fixture 全部由**独立生产者**写出（python-docx / openpyxl / python-pptx / Pillow / LibreOffice），
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
