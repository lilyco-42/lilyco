# `lbin office-*` 的 fixture：每一份都有署名的生产者

这些文件**不是手搓的字节**。断言如果来自我自己写的字节，那就只证明了「我写的东西能被我自己读回来」，
不证明能读真实世界里的办公文件。所以这里的每一份都由一个独立程序写出，
重跑 `python scripts/office_fixtures.py --force`（需要 LibreOffice；python-docx / python-pptx /
openpyxl 装在 `D:/app/scoop/apps/python/current/python.exe` 那套解释器里，PATH 上的 uv python 没有）。

| 文件 | 生产者 | 里面故意放了什么 |
|------|--------|------------------|
| `notes.docx` | python-docx 1.2 | 两级标题、正文、2×2 表、站外超链接、一张 PNG、一条批注、分页符、核心属性 + 自定义属性 |
| `notes-hf.docx` | python-docx（`write_header_docx`） | 两个节、两份不一样的页眉 + 一份页脚：`word/header1.xml`、`word/header2.xml`、`word/footer1.xml`（第二节要显式断开链接才会多出第二个页眉部件）；正文 5 段里 2 段是空的 |
| `notes.docm` | 由 `notes.docx` 加部件 | `word/vbaProject.bin` + `vbaSignature.xml` 与对应内容类型：**合成**的宏样本（见下） |
| `book.xlsx` | openpyxl 3.1 | 三张表（其中一张 hidden）、公式、合并格、命名区域、表格部件、中文表名 |
| `formats.xlsx` | openpyxl 3.1（`write_formats_xlsx`） | 日期格的 `s=` 指向 `cellXfs` 的**下标**而不是格式号；日期/百分比/货币被写成自定义号 164-168（内置表查不到）；`C5` 的格式串带引号汉字字面量；**`C7` 是长得像日期的文本** |
| `deck.pptx` | python-pptx | 两页：标题 + 正文 + 备注 + 图片；第二页一张 2×2 表；4:3 尺寸 |
| `notes.odt` / `book.ods` / `deck.odp` | LibreOffice（从上面三个 OOXML 文件转来） | 真 ODF 写入者产出的三种 ODF |
| `deck.odp`（结构） | 同上 | 两页：`draw:name` 是「预算评审」与「第二页：数字」；第一页有 `presentation:class="notes"` 的备注（「评审时先讲口径再讲数字」），**旁边还坐着页码占位，里面的样字是 `<编号>`** —— 整页一把抓就会把它当正文；两个母版页名、两个版式名，但文件里没有任何版式定义；尺寸 25.4cm×19.05cm landscape 在 styles.xml 的 page-layout 里 |
| `notes.odt`（结构） | 同上 | 10 段（2 段是空的）、两个带 `text:outline-level` 的标题、1 张 2×2 表（名叫「表格1」）、一条 `text:annotation` 批注、一个 `draw:frame`+`draw:image`、一个 `text:a` 超链接、5 个 `text:sequence-decl`；`meta.xml` 自报 paragraph-count 10 / page-count 2 / word-count 61 |
| `formats.ods` | LibreOffice（从 `formats.xlsx`） | 同一份格式账的 ODF 写法：日期是 `office:date-value` 的 ISO 串、百分比带 `12.5%` 这种显示文本、货币只剩显示里的 ¥；每行尾部 `number-columns-repeated="16381"` 的填空、行首还有重复 2 的空格 |
| `notes-foot.docx` | LibreOffice（从 `office_fixtures.py` 里的 `FOOTNOTE_RTF` 导入写出） | 两条真脚注 + `word/footnotes.xml` 里那**两条分隔符**（`w:type="separator"` / `"continuationSeparator"`，没有正文）：数脚注不能只数 `w:footnote` 元素 |
| `notes-end.docx` | LibreOffice 的 **docx 导出器**（把尾注注进 `notes-foot.docx` 再让它照抄一遍） | 一条真尾注 + 一条脚注：`word/endnotes.xml` 的字节全是它写的（两条分隔符是它自己的 `<w:separator/>` 那一族写法），尾注这一支第一次有件可走 |
| `notes-end.odt` | LibreOffice（从 `notes-end.docx`） | 同一笔账的 ODF 存法：`text:note-class="endnote"` 那条是它的 ODT 导出器写的，编号还换成罗马数字 `i`（`footnote` 仍是阿拉伯数字） |
| `hidden.xls` | LibreOffice（从 `hidden.xlsx`） | 第四种存法：行藏在 ROW(0x0208) 的一个位、列藏在 COLINFO 的一段范围（首末都含），而 LibreOffice 写的 COLINFO 用的是老 record id **0x007D**（MS-XLS 给 BIFF8 的名字是 0x07D0） |
| `cell-notes.xlsx` | openpyxl | 三条表格批注，部件在 **`xl/comments/comment1.xml`**，表的关系用**绝对** Target（`/xl/comments/comment1.xml`）、关系 Id 还不是 rId 而是字面量 `comments`；作者住在同部件的 `<authors>` 列表里，格子上只有 `authorId` 下标 |
| `cell-notes-lo.xlsx` | LibreOffice（`cell-notes.xlsx` → .ods → .xlsx） | 同一批字的另一副面孔：部件在 **`xl/comments1.xml`**、Target 是相对的 `../comments1.xml`、注文字包在 `<r><rPr>…<t>` 里，而且条目顺序都变了（A3 排在 B2 前面） |
| `cell-notes.ods` | LibreOffice（从 `cell-notes.xlsx`） | ODF 的存法：批注是 `office:annotation`，**坐在格子里面**（`dc:creator` 给作者、`<meta:date-string/>` 是空的），一锅端取格子的字就会把注当成这一格的内容 |
| `cell-notes-many.xlsx` | openpyxl | 第四种存法的种子：一次只改一个变量 —— 作者名有 ASCII 的（`AB`）也有中文的、字数有 2 也有 3、注的字带换行、格子拉到 `AA100`、注还分到两张表上 |
| `cell-notes-many.xls` | LibreOffice（从 `cell-notes-many.xlsx`） | 批注的第四种存法：字与「哪个格子 + 谁写的」分在**同一条流**的两类记录上（后者住在该表子流的末尾），编码旗标那一位与 BIFF8 的 `fCompressed` 惯例**相反** |
| `notes-end.rtf` | LibreOffice（从 `notes-end.docx`） | 注的第三种存法：脚注与尾注**都**写成 `{\*\footnote …}` 这一个群，尾注只在群里多一个 `\ftnalt`；分隔符另走 `{\*\ftnsep\chftnsep}` |
| `tables.docx` / `tables.odt` / `tables.rtf` | python-docx 与 LibreOffice（两张表：3×2 与 2×2，中间夹一段正文，首尾各一个标题） | 表那一份的对照件：三家都给 5 行 10 格，而 RTF 只敢给行数与格子数 —— 「几张表」的分组规则在 `notes.rtf`（一张）与这份（两张）上试过，单表对、两表数成一张 |
| `paper-a4.docx` / `paper-a4.odt` / `paper-a4.rtf` | python-docx 与 LibreOffice（A4 纵向一节 + 横过来的一节） | 那张纸的第二尺寸：三家换算到 0.1mm 后短边都是 **21001**（不是 21000 —— OOXML 与 RTF 写 11906 twips，ODF 照抄成 `21.001cm`），所以这一支不给尺寸起名；横排那一节 docx 与 odt 都有第二条并写着 `orient=landscape`，而 RTF 全文一个 `\landscape` 都没有 → 那一条流只交文档默认的纵向 |
| `toc.docx` | LibreOffice 的 **docx 导出器**（把目录注进 `notes.docx` 再让它照抄） | 真目录：`<w:sdt>` + `<w:docPartGallery w:val="Table of Contents"/>`，级别在域指令文字里 —— LibreOffice 把引号写成 `&quot;`，所以 `TOC \o "1-2" \h` 要还原实体才读得对 |
| `toc.odt` | LibreOffice（从 `toc.docx`） | 同一件东西的另一副面孔：`text:table-of-content`（名字 `目录1`）、级别在 `text:table-of-content-source/@outline-level="2"`，另外**十级条目模板全写出来**（`entry_templates` 报的是文件写了几个，不是用上了几级） |
| `notes-hf.odt` | LibreOffice（从 `notes-hf.docx`） | 同一批字的 ODF 存法：页眉页脚在 **styles.xml 的 master-page** 里，两个节 = 两个 master-page（`Standard` 与 `Converted1`），各带一份 header + footer |
| `notes-hf.rtf` | LibreOffice（从 `notes-hf.docx`） | 第三种存法：`\header` / `\headerl` / `\footer` 这些**目标（destination）**里的字，与正文混在一个流里 |
| `revisions.docx` | python-docx + 手注入 `w:ins` / `w:del` / `w:rPrChange` / 段落标记 | 四种修订各一处，而且**没被拆开**：3 个 `w:ins`（含段落标记那一条）、1 个 `w:del`、1 个 `w:rPrChange` → 5 条逻辑改动 |
| `revisions-lo.docx` | LibreOffice（从 `revisions.docx`） | 同一批字的 OOXML 另一副面孔：一次插入被拆成两个 run（数字与单位各一条），段落标记那一条反而被丢掉 → 6 个元素、4 条改动 |
| `revisions.odt` | LibreOffice（从 `revisions-lo.docx`） | 同一批字的 ODF 存法：4 个 `text:changed-region`（2 插 1 删 1 改格式），日期没有那个 `Z`，改格式那条带着被改的字，删掉的那段只住在 region 里 |
| `protected.docx` | python-docx（`notes.docx` 的副本 + 手注入 `w:documentProtection`） | 编辑限制写在 `word/settings.xml`：`w:edit="readOnly"` + `w:enforcement="1"` + 一套 crypt 属性 |
| `protected-lo.docx` | LibreOffice（从 `protected.docx`） | 同一份限制被 LibreOffice 原样重写回来（真生产者也会这么写） |
| `protected.odt` | LibreOffice（从 `protected.docx`） | **同一份内容换了 ODF 就没保护了**：`settings.xml` 里 ProtectForm / ProtectBookmarks / ProtectFields 全是 false —— 那条编辑限制没跟着搬过来 |
| `locked-sheet.xlsx` | openpyxl 的 `book.xlsx` + 手注入 | 两层保护：`workbookProtection lockStructure="1"`（改 openpyxl 留的那个空元素，不是再加一个）与 `sheetProtection sheet="1" formatCells="0" insertRows="1"` |
| `locked-sheet-lo.xlsx` | LibreOffice（从 `locked-sheet.xlsx`） | 同一家族的另一种拼法：`sheet="true" formatCells="false"`、等于默认的开关省掉，而 **`workbookProtection` 被写成了空的**（结构锁丢了） |
| `locked-sheet.ods` | LibreOffice（从 `locked-sheet.xlsx`） | ODF 的表保护是 `table:table` 身上的属性：`table:protected="true"` + `table:protection-key` + 摘要算法那条 URI |
| `locked-second.xlsx` | openpyxl 的 `book.xlsx` + 手注入（锁在**第二张**表） | 与 `locked-sheet.xlsx` 是一组对照，唯一的差别是锁放在第几张表上 |
| `locked-sheet.xls` | LibreOffice（从 `locked-sheet.xlsx`） | BIFF8 的表级保护：`0x0012` + `0x0013` + `0x00DD` 三条，写在**被锁那张表自己的子流**里 |
| `locked-second.xls` | LibreOffice（从 `locked-second.xlsx`） | 对照的另一半：锁挪到第二张，这三条跟着挪窝 —— 「按表记」是这么量出来的，不是按记录名推的 |
| `notes.doc` | LibreOffice（从 `notes.docx`） | MS-CFB 复合文档 + WordDocument 流 + `1Table` 里的 piece 表 |
| `notes-en.doc` | LibreOffice（从纯 ASCII 的 `notes-en.docx`） | 中英一视同仁仍写 16 位 piece —— 记下这个事实，见下 |
| `book.xls` | LibreOffice（从 `book.xlsx`） | BIFF8：BOUNDSHEET（含隐藏表）、SST + CONTINUE、LABELSST / RK / FORMULA |
| `formats.xls` | LibreOffice（从 `formats.xlsx`） | 数字格式那一跳的真件：格子的 ixfe → XF 记录（0x00E0，格式号在正文偏移 2）→ FORMAT 记录（0x041E）；这份里自定义号 165~169 是日期/日期时间/百分比/¥/汉字日期，另有内置号 9 与 41~44 |
| `mulrk.xlsx` | openpyxl | 一行连续的八个数字 + 隔开一行三个 —— 就是为了逼出 MULRK 那种记录 |
| `mulrk.xls` | LibreOffice（从 `mulrk.xlsx`） | BIFF8 的 MULRK(0x00BD)：一行连续格子共用一条记录，每格自己带 `{ixfe(2), rkmac(4)}`；这份件里正好两条（8 格与 3 格） |
| `deck.ppt` | LibreOffice（从 `deck.pptx`） | PowerPoint 97 记录树 + 一个 59 万字节、走 FAT 的属性集流（大流那条分支的样本）；按 `0x03EE` 归出 2 页，与 `deck.pptx` 每页逐张一致 |
| `notes.rtf` | LibreOffice（从 `notes.docx`） | 字体表、颜色表、样式表、`\*\userprops`、域代码与 `\'hh` 回退字节 |
| `hidden.xlsx` | openpyxl 3.1（`write_hidden_xlsx`） | 第 3、4 行隐藏，C/D/E 三列隐藏，**D2/E2 里有字**；一列一条 `<col min="3" max="3" hidden="1">` |
| `hidden-lo.xlsx` | LibreOffice（`hidden.ods` 转回 OOXML） | 同一份账的另一种写法：`<col min="3" max="5" hidden="true">` 一条盖三列，没隐藏的行也写着 `hidden="false"` |
| `hidden.ods` | LibreOffice（从 `hidden.xlsx`） | 隐藏换成 `table:visibility="collapse"`，列那一跳还带 `number-columns-repeated="3"` |
| `notes.pdf` | LibreOffice（从 `notes.docx` 导出） | Writer 那份的 PDF：2 页 letter、5 张子集 TrueType 字体（每张都带 `/ToUnicode`）、2 张 8×8 图、`/Lang (en-US)`、`/MarkInfo /Marked true`、一个 URI 批注、Info 里七个键（`/Title` 是 `<FEFF…>` 的 UTF-16BE 中文） |
| `deck.pdf` | LibreOffice（从 `deck.pptx` 导出） | 同一批字的 Impress 存法：页面 `0 0 720 540`、`/Lang (zh-CN)`、6 张字体、没有批注 —— 与 `notes.pdf` 一起把「不同应用 → 不同页尺寸与语言」钉住 |
| `objstm.pdf` | qpdf 12.3.2（经 pikepdf 10.13，从 `notes.pdf` 再存） | **68 个对象里只有 17 个是明写的**：另外 51 个挤在一个 `/Type /ObjStm`（`/N 51 /First 388`）里；文件里**没有 `trailer` 这个词**，`/Root`、`/Info` 只写在 `/Type /XRef` 的流字典里（`/Size 69`）。只扫 `obj` 的读者会报「0 页」，只认 `trailer` 的读者找不到元数据 |
| `locked.pdf` | qpdf（`Encryption(R=6)`，从 `notes.pdf`） | AES-256 真加密，口令 `lbin-test`（owner `lbin-owner`；这是测试件，口令不是秘密）。`pdfinfo` 不给口令直接 `Incorrect password`；`/Encrypt` 指着的字典是 `/Filter /Standard`、`/V 5`、`/R 6`、`/Length 32`、带 `/O` `/U` `/OE` `/UE` `/P` |
| `perms.pdf` | qpdf（pikepdf，从 `notes.pdf`） | **只设 owner 口令**的一份（用户口令为空）：于是 `/P` 那些位真的生效，工具也进得去 —— pdfinfo 读成 `Encrypted: yes (print:no copy:no change:yes addNotes:no algorithm:AES-256)`，与 `lyco_pdf_nav.py` 从 `/P -3384` 算出的位逐条一致 |
| `risk.pdf` | **手搓**（`office_fixtures.py` 的 `write_risk_pdf`，逐对象自数 `<<`/`>>`） | LibreOffice 不肯写的五种形状：`/AcroForm` + 一个 `Tx` 字段、文档级 `/JavaScript`（名字树 + 流）、页 `/AA` 触发的脚本、`/Launch` 动作（打开 `winword.exe`）、`/EmbeddedFiles` 附件 `badge.exe`；页对象**不写** `MediaBox`/`Rotate`，从 `/Pages` 继承。写完用 `pdfinfo` 验：`Form: AcroForm`、`JavaScript: yes`、`Pages: 1`、`Page size: 612 x 792`、`Page rot: 90` —— 五条都被第三方读者认了才算 fixture |

