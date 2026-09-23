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
| `notes-hf.odt` | LibreOffice（从 `notes-hf.docx`） | 同一批字的 ODF 存法：页眉页脚在 **styles.xml 的 master-page** 里，两个节 = 两个 master-page（`Standard` 与 `Converted1`），各带一份 header + footer |
| `notes-hf.rtf` | LibreOffice（从 `notes-hf.docx`） | 第三种存法：`\header` / `\headerl` / `\footer` 这些**目标（destination）**里的字，与正文混在一个流里 |
| `notes.doc` | LibreOffice（从 `notes.docx`） | MS-CFB 复合文档 + WordDocument 流 + `1Table` 里的 piece 表 |
| `notes-en.doc` | LibreOffice（从纯 ASCII 的 `notes-en.docx`） | 中英一视同仁仍写 16 位 piece —— 记下这个事实，见下 |
| `book.xls` | LibreOffice（从 `book.xlsx`） | BIFF8：BOUNDSHEET（含隐藏表）、SST + CONTINUE、LABELSST / RK / FORMULA |
| `deck.ppt` | LibreOffice（从 `deck.pptx`） | PowerPoint 97 记录树 + 一个 59 万字节、走 FAT 的属性集流（大流那条分支的样本） |
| `notes.rtf` | LibreOffice（从 `notes.docx`） | 字体表、颜色表、样式表、`\*\userprops`、域代码与 `\'hh` 回退字节 |
| `hidden.xlsx` | openpyxl 3.1（`write_hidden_xlsx`） | 第 3、4 行隐藏，C/D/E 三列隐藏，**D2/E2 里有字**；一列一条 `<col min="3" max="3" hidden="1">` |
| `hidden-lo.xlsx` | LibreOffice（`hidden.ods` 转回 OOXML） | 同一份账的另一种写法：`<col min="3" max="5" hidden="true">` 一条盖三列，没隐藏的行也写着 `hidden="false"` |
| `hidden.ods` | LibreOffice（从 `hidden.xlsx`） | 隐藏换成 `table:visibility="collapse"`，列那一跳还带 `number-columns-repeated="3"` |

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
4. **`.ppt` 的「一张幻灯片」不是能从记录里直接数出来的**。`deck.ppt` 的 PowerPoint Document
   流按 8 字节表头（`+0` recVer、`+2` recType、`+4` 长度）走满 1427 条记录、零处错位，
   67 个文本原子（`0x0FA0` / `0x0FA8` / `0x0FBA`）全在里面 —— 但 `SlideContainer`
   有 11 个（母版、备注页都算），**它不等于 2 张幻灯片**。所以 `office-slide` 对 `.ppt`
   只报原子、`slides` 留空，并在 notes 里写明"要 SlideContainer 与 SlidePersistAtom 配对
   才定得下页"。另一处只有踩过才知道的：`TextCharsAtom` 规范写"16 位字符、低字节
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

## 这些数字从哪来

Rust 测试里每个期望值都来自第二读者对这些文件的独立读取：
`scripts/acceptance/office_reader.py`（OOXML / ODF / MS-CFB / OLE 属性集，只用标准库）、
`lyco_rtf.py`（RTF）、`lyco_legacy.py`（`.doc` piece 表、`.xls` BIFF8、`.ppt` 记录树）、
`lyco_formats.py`（`.xlsx` 的数字格式与日期换算），以及 `office_reader.py` 里的
`ods_facts()`（`.ods` 的重复计数、覆盖格与自动样式可见性）。
CI 的 `apps` job 会把编出来的 `lbin` 再跑一遍 `office_probe.py` 与它们逐字段对账，
不一致就红 —— 而不是只跑一遍单元测试说"自己跟自己也挺一致"。