## 几件只有踩过才会记下来的事

1. **宏样本是合成的**。这里没有真 VBA 工程（也没有能造它的工具），`notes.docm` 只证明
   「包里出现 `vbaProject.bin` / 声明了宏内容类型」这条检测成立，**不证明宏内容可解**。
   `office-objects` 的 about 文本里也是这么写的。
2. **纯 ASCII 的 `.doc` 也走 16 位 piece**。写这份样本的初衷是让 8 位（`fc` 的 bit30 = 压缩、
   偏移还要除二）那条分支有真实覆盖，LibreOffice 却没有那样编码。所以那条分支由
   `word.rs` 的**合成字节**单测算术，并在测试注释里写明它不冒充 Word 的文件。
3. **同一份文件里的两个时间戳本来就互相不一致**。`notes.docx` 的 `core.xml` 写
   `2013-12-23T23:15:00Z`（python-docx 的模板默认值），LibreOffice 转成 `.doc` 后
   OLE 属性集里的 FILETIME 换算是 `2013-12-23T15:15:00`。两边都照文件里的数报，
   谁也不许被"修正"成跟对方一样 —— 那是替文件编话。
4. **`.ppt` 的「一张幻灯片」不是从记录名数出来的，是拿内容对出来的**。`deck.ppt` 的 PowerPoint Document
   流按 8 字节表头（`+0` recVer、`+2` recType、`+4` 长度）走满 1427 条记录、零处错位，
   67 个文本原子（`0x0FA0` / `0x0FA8` / `0x0FBA`）全在里面 —— 但 `SlideContainer`（`0x03F8`）
   有 11 个（母版、备注页都算），**它不等于 2 张幻灯片**。
   真正一页一个的是 `recType 0x03EE` 的容器：这份文件里 2 个，各自子树里的文字与 `deck.pptx` 的
   `ppt/slides/slide1.xml` / `slide2.xml` 逐张一致（张数、顺序、每行的字都对得上），所以
   `office-slide` 按它归页，交回每页的行与原子数，并带上那条记录在流里的偏移 —— [MS-PPT] 的
   规范文本我手上没有，故只报数值，不编 recType 的名字。归了页的原子 9 个，剩下 58 个不归任何一页 ——
   备注页的字、母版与版式里的占位文字都在其中（`office-text` 逐条列，`office-slide` 不硬塞给某页）。另一处只有踩过才知道的：`TextCharsAtom` 规范写"16 位字符、低字节
   Windows-1252"，而 LibreOffice 对非 ASCII 写的是真 UTF-16 —— 高字节全零时两种读法结果
   相同，所以判据用字节自己给（见 `ppt.rs` 模块注释第 3 条）。

5. **RTF 的 `\info` 不是一座孤岛，也不是一个群**。`notes.rtf` 里 `\info{}` 只包住了
   标题那一对 `\upr`，`subject` / `keywords` / `doccomm` / `author` / `creatim` /
   `\*\userprops` 全跟在同一层往后排 —— 只读「`\info` 后面那一个群」会只剩标题。
   而且标题的真值在 `\upr` 的**第二**群（`\*\ud`）里，第一群是一串 `?`；`proptype`
   这类带数字参数的控制字又落在群与群**之间**。属性名要按群整块交账，逐字交账会把
   `AppVersion` 变成十条自定义属性。`\printim` 是全零，报成 `0000-00-00` 就是替文件编话。

6. **一个格子是不是日期，账不在格子上**。`formats.xlsx` 里 `C1` 写的是 `s="1"` ——
   那是 `xl/styles.xml` 的 `cellXfs` **下标**；绕开这一层，41631 就只是一个数。绕过去
   之后还有两处：openpyxl 把日期/百分比/货币全写成**自定义**格式号 164-168（内置表里
   查不到），而 `C7` 是文本 `12/23/2013` —— 格式号是 0，谁都不该替它猜一个日期。
   换算还要看 `xl/workbook.xml` 的 `date1904`，而 1900 系统里第 60 号是那个不存在的
   1900-02-29：文件自己写着 60，就照 60 报，不替它改成某一天。

7. **ODF 的格子没有名字，也没有序列数**。`book.ods` 每一行的最后一个格子写着
   `number-columns-repeated="16382"` —— 那是一片空白，占 16382 列而不是 1 格；
   `formats.ods` 的行首还有 `repeated="2"` 的空格，所以列号必须一路累加，
   否则 `C2` 会被数成 `A2`。日期在 ODF 里直接就是 `office:date-value="2013-12-23"`，
   没有 1900/1904 那套基准要猜，而显示文本（`2013年12月23日`）与值是两样东西。
   隐藏表更绕：`table:table` 只写 `table:style-name="ta3"`，得去那个自动样式的
   `table:table-properties` 里读 `table:display="false"`。 LibreOffice 自己在 `meta.xml`
   写了 `cell-count="11"` —— 数出来的格子数与它对得上，这是第三方给的保证。
   还有 `calcext:value-type` 那份实验命名空间的副本，按局部名乱抓就会抓到它。

8. **ODF 的批注不在另一个部件里，它嵌在正文段里面**。`notes.odt` 的那条 `text:annotation` 是某个 `text:p` 的孩子，作者与时间还在它自己的 `meta:creator` / `meta:date` **子元素**上（docx 那边是 `w:comment` 的属性）。整段 `itertext()` 一把抓就会把「这里要补上不含税口径」接在正文后面，段数也会比「正文段」多一 —— LibreOffice 自报 10 段就是这么来的。
   ODP 另一坑：`presentation:notes` 里除了备注框还坐着页码占位，里面是样字 `<编号>`，按整页取字就会把它当正文。

9. **同一份数据，两套账铺出来必须是同一张网**。`formats.xlsx` 与 `formats.ods` 是 LibreOffice
   从同一个文件转出来的两份，`--csv` 把它们各自的第一张与第二张表逐字比过：一边靠
   「序列数 + cellXfs 格式码」判日期，一边直接读 `office:date-value`，两条路给出的 CSV
   相同才说明都没猜。唯一的差别在 `book` 那一对：`合计` 旁边 xlsx 是空格，ods 是 `142000` ——
   openpyxl 写公式不写缓存结果（`<v></v>`），LibreOffice 写了。这不是读者的分歧，是文件事实。

10. **关系里的 `Target` 可以是从包根算起的**。openpyxl 在 `xl/_rels/workbook.xml.rels`
    里写 `Target="/xl/worksheets/sheet1.xml"`（前面带斜杠），Word 与 LibreOffice 写相对形式
    `worksheets/sheet1.xml`。只按「相对当前部件所在目录」拼一条规则，前者会拼成
    `xl//xl/worksheets/sheet1.xml` —— 读不到部件，整张表就成了零格，看着像空表。

11. **`word/footnotes.xml` 里有四条 `w:footnote`，而文档只有两条脚注**。Word 与 LibreOffice
    都会在这个部件里放两条占位（`w:type="separator"` 与 `"continuationSeparator"`，正文是空的）：
    只数元素个数就把两条说成四条，`--keep-empty` 一开还多出两条空「脚注」。
    这份样本由 LibreOffice 从 RTF 导入写出（python-docx 1.2 没有加脚注的 API）——
    顺带记下 RTF 的写法：要用 `{\footnote …}` 这一族，`\footnote{…}` 那种 LibreOffice 的导入器
    会把别处的字串行当脚注正文（第一版就把字体名 `Calibri;` 写了进去）。

12. **屏上写着 `¥124000.00` 的那一格其实不是货币格式**。`formats.ods` 的 `ce4` 指的是
    一个 `number:number-style`，¥ 是里面的字面量 `<number:text>¥</number:text>`，
    不是 `number:currency-style`；而 `ce2` 那个 `number:date-style` 里坐着
    `hours` / `minutes` / `seconds` —— ODF 的「日期时间」就是一个带时间部件的 date-style，
    没有单独的元素名。**类别只能看元素自己的名字**，照屏上的样子反推就是替文件编一个类别。
    数据样式还分在两处：`N41`/`N49`/`N99` 在 `content.xml`，`ce2`~`ce4` 指的
    `N150`/`N151`/`N152` 在 `styles.xml` —— 只读一份就会报「找不到格式」。
    另外 ODF 的格式是一棵元素树，没有 Excel 那种格式串，所以这里只逐条抄 token
    （`year`、`text:-`、`month`…），不重构 `yyyy-mm-dd`。

13. **ODF 的 `meta.xml` 里有一条写着空串的引用**：`<meta:template xlink:href="" xlink:type="simple"/>`。
    数「站外目标」时它既不是包内路径也不是外部地址 —— 照文件报出来（`target: ""`），
    不替文件删掉一条它自己写下的属性。「在不在包外」只用 URI 的通用形状判：带 scheme 的算外面。

14. **RTF 的页眉与正文在同一个流里**，只靠目标群分开：`{\header\pard …\par }`。
    以前这些字会被当成正文行交出去。两个坑：
    `\headery720` / `\footery720` 是「页眉高度」那种**格式**控制字，不是目标 ——
    控制字读到字母为止，所以整名比对（`header` vs `headery`）刚好把它们分开；
    而一条页眉会同时写进 `\header`、`\headerf`（首页）好几个口袋，
    `notes-hf.rtf` 里 3 条页眉 + 2 条页脚就是从 6 个目标群读出来的 ——
    每条都带着口袋名，不替文件合并成「一份页眉」。

15. **「这份文档多少字」有三份账，而且两个对不上是正常的**。`notes.odt` 的
    `meta.xml` 自报 `character-count="67" non-whitespace-character-count="66" word-count="61"`，
    我们与 LibreOffice 各数各的字符数，结果 67/66 完全一致；词数我们报 10 ——
    因为 `words_by_space` 就是「按空白切的词」，一整段中文算一个，而 LibreOffice
    按中文词切。docx 那边更直接：python-docx 写了 `docProps/app.xml`，可
    `Words` / `Characters` / `Paragraphs` / `Lines` 全是 0（它没数过，不是文档没字），
    只有 `Pages` 是 1。所以两边并排给，键名把口径写死，谁也不许盖掉谁。
    另：`notes-hf.odt` 我们数 40 个字符而生产者数 86 —— 差的是页眉页脚那份，
    我们只数正文（`statistics.ours`），这条口径差也是有意留出来的。

16. **PDF 的「不追交叉引用表」要补两层才成立**（`objstm.pdf` 就是为这两层留的）。
    对象在文件里以 `N G obj … endobj` 明写，xref 只是索引，所以容忍读法从扫标记开始 ——
    但 qpdf 那份里 **68 个对象只有 17 个是明写的**，另外 51 个住在 `/Type /ObjStm` 的压缩流里
    （头部是「对象号 体内偏移」成对，偏移**相对 `/First`**；这条不是背出来的，是拿一份真的
    Word 2013 文件对出来的：它 `/N 6 /First 39`，头部 `14 0 13 51 10 101 …`）。
    第二层：PDF 1.5 起 trailer 的键可以整个搬进 `/Type /XRef` 的流字典 —— 那份文件里
    `trailer` 出现 **0 次**，只认 `trailer` 的读者连元数据都找不到。
17. **`/Type/Page` 与 `/Type/Pages` 差一个 s，含义差一整页**。子串匹配会把树节点数成页，
    页数立刻多一；两边的读者都用「名字整段比对」，`objstm.pdf` 的 2 页 / `/Count 2` 就是这条的哨兵。
18. **`MediaBox` 与 `Rotate` 可以不写在页上**（`risk.pdf` 故意这样写：页对象两个都没有，
    `/Pages` 上写一份）。不沿 `/Parent` 往上走就会报「这页没有尺寸」，而 `pdfinfo` 照样报
    612×792 —— 报不出就是读者的错，不是文件的错。
19. **加密的 PDF 里「读得出来」的不等于「是真的」**。`locked.pdf` 的 `/Lang` 按字符串规则
    能解出一串乱码（`*Þ1s~A%¹QkXo»åPV*…`），第一版读者就把它当成语言标记交了出去。
    加密只对**字符串与流正文**下手，名字与数字不动，所以那份文件里页数、页面尺寸、字体名
    照样报得出，而 `/Lang` 与 Info 各项**刻意给 null** 并说明为什么。
20. **手搓的 fixture 必须先过第三方读者**。`risk.pdf` 第一版被 `pdfinfo` 判为
    `Kid object (page 1) is wrong type (stream)` —— 起因是某个字典少了一个 `>`，
    一个字符的错让页对象变成了流；现在生成器每个对象先自数 `<<`/`>>` 再写文件，
    写完仍要 `pdfinfo` 报出预期页数与标志位才算数。

21. **PDF 的正文难在位置，不难在认字**（`notes.pdf` 是第一份把三条规则全踩齐的样本）。
    LibreOffice 写一个中文标题会拆成好几段 `TJ`，段间带着 `-2999` 这类大负数。三条规则少一条，
    结果都是「字全认得、顺序全错」这种最像读通了的答案：
    `BT` 把文本矩阵与文本行矩阵都复位成单位阵（不复位就把上一行的坐标一路累加，一行会被拆成三行）；
    字形前进量只加在 `e` 上（`e' = e + a·step`，误用 `d` 就变成每字往上飘 —— `124000` 六个数字
    被摆成一条斜线）；**`TJ` 数组里那个数是反着用的**，正数把笔往左推、负数往右推
    （当成正常符号，标题会被排成「一：算口径级标题预」）。
    还有两条是量出来才敢写的：`/Resources` 与 `/Font` **两处都可能写成间接对象**
    （LibreOffice 写的是 `/Font 66 0 R`，只认内联那种就整页解不出字，而且解不出得很安静）；
    行容差要跟着字号走（表格里「服务器」与右对齐的「124000」基线差 4.3 点，
    固定 2 点会把一行拆成两行）。逐行结果与 `pdftotext`（xpdf 系，第三套代码）对过：
    散文与标题一致，表格那几行我们按视觉行给（`科目 金额` / `服务器 124000`），
    xpdf 的 plain 模式按它自己的列启发式给（`科目 服务器` / `金额 124000`）——
    这一处不同是**两种口径**，不是谁读错了，记在这里。
22. **PDF 的界也记全**：内容流是密文的加密件不给正文（只说解不出来）、图形流里的字不做、
    CID 字体的宽度在 `/W` 数组里（不跟，那种字体认得出字但位置会偏）、表单字段值与签名校验不做。
25. **加密的 PDF 里「哪儿」不是密文，「什么」才是**（`perms.pdf` 与 `locked.pdf`）。
    `/Encrypt` 字典本身、对象的**编号**、`/P` 那个数、页树与书签的 `/First`→`/Next` 结构全都读得出，
    所以加密件照样报「有几条书签、跳到第几页、允不允许打印/复制/批注/填表」；
    而 `/Title`、`/URI` 这些**字符串**是密文，解出来是乱码 —— 那一律给 null。
    另外两条是这一族自己踩的：`/Outlines` 那个字典**不是**一条书签（第一条在它的 `/First` 上），
    以及 `/P` 是**负数**（-3384 / -1028），不带符号的整数读法直接读不出来。
    权限的位号从 **3** 起（规范就是这么数的）：3 打印、4 改、5 复制、6 批注，R3 才有 9 填表、
    10 无障碍抽取、11 拼页、12 高质量打印；R2 的文件里后四位交回 null，不假装知道。
23. **「一次编辑」不是一个元素，也不是一条 region**（`revisions.docx` / `revisions-lo.docx` /
    `revisions.odt` 是同一批字的三副面孔）。LibreOffice 写 OOXML 会把「插入 124000 元」拆成两个
    `w:ins`（数字与单位各一条），而它自己导出的 ODF 把同一次编辑写成**一个** `text:changed-region` ——
    于是 OOXML 那侧把**相邻**且同（类型 + 作者 + 时间 + 所在段 + 是否段落标记）的元素并起来之后，
    两份文件的账逐字相同（4 条：插 124000 元 / 删 89000 元 / 王五改了那半句的字样 / 整段新加）。
    这条合并规则不是照规范抄的（规范没说一次编辑只能一个元素），是这两份 fixture 对出来的。
    顺带记三处差别：段落标记的 `w:ins` 在 LibreOffice 导出时丢掉（python-docx 那份有，5 条 vs 4 条）；
    ODF 的日期不写那个 `Z`。「改了格式」那一条也得带着**被改的那些字**，两家住的地方不一样：
    ODF 在 region 引出来的那段区间里，OOXML 的 `w:rPrChange` 只装新的 `rPr`，字在它所住的那个
    `w:r` 身上 —— 两边都绕这一步，四份账才逐字相同（这一条是 CI 头一次真跑对账比出来的：
    当时 docx 那条永远是空串，与 ODF 不同形）。
    还有一条是这三份 fixture 修出来的：`text:tracked-changes` 里那份 `text:p` 是**被删掉**的段落，
    从前 `office-doc` 的段落数与 `office-text` 的正文都会把它当现存的读回来。

24. **「这份还能动吗」四家写在四个地方，而且互相不搬**（`protected.*` 与 `locked-sheet.*`）。
    docx 在 `word/settings.xml` 的 `w:documentProtection`（`w:edit` 说限制成什么、
    `w:enforcement` 才说开没开）；xlsx 分两层，`workbookProtection` 与每张表的 `sheetProtection`；
    ODF 的表保护是 `table:table` 身上的 `table:protected` + `table:protection-key`，文档级则在
    `settings.xml` 的 config-item 里。三条实测出来的坑：① 同一句话两种拼法 —— openpyxl 写
    `sheet="1" formatCells="0"`，LibreOffice 重写同一份东西写 `sheet="true" formatCells="false"`
    并且把等于默认的 `insertRows="1"` 整个省掉，所以省掉的不能补成 false；② LibreOffice 导出 xlsx
    时把 `lockStructure="1"` 写成了一个**空的** `<workbookProtection/>`，结构锁就这么丢了（openpyxl
    本来也爱留一个空元素 —— 元素在场不等于锁上）；③ 把带 `w:documentProtection` 的 docx 转成 .odt，
    LibreOffice 不搬那份限制，三个 Protect* 全是 false —— 同一份内容换个格式就"没保护"了。
    还有一条是自己踩的：往 `xl/workbook.xml` 里**再插**一个 `workbookProtection` 会造出同段两个
    同名元素（`maxOccurs=1`），LibreOffice 只读第一个，锁就"凭空丢了" —— 要改 openpyxl 留的那个空的。

26. **`.xls` 的锁是「按哪张表」记的，而且靠子流位置认表**（`locked-sheet.xls` 与 `locked-second.xls`）。
    BIFF8 没有 OOXML 那种「工作簿一层 + 表一层」：`PROTECT`(0x0012)、`PASSWORD`(0x0013)、
    `SCENPROTECT`(0x00DD) 是写在**被锁那张表自己的子流**里的记录。这一条是量出来的而不是按记录名推的：
    那两份件唯一的差别是锁放在第一张还是第二张表，而三条记录跟着挪窝；LibreOffice 自己把这两份 .xls
    import 回 .ods，也只在同一张表上写 `table:protected="true"` + `table:protection-key` ——
    归属关系两边一致。归位的判据是 BOUNDSHEET(0x0085) 自报的子流起点，不是「第几条子流」。
    两处不猜：`0x00DD` 没找到第二个读者认得它，所以只交回原值、不替它编开关名；`0x0013` 那格是
    Excel 那套 16 位旧哈希（这批件里是 `6e4e`），不是口令 —— 而且它算不出来：`locked-sheet.ods`
    那一路（`table:protection-key` 是摘要）转成 .xls 时，这一格写的是 0。

27. **没被真件走到的代码，两份读者会一起错**（`mulrk.xls`）。MULRK(0x00BD) 一行连续格子
    共用一条记录：`rw(2) + colFirst(2) + 每格 {ixfe(2), rkmac(4)} + colLast(2)` ——
    值是每条的**后**四个字节。Rust 与 `lyco_legacy.py` 都曾写成 `4 + i * 6`，于是第一格
    解出来是 1144750.11 这种看着像浮点误差的乱数；两边同一个错，对账永远绿。
    这份件是专门造来走这条路 的：`mulrk.xlsx` 一行八个连续数字，LibreOffice 转 .xls 后
    确实并成一条 54 字节的 MULRK，按正确偏移解出 1000.5…8000.5，与 LibreOffice 自己把这份
    .xls 读回 .ods 交出来的 A2:H2 逐格相同（第二行三个整数 7/14/21 同理）。
    记下这条是因为方法：`book.xls` / `formats.xls` 里都没有 MULRK，所以「两边一致」
    从来不是「两边都对」的证据 —— 对账只能覆盖真件走得到的部分。

28. **`.xls` 的数字格式在另一条跳上**（`formats.xls`）。格子的记录只写 `ixfe`，它是 XF 记录
    （0x00E0）在这条流里的**出现序号**；XF 自报的格式号在正文偏移 2（前两个字节是父样式索引）；
    格式号 >=164 的串在 FORMAT 记录（0x041E）里，内置号（这批件里遇到 9 与 41~44）文件里不写串。
    FORMAT 的布局是按长度对出来的：`ifmt(2) + cch(2) + 拼法(1) + 串`，`12 = 5+7`（General）与
    `29 = 5+12×2`（16 位那份）两条都刚好对上，所以不是猜的。日期基准看 DATEMODE（0x0022），
    这份件写的是 0。换算出来的日期与 LibreOffice 自己把这份 .xls 读回 .ods 的 `office:date-value`
    逐格相同（C1 2013-12-23、C2 2013-12-23T15:15:00、C5 汉字那格、另一张 A1 2026-09-23）。
    一处按字面判：168 号是 `\¥#,##0.00`，币符是**转义的字面量**，所以判成 number 而不是 currency ——
    与 ODF 那边「¥ 写成字面量的就还是 number-style」同一条规矩。

29. **尾注部件这台机器上有生产者，但不在 RTF 那条路上**（`notes-end.docx`）。
    `office-doc` / `office-text` 都有 `endnote` 那一支，此前只有「包里没有 `word/endnotes.xml`
    就是 0 条」这一半有证据，「真有几条尾注」那一半没有。试过两条路都拿不到：
    RTF 的 `\endnote` 被 LibreOffice 的导入器摊进正文（所以 `notes-foot.docx` 只有脚注），
    ODT 里手搓的 `text:endnote` 它读不见（没有 `text:notes-configuration` 就不认）。
    真路是 **docx 导出器**：把一条尾注注进 `notes-foot.docx`（`office_fixtures.py` 的
    `write_endnote_seed`）再让它 `--convert-to docx` 照抄一遍，出来的包里
    `word/endnotes.xml` 还在，而且两条分隔符被它**改写成自己那套写法** ——
    我注进去的是空的 `<w:r/>`，它写出来的是 `<w:r><w:separator/></w:r>` 与
    `<w:continuationSeparator/>`，注引用的样式名也从 `Style14` 换成它自己的 `Style15`。
    这两条就是「分隔符不是一条注」与「注的字不混进正文」在尾注这一支上的真件证据：
    部件里三条 `w:endnote`，读出来一条。手搓的只有「这里有一条尾注」那个意图和那句字。
    同一条路顺手把 **ODF 那一支**也点亮了：`notes-end.docx → notes-end.odt` 那一转里，
    LibreOffice 写出 `text:note-class="endnote"`（`text:id="ftn3"`，编号是罗马数字 `i`，
    段样式叫 `Endnote`）—— 之前「ODT 里手搓的 endnote 它读不见」是**导入**那一侧的事，
    导出这一侧一直是全的。还量到一件 RTF 的事：它的 RTF **导出**把尾注写成
    `{\*\footnote\ftnalt …}`，也就是脚注套子加 `\ftnalt` 这个反标志（外加一条
    `\aendnotes` 注解）—— 尾注与脚注在 RTF 里靠这一个词区分，而这条目前只是量到，
    还没有读者去判它（`office-doc` 压根没有 rtf 那一支，「有几条注」这一问在 RTF 上没人答）。

30. **`.xls` 的隐藏行与隐藏列：那一位是拆开两个变量量出来的**（`hidden.xls`）。
    BIFF 不写「隐藏」这个开关，它写在行的属性记录里。第一眼看 `ROW`(0x0208) 正文偏移 8
    那一格（MS-XLS 说那里是 grbit）会发现它全是 0，而隐藏那两行的偏移 12 那一格是
    `0x0120`、其余行是 `0x0100` —— 差的正好是 `0x20`。但「差 0x20」这一条**单独看不算证据**：
    同一格也可能是行高的另一种写法（256 与 288 都是看着像高度的数）。所以做对照件把
    两个变量拆开：一份把五行分别设成 4pt/15pt/60pt/250pt 而**不藏任何行**，
    那一格恒为 `0x0140`；另一份同样设一行的高度、只把第三行藏起来，它就变成 `0x0120`；
    而像 `hidden.xls` 那样没有自定义行高的可见行，恒为 `0x0100`。于是 `0x20` 是隐藏位、
    `0x40` 跟着「有没有自定义行高」走 —— 按 `0x20` 判不会把看得见的行算成隐藏
    （`case-c.xls` / `case-d.xls` 在 `.scratch/xh/` 里，不算 fixture，只是量的过程）。
    列是另一件事：`COLINFO` 写的是**首末都含的一段**（这份件里 `colFirst=2 colLast=4` 一条
    就是 C/D/E 三列），少展开一格就少报一列；而 LibreOffice 给这条记录用的 id 是
    **0x007D**（BIFF5 那个），不是 MS-XLS 给 BIFF8 命名的 0x07D0 —— 两个 id 都认。
    最后一条诚实话：手上只有 LibreOffice 写的 .xls，「偏移 8 那一格带 0x20」这条分支
    在这台机器上没有任何件走过，所以两处都查；这只说明我们没验过 Excel 那一家的写法，
    不代表它那样写。四种存法（openpyxl 一列一条、LibreOffice 的 xlsx 并成一段、
    ODF 的 `collapse`、BIFF 的字段位）在 `hidden_rows_and_columns_are_counted_whichever_way_they_are_written`
    与 probe 的 3a5 里必须报同一个数：2 行、3 列。

31. **表格批注是「两跳」的东西，两个生产者把部件放在两个地方**（`cell-notes.xlsx` 一族）。
    批注**不在** `xl/worksheets/sheet1.xml` 里：要先从这张表自己的
    `xl/worksheets/_rels/sheet1.xml.rels` 找到 Type 结尾是 `/comments` 的那条关系，
    再解 Target 才有部件。这一跳上有三件事只有对照着量才看得见：
    * **部件名与 Target 拼法都不同**：openpyxl 写 `xl/comments/comment1.xml` 并把关系
      Target 写成**绝对**的 `/xl/comments/comment1.xml`（而且关系的 `Id` 是字面量
      `comments`，不是 `rId4`）；LibreOffice 写 `xl/comments1.xml`，Target 是相对的
      `../comments1.xml`。只按一种拼法解路径，另一家的件就报「没有批注」。
    * **作者名不在格子上**：`<comment authorId="1">` 是个**下标**，指向同一部件开头
      `<authors><author>` 那个有序列表；按名字找会一条也找不到。
    * **注文字的元素层级也不同**：openpyxl 直接写 `<text><t>…</t></text>`，
      LibreOffice 包成 `<text><r><rPr>…</rPr><t>…</t></r></text>`；两边都按 `t` 收才对。
      连条目的**顺序**都不一样（LO 那份 A3 排在 B2 前面），所以对账按格子配对而不是比列表顺序。
    ODF 那边是同批字的第三种存法，也是最容易读错的一种：`office:annotation`
    **就是格子的孩子元素**，`text:p` 与格子的正文并排坐着 —— 一锅端取格子的字，
    B2 就成了「124000\n这里要补上不含税口径」。这一条与 .odt 那边
    「批注（`text:annotation`）与修订表（`text:tracked-changes`）里的段不算正文」是同一条规矩。
    最后：`Comment(text, author, dt)` 里那个时间戳，openpyxl **根本没写进部件**，
    LibreOffice 转出的 ODF 也只留一个空的 `<meta:date-string/>` —— 两边都交回 null，
    不替文件编一个创建时间。`.xls` 是第四种存法，见下面第 35 条。

32. **RTF 的 `\*` 修饰的是紧跟它的那个群，不是一句「见 `\*` 就跳」**（`notes-end.rtf`）。
    RTF 里 `\*` 的意思规范写成「不认识这个目标群就把整群跳过」—— 重点在**认识与否**。
    这一版原先的实现是「本域不认识任何带 `\*` 的目标」，于是见到 `\*` 就整群丢：
    `fldinst`（域指令原文）、`userprops`、批注的内部文本确实该丢，
    但 LibreOffice 的脚注与尾注恰恰写成 `{\*\footnote …}` —— **一句注都不剩**。
    这份件量的就是这一条：改完之后 `{\*\footnote …}` 与 `{\*\footnote\ftnalt …}`
    分别读成一条脚注、一条尾注（同一条 docx 转出来的三份件：OOXML 两条脚注一条尾注、
    ODF 按 `text:note-class`、RTF 按 `\ftnalt`，字一模一样）。
    两条附带的判据也都是量出来的：
    * 尾注**没有自己的口袋名** —— LO 两条都用 `footnote`，靠群里的 `\ftnalt` 反标志分开
      （Word 那族才会另写 `endnote` 口袋，所以两个词都认，`endnote` 直接算尾注）。
    * `{\*\ftnsep\chftnsep}` 与 `{\*\ftncn\chftncn}` 是注的**排版定义**（分隔符、续分符、
      编号占位），不是一条注 —— 与 OOXML 部件里那两条 `separator` 是同一件事的第三种写法。
    注的字一份都不留在正文行里：`Body` 那行还是 `Body`，不跟着拖出「Footnote: …」。

33. **「有没有目录」这一问，两家的写法毫无共同点**（`toc.docx` / `toc.odt`）。
    OOXML 是一层 `<w:sdt>` 套着 `<w:docPartGallery w:val="Table of Contents"/>`（Word 与
    LibreOffice 都这么写），而**「收几级」不在这层的任何属性上，在域指令的文字里** ——
    `TOC \o "1-2" \h`；而且这层壳可能整个没有、只剩那条域指令（Word 老格式与不少转换器
    就这样），所以两种都得找。ODF 完全不同：`text:table-of-content` 一块，
    名字在 `text:name`（LibreOffice 写「目录1」）、级别在
    `text:table-of-content-source` 的 `outline-level`（这份件是 2），
    另外它把**十级条目模板全部写出来** —— 用「有几个模板」当「收了几级」会错出五倍，
    所以 `entry_templates` 只报文件写了几个，级别另报 source 那个属性。
    生产这条路和尾注一样：python-docx 给不出目录，注进 `notes.docx` 让 LibreOffice
    照抄一遍 —— 它连引号都替自己转义成 `&quot;`，**不还原实体就读不到 `\o` 的值**
    （两份读者在这一条上给出同样的 `1-2`，是因为都走各自的 XML 解码头，不是靠字符串猜）。
    没有目录的件报 `present: false`（键在、值为假），不是缺键；`.doc` 报 null（这一版没看）。

34. **同一份 RTF 在两条命令里答的是两个问题，但必须出自同一次读取**（`notes.rtf` /
    `notes-hf.rtf` / `notes-end.rtf`）。`office-text` 问「写了什么」，`office-doc`
    问「结构数得清几项」，两边都调 `rtf::extract`，所以段数与注数必须一致：
    `notes-end.rtf` 是 4 段 + 2 脚注 + 1 尾注（注的目标群 3 个），`notes.rtf` 是 7 段
    零注一张图，`notes-hf.rtf` 是 3 段 + 6 个页眉页脚群。差别只在 RTF **不是包，是一条流**：
    样式名、表格线、分节归属、目录、批注、修订、保护这一版都没判，于是 `office-doc`
    在这一支把这些项交回 `null` 而不是 `0` —— 0 是一份主张（「这份文件里没有表格」），
    null 是一句实话（「这一支没看」）。段口径两边也统一：`\par` 切出来、逐行 trim、
    丢空行，与 `lyco_rtf.py` 的 `lines` 同一条规则，所以 `statistics.ours`
    那三个数（116 / 100 / 20）是拿同四行字两边各数一遍对出来的。

35. **批注的第四种存法：`.xls` 把一条注拆成同一条流里的两类记录**（`cell-notes-many.xls`）。
    前三种是「换部件」（OOXML 靠表自己的关系表跳过去）与「换元素」（ODF 让注坐在格子里面），
    这一种连部件都不换：
    * 一条记录（这份件里是 `0x01B6`）给**字** —— 正文偏移 0 是它自报的头长 18、
      偏移 10 是它自报的**字数**，紧跟的第一条 CONTINUE 的**首字节是编码旗标**：
      0 = 一格一字节、1 = 一格两字节。这一位与 BIFF8 那个 `fCompressed` 的惯例**相反**，
      是拿一份 ASCII 作者的件与三份中文作者的件对出来的，不是引来的。
      那条 CONTINUE 之后还有一条 CONTINUE，它是注的**扩展头**不是字的续块 ——
      拼进来就会多出一串不像字的字节，所以只吃第一条并按自报的字数切。
    * 另一条记录（这份件里是 `0x001C`）给**哪个格子 + 谁写的**：
      `row(2) col(2) 留零(2) 自报的序号(2) 作者字数(2) 编码旗标(1) 作者串`，串后面还有一个 `0x00`。
      它住在**这张表自己的子流末尾**，所以两张表的注不会串门。
    * 两份列表按出现顺序配（量的这几份件里字的记录与格子记录同序），
      每条再带 `whole` 说两边自报的字数是不是都正好切出来，两类记录各自的条数也一起交 ——
      对不上时那是一个看得见的数，不是被悄悄截短的一份账。
    为什么这份件要一次改四个变量：作者名 ASCII 与中文（练那一位旗标）、作者字数 2 与 3
    （练串长）、注的字带换行（练 `0x000A` 不成为分隔符）、格子拉到 `AA100`（练两位列名），
    再加第二张表（练按子流归位）。
    两个**边界**也写在这里：① 这套读法只在 LibreOffice 写的 .xls 上量过，
    Excel 自己怎么写注手上没有件可以对证，所以「0 条」说的是「这套读法没找到」；
    ② 那两个记录号**不替它们编规范名** —— MS-XLS 把 `0x001C` 那个位置留给 EXTERNSHEET，
    而这里量到的内容是「格子 + 作者」，硬套名字就是编话。
    第二个读者是 LibreOffice 自己：把这份 .xls 再转回 .xlsx，`xl/comments` 那一条路读出来的
    (表, 格子, 字) 与这里逐条一致；而它转回去时**作者整个丢了**（写成「未知作者」），
    所以作者那一半只有字节层面的对证，这一点如实记下。

36. **RTF 的表：数得出行与格子，数不出「几张表」**（`notes.rtf` 一张 2×2，
    `tables.rtf` 两张 3×2 与 2×2）。这条流里表的证据是四个控制字的条数：
    `\trowd`（一行一条行定义）、`\row`（一行一条行结束）、`\cell`（一格一个）、
    `\intbl`（一格一个表内段落），嵌套表另走 `\nestrow` / `\nestcell`。
    实测：`notes.rtf` 给 2/2/4/4，`tables.rtf` 给 5/5/10/10 —— 与**同一份文档**的
    docx 与 odt 两副账里的 `table_rows` / `table_cells` 一字不差。
    所以 `office-doc` 的 RTF 分支把行数与格子数交出来，并把两个原始条数
    （`table_row_defines`、`table_cell_paras`）一起交，嵌套的两个另给、不混进主账。
    **但 `tables` 仍是 null**：试过一条看起来很像的规则 ——
    「连续的 `\trowd` 算一张表，遇到一个不带 `\intbl` 的段落就结束这一张」——
    它在单表件上给出 1（对），在两张表件上给出 1（错，应是 2）。
    一条会在真件上静默数错的推断，不如一个 null：null 说的是「这一判断不住」，
    报 1 说的是一份主张，而它是假的。要补上这一问，得先有一个能区分两张件的判据。

37. **RTF 的链接住在一个域的群里，读它要「前瞻」不能「吃掉」**（`notes.rtf`）。
    `{\field{\*\fldinst HYPERLINK "https://example.com/budget" }{\fldrslt {预算制度}}}` ——
    地址在**域指令**那一群里，而那一集团本身是「不认识就跳过」的对象（它不是页面上的字）。
    所以这一条不是把 `\*` 的规则再改一遍（#32 那条已经定死：不认识才跳），
    而是**看到 `\field` 时往群里看一眼**：不推进游标、不改 skip，
    `\fldrslt` 的显示文字照旧流进正文 —— `notes.rtf` 仍是 7 行，其中一行就是「预算制度」。
    读出来的地址与同一批字的 docx 那一份（`word/_rels/document.xml.rels` 里
    `Type="…/hyperlink"` 那一条）**一字不差**，显示文字也一样。
    两个数分开交：`fields` 是域有几群（页码、日期都是域），链接只是其中认出来的那些；
    实测 `notes.rtf` 是 1 域 1 链接，另外三份 RTF 都是 0 域 0 链接。
    站外/站内这一条按地址自己说（含 `://` 算站外）—— 这一族没有关系表可查，
    不去借 OOXML 那套 TargetMode 的话。

38. **字体表与样式表：认得了就别连名字一起丢，但也别因此改掉跳过的那笔账**
    （`notes.rtf` 14 个字体、样式表写了 77 条、正文用到 3 个样式）。
    `{\fonttbl{\f0…Times New Roman;}{\f1…Symbol;}…}` 与
    `{\stylesheet{\s0…Normal;}{\s1…heading 1;}…}` 以前都整群跳过 —— 于是
    「这份文件用了哪些字体 / 哪些样式」这一问在 RTF 上根本没有答案。
    这次的读法是**前瞻**：那两群仍然整群跳过（里面的一个字都不进正文），
    只是在跳之前把子群里的定义读一遍。为什么不让它们变成「认识的群」像页眉那样吃掉？
    试过 —— 那两群里坐着几十个 `{\*\falt …}` / `{\*\csN …}` 子群，整群吃掉会让
    `skipped_destinations` 从 65 掉到 23，那个诊断数就没法与改前比了。
    两个口径因此各自守住：跳过的群数不变（65 / 55 / 20 / 53 四份件全对），
    正文里的 `\sN` 只数正文那一份（样式表自己那 77 条定义不算使用），
    实测 `notes.rtf` = Normal 9 次、heading 1 一次、heading 2 一次 ——
    两个标题次数与同一批字的 docx 那一份（Heading1 1、Heading2 1）一致，
    `Normal` 的差是两家的记法不同（Word 只给显式带样式的段落记名），不强行归一。
    **字体名有一条硬规矩**：条目自己写 `\fcharsetN`；N 不为 0 而名字里又有非 ASCII 码位时
    （`notes.rtf` 里那个日文字体就是这么写的，charset 128 = Shift-JIS），
    按 cp1252 解会得出「‚l‚r ƒSƒVƒbƒN」这样一串看着像字的乱码 ——
    所以那种条目只交 `name: null` 与它自报的字符集号，不假装我们读得出。
    全 ASCII 的名字与字符集无关（每一种字符集都含 ASCII），照样交名字。
39. **RTF 的标题：层级就写在样式名里，两处一接才有**（`notes.rtf` 两级、`notes-hf.rtf` 一级、
    `notes-end.rtf` 一条也没有、`tables.rtf` 两级）。
    RTF 的段落只说自己用哪个样式号（段属性里的 `\s1`），号叫什么名字在样式表里 ——
    所以「这是不是标题」要先把号查到名字，再看名字合不合 `heading N` 那个形状
    （`heading` 与数字之间至少一个空格、数字后面不能再有字；这个词不分大小写，
    Word 写 `Heading 1`、LibreOffice 的 RTF 导出写 `heading 1`）。不合形状的一条也不报，
    不替文件认一个层级。`\sN` 与 `\csN` 是**两个各自的编号空间**：
    `{\*\cs1 heading 3;}` 不能顶掉 `\s1` 的名字 —— 单测里专门放了这一对同号的陷阱。
    实测四份件的这本账与同一批字的 docx 那份一字不差
    （`notes.rtf` = 一级「一级标题：预算口径」+ 二级「二级标题：明细」；
    `tables.rtf` = 一级「两张表的样本」+ 二级「二级标题」），
    而 `notes-end.rtf` 全用 `Normal`，交回**空数组**而不是 null ——
    这一支确实看过样式表，「没有」与「没看」还是两件事。
    末尾没有 `\par` 的那一段不收：段以回车收，与 `lines` 同一口径，两边读者也是同一口径。
40. **那张纸：三家三种单位，换成 0.1mm 的整数才能对表**（五份件 × 三家 = 十四对）。
    docx 与 RTF 写 twips（1/1440 英寸）：`w:pgSz w="12240" h="15840"`、
    `\paperw12240\paperh15840\margl1800`；odt 写**自带单位**的十进制串：
    `fo:page-width="21.59cm"`。换算不用浮点 —— 两个读者会在最后一位上各说各话，
    所以两边都走「十进制精确展开 + 乘分数单位 + 逢半进一」的整数式子
    （`round()` 在 python 里是**逢半取偶**，正好会在 .5 上分家，故不用它）。
    12240 twips 与 21.59cm 都换成 21590（0.1mm），十四对全部如此 —— 这是这条链的地基。
    **两家会不一致，也照实报三个数**：`notes-hf` 的 docx 上下边距写 1440 twips，
    而 LibreOffice 自己导出的 odt 与 rtf 都写 720 / `1.27cm`（= 1270）——
    不挑一个当准，也不替文件合并。三条口径上的坑各自钉住：
    odt 的 `styles.xml` 里还坐着一条只写网格设置的 `page-layout-properties`，
    那条不是一张纸、也不占序号；`orient` 只交文件写了的（docx 与 RTF 竖排时干脆不写 → null，
    odt 明写 `portrait`）；RTF 只有一条（某一节的覆写住在 `{\*\sectx}` 群里，
    而这一族不判分节归属，`sections` 仍是 null），`\header` 那种已知目标群里写的
    `\paperw` 也不算文档默认值 —— 那一群整个另读，主循环看不见它。
41. **换一份尺寸才看得出来：A4 在这三家都不是「210×297」**（`paper-a4.docx` / `.odt` / `.rtf`）。
    Letter 恰好是 python-docx 模板的默认值，光在它上面量，换算式子凑对了也不知道。
    A4 的短边 OOXML 与 RTF 写 **11906 twips** → 21001（0.1mm），LibreOffice 的 ODF 又
    照抄成 `21.001cm` → 同一个 21001；长边 16838 twips → 29700。
    **这三份 A4 件全部落在 21001×29700 上，三家互相一致** —— 但「210mm×297mm」那个名字谁都够不着。
    这就是这一支**不报纸张叫什么**的原因：查表认「A4」要在 0.1mm 上开容差，
    而开了容差就得回答「那 JIS B5 与 ISO B5 差 4mm 算不算同一张」，
    不如把 21001×29700 这个数交出去，让人自己认。
    同一份件还量出两件事：横过来的那一节在 OOXML 与 ODF 里都写着
    （`orient="landscape"`、宽高对调、边距 1.5cm → 1499），
    而 LibreOffice 的 **RTF 导出整份文件一个 `\landscape` 都没有** ——
    那一副里只剩文档默认的纵向，所以它的 `orient` 只能是 null、`papers` 只有一条。
    两份件的这种差是**文件的差**，不是读者的差：第二个读者与 Rust 在同一份件上读到同一个东西。

## 这些数字从哪来

Rust 测试里每个期望值都来自第二读者对这些文件的独立读取：
`scripts/acceptance/office_reader.py`（OOXML / ODF / MS-CFB / OLE 属性集，只用标准库）、
`lyco_rtf.py`（RTF）、`lyco_legacy.py`（`.doc` piece 表、`.xls` BIFF8、`.ppt` 记录树）、
`lyco_formats.py`（`.xlsx` 的数字格式与日期换算），以及 `office_reader.py` 里的
`ods_facts()`（`.ods` 的重复计数、覆盖格与自动样式可见性）。
那张纸（纸面尺寸与四边）另有 `scripts/acceptance/lyco_pages.py`：同样只吃标准库，
docx 用 ElementTree 找 `w:sectPr` 的 `w:pgSz` / `w:pgMar`，odt 找 `styles.xml` 里真写了
`fo:page-width` 的那些页布局，两边都按同一条整数式子换成 0.1mm（RTF 的那一串在
`lyco_rtf.py` 的走查里，交给 `lyco_pages.rtf_entry` 换算）。
修订那一份另有 `scripts/acceptance/lyco_revisions.py`：ElementTree 的 `.tail` 天然带着
「插入的字夹在两个标记之间」那个顺序，而 Rust 那边靠 xmlscan 的 `#text` 子节点走同一条路 ——
同一份 `revisions-lo.docx` 与 `revisions.odt` 两边逐条对得上，才对得起「合成规则」这四个字。
保护那一份另有 `scripts/acceptance/lyco_protect.py`：同样只吃标准库，
按 ElementTree 的属性取法把四家的开关与两层结构各读一遍，与 `protect.rs` 逐字段对。
CI 的 `apps` job 会把编出来的 `lbin` 再跑一遍 `office_probe.py` 与它们逐字段对账，
不一致就红 —— 而不是只跑一遍单元测试说"自己跟自己也挺一致"。

PDF 那一族另有一份只依赖标准库的读者：`scripts/acceptance/lyco_pdf.py`
（对象表、对象流那一层、trailer 与 `/Type /XRef` 两处找 `/Info`、字符串三件事、
页树与继承、字体与图片清单、脚本与动作）。这一族还有**第三、四个读者**可查：
本机 MiKTeX 带的 `pdfinfo` / `pdffonts` / `pdftotext`（xpdf 系，与本仓两套实现都无关）。
页数、页面尺寸、`Page rot`、`Encrypted`、`Form`、`JavaScript`、`Tagged` 逐项与它对过，
字体清单的对象号（含「这个字体对象住在对象流里」）与 `pdffonts` 的 `object ID` 列一致；
`locked.pdf` 不给口令时 `pdfinfo` 直接拒绝打开，而这边照样报得出结构 —— 这两件事
都不在 CI 里跑（Runner 上没有这些程序），是留在这里供复核的出处。
