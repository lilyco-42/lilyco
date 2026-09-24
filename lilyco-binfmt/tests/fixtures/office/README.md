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
| `deck-lo.pptx` | LibreOffice（`deck.pptx` → .odp → .pptx） | 同一份稿子的第二个生产者：母版从 1 份变 11 份、版式从 11 变 9、`p:sldSz` 上那个 `type` 属性**整个省掉**（尺寸两个数一字不差），备注里多出一个页码占位（字面量 `<编号>`），而第一页那句「新增两台 64 核应用服务器」被切成三个 `a:t` —— 段落数仍然是 3 |
| `deck-tables.pptx` | python-pptx | 一页一张 3×3 的表，**一次只改一个变量**：横合（第一行前两格）、竖合（第三列后两行）、只给第二行设行高 `914400`、只给第一列设列宽 `2743200`、只给 `B2` 那格设垂直对齐与左右边距、只给一格设填充色、还有一格写两段 |
| `deck-tables-lo.pptx` | LibreOffice（`deck-tables.pptx` → .odp → .pptx） | 同一张表的第二种写法：`tblPr` 变成**空的**（`firstRow` / `bandRow` 与那条 `tableStyleId` 全没了），没说过话的两行行高从 `609600` 变成 `609480`，每格的 `a:tcPr` 反倒补满四道边、一个填充与五个边距 —— 而合并那四个字（`gridSpan` / `hMerge` / `rowSpan` / `vMerge`）与列宽一字未变 |
| `deck-tables.odp` | LibreOffice（上面那一转的中间件） | 同一张表的第三种写法：列宽换成 `7.62cm` 与 `5.08cm`、行高换成 `1.693cm` 与 `2.54cm`，而合并改成**另写一格** `table:covered-table-cell`（既不是 docx 的不写、也不是 pptx 的 `hMerge`） |
| `deck-chart.pptx` | python-pptx 1.0.2（`write_pptx_charts`） | 演示稿上的图：同一页挂柱形（两条系列）与饼图（一条），第二页一张也没有；引用指向**图自己那张内嵌工作簿**（`ppt/embeddings/Microsoft_Excel_Sheet1.xlsx` 里的 `Sheet1!$B$1`），值全缓存了，轴 id 写成**负数** |
| `deck-chart-lo.pptx` | LibreOffice（`deck-chart.pptx` → .odp → .pptx） | 同一批图的第二种写法：`ppt/charts/` 里多出 style 与 colors 四个部件（按目录数会数成六张图，实际两张），`c:f` 里不再写引用而写 `label 0` / `categories` / `0` 这种字面量，**而缓存的数一字未变**；饼图那一侧另补了一个标题「占比」 |
| `notes.odt` / `book.ods` / `deck.odp` | LibreOffice（从上面三个 OOXML 文件转来） | 真 ODF 写入者产出的三种 ODF |
| `deck.odp`（结构） | 同上 | 两页：`draw:name` 是「预算评审」与「第二页：数字」；第一页有 `presentation:class="notes"` 的备注（「评审时先讲口径再讲数字」），**旁边还坐着页码占位，里面的样字是 `<编号>`** —— 整页一把抓就会把它当正文；两个母版页名、两个版式名，但文件里没有任何版式定义；尺寸 25.4cm×19.05cm landscape 在 styles.xml 的 page-layout 里 |
| `notes.odt`（结构） | 同上 | 10 段（2 段是空的）、两个带 `text:outline-level` 的标题、1 张 2×2 表（名叫「表格1」）、一条 `text:annotation` 批注、一个 `draw:frame`+`draw:image`、一个 `text:a` 超链接、5 个 `text:sequence-decl`；`meta.xml` 自报 paragraph-count 10 / page-count 2 / word-count 61 |
| `chart.ods` | LibreOffice（`chart.xlsx` → .ods） | ODF 的图是**嵌入对象**：`数据` 那张表里两个 `draw:frame` 各指一个 `Object N/` 目录，那里面的 content.xml 才写着 `chart:chart`；类型只在每条 `chart:series` 上（`chart:bar` / `chart:line`），点数另有一条 `chart:data-point@chart:repeated` 自报「这一条顶两个点」，地址是第三种写法（`数据.B2:数据.B3`：点分隔、不带 `$`），末尾还抄了一张 `local-table`（10 / 25 / 4 / 9） |
| `deck-chart.odp` | LibreOffice（`deck-chart.pptx` → .odp） | 同一批图绕一圈 ODP：引用不再指稿子的数据，而是指图自己那张表（`local-table.$B$2:.$B$3`），饼图的类名成了 `chart:circle`、标题「占比」只有这一家写了，而且 frame 连备注框一起编号 —— 第一张图叫 `Chart 2` |
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
| `chart.xlsx` | openpyxl 3.1.5（`write_chart_xlsx`） | 两张图挂在同一张表上（柱形与折线，系列数还不相等）：`c:f` 里同一段格子写成 **`'数据'!B1`**（带引号、不带 `$`），类目是文本格却写成 **`c:numRef`**，而且**一条 `c:pt` 缓存都没有** —— 「图里画的是哪些数」在这里只能交引用 |
| `chart-lo.xlsx` | LibreOffice（`chart.xlsx` → .ods → .xlsx） | 同一批图的另一副面孔：`c:f` 写成 **`数据!$B$1`**（不引号、绝对），类目改用 **`c:strRef`**，并且 `strCache` / `numCache` 把 `ptCount` 与每格的值都缓存了（一月/二月、10/25）；两个轴 id 是随机数，与 openpyxl 那两份的 10/100 没有任何关系 |
| `rules.xlsx` | openpyxl 3.1.5（`write_rules_xlsx`） | 条件格式四种规则与数据验证三种：一条 `sqref` 里塞两段区间（`A2:A6 B2:B4`）、`cellIs`/`expression` 只写 **`dxfId` 下标**（真样式在 `styles.xml` 的 `dxfs` 里，那一条只有 `font/b` 与 `font/color`）、色阶的颜色写成 `00FFFFFF` 这种 alpha 为 00 的串、`dataValidations` 自报 `count="3"`，而 list 那条**不写 operator** |
| `rules-lo.xlsx` | LibreOffice（`rules.xlsx` → .ods → .xlsx） | 同一批规则的第二种写法：`priority` 换成自己排的 2/4/5、开关从 `1/0` 换成 `true/false`、给 list 补了 `operator="equal"`、给每条验证补了 `formula2=0`、把 custom 公式开头的 `=` 去掉、色阶的白写成 `FFFFFFFF`，而且同一条 dxf 里多补了 `name`/`family`/`sz` |
| `view.xlsx` | openpyxl | 窗口与页眉页脚的第一种写法：`pane state="frozen"` 冻在 `B3`、`showGridLines="0"`、`tabSelected="1"`、`zoomScale="150"`、三条 `selection`（第一条不点 `topLeft`），页眉页脚写四段字（含一个字面 `&&`）与 `differentOddEven="1"`；第二张表用 `state="split"`（拆分不是冻结），第三张表什么都不设 —— **`headerFooter` 那个元素整个不写** |
| `view-lo.xlsx` | LibreOffice（`view.xlsx` → .xlsx，同一个格式重写） | 同一张表的第二种写法：十五个属性全写出来、布尔换成 `true/false`、`selection` 变成四条人手一份还补 `activeCellId="0"`、每段字前面多一个 `&"Calibri"`、给每张表都写出 `headerFooter`（哪怕里面是空的）；**而那个 split 的 pane 整个不见了**（冻住的那张留着） |
| `size.xlsx` | openpyxl | 列宽行高与筛选/表对象的第一种写法：A 列 `22.5`、C 列 `4` 且藏着，第 2 行 `40`、第 3 行 `8`（第 1 行什么都不写），默认行高 18 写在 `sheetFormatPr`（那一族管默认宽度叫 **`baseColWidth`**），筛选范围 `A1:C3` 带一个筛掉的值「甲」，另挂一个范围**不同**的表对象 `A1:B3`（列名拿范围第一行的字当，于是第二列叫 `10`）；`tableParts` 自己写 `count="1"` |
| `size-lo.xlsx` | LibreOffice（`size.xlsx` → .xlsx，同一个格式重写） | 换一家换算就换一套数：同一列成 `20.47` 与 `3.64`、同一行成 `39.75` 与 `7.5`，连没说过话的那一行也被补上 `ht="18"`；「默认列宽」改叫 **`defaultColWidth="7.7734375"`**、`baseColWidth` 不见；两张表都写 `sheetPr filterMode`（`true` 与 `false`），`filterColumn` 上那两个开关反倒不写；表对象补 `totalsRowCount`/`totalsRowShown`、样式开关从 2 个变 5 个，**而 `tableParts` 的 `count` 不写了** |
| `notes-end.rtf` | LibreOffice（从 `notes-end.docx`） | 注的第三种存法：脚注与尾注**都**写成 `{\*\footnote …}` 这一个群，尾注只在群里多一个 `\ftnalt`；分隔符另走 `{\*\ftnsep\chftnsep}` |
| `tables.docx` / `tables.odt` / `tables.rtf` | python-docx 与 LibreOffice（两张表：3×2 与 2×2，中间夹一段正文，首尾各一个标题） | 表那一份的对照件：三家都给 5 行 10 格，而 RTF 只敢给行数与格子数 —— 「几张表」的分组规则在 `notes.rtf`（一张）与这份（两张）上试过，单表对、两表数成一张 |
| `paper-a4.docx` / `paper-a4.odt` / `paper-a4.rtf` | python-docx 与 LibreOffice（A4 纵向一节 + 横过来的一节） | 那张纸的第二尺寸：三家换算到 0.01mm 后短边都是 **21001**（不是 21000 —— OOXML 与 RTF 写 11906 twips，ODF 照抄成 `21.001cm`），所以这一支不给尺寸起名；横排那一节 docx 与 odt 都有第二条并写着 `orient=landscape`，而 RTF 全文一个 `\landscape` 都没有 → 那一条流只交文档默认的纵向 |
| `tables-merged.docx` / `tables-merged.odt` | python-docx 与 LibreOffice（一张横向合并的 2×3 + 一张纵向合并的 2×2） | 合并格的两种写法：OOXML 把横向合掉的那一格**整个不写**（第一格带 `w:gridSpan="2"`，那一行 2 个 `w:tc`），ODF 把被盖住的那一格照样写出来（空的 `covered-table-cell`，那一行 3 个格）；纵向合并 OOXML 写 `vMerge`（restart / continue 两头），ODF 只在起头那格写 `number-rows-spanned="2"` |
| `para.docx` | python-docx（`write_para_docx`） | 「这一段自己排成什么样」的四种情况：第一段两端对齐 + 左缩进 `1701`（3cm 换算）+ 首行 `480` + 段前 `120` 段后 `60` + `line="360" lineRule="auto"`；第二段右对齐 + `hanging="360"` + **同一个 `line="360"` 配 `lineRule="exact"`**；第三段一条 `w:pPr` 都不写（`checked` 6 与 `listed` 4 的差就是它）；第四段居中之外把**第二种单位**摆出来 —— `w:left="0"` 与 `w:leftChars="200"`、`firstLineChars="150"` 并排；末尾一节改成两栏（`w:num="2" w:space="425"`），模板自带那一节只有 `w:space="720"` 没有 `num` —— 两条 `w:cols` 都在，所以 `sections` 2、`written` 2 而 `multi` 只有 1 |
| `para.odt` | LibreOffice（从 `para.docx`） | 同样的话换了地方住：段上只有 `text:style-name="P1"`，属性在 `style:paragraph-properties` 上（一跳）；`both`→`justify`、`right`→`end`、`1701`→`fo:margin-left="3cm"`、`hanging="360"`→**负的** `fo:text-indent="-0.635cm"`；`line 360 + auto/exact` 换成 `fo:line-height="150%"` 与 `"0.635cm"`（靠单位分别）；按字数那段改用另一个前缀 —— `loext:margin-left="2ic"`、`loext:text-indent="1.5ic"`，`fo:` 上什么都没有；第三段点名的 `Standard` 住在 styles.xml → `resolved: false`、`written: null`；两栏变成 `text:section` + family=section 的样式（`fo:column-count="2"`、`fo:column-gap="0.751cm"`，每栏一份 `style:rel-width` = `32767*` 与 `32768*`，另写 `style:dont-balance-text-columns="true"`） |
| `para.rtf` | LibreOffice（从 `para.docx`） | 这一族两份账**都不交**，理由量在这份件里：正文五段每段前面都把样式默认重发一遍（`\sl276\sb0\sa200\ltrpar…` 段段都有，「写了什么」与「继承了什么」分不开）；第二段那个右对齐整个没写出来（全文 `\qr` 0 次、`\qj` 与 `\qc` 各 1 次）；按字数的那两个缩进被抹平成 `\li0\fi0`；1.5 倍与固定 18 磅它用**符号**分别（`\sl360\slmult1` 与 `\sl-360\slmult0`）；全文 `\cols` 0 次 → 两栏在这条流里不存在 |
| `lists.docx` | python-docx（`write_list_docx`） | 编号的三个来源一次摆开：走样式那三段（两份 `List Number` 与一份 `List Bullet`）段上**一个编号属性都没写**；写 `w:numPr` 那三段里有一段点 `numId="3" ilvl="1"`，而模板那九份抽象全是 `multiLevelType="singleLevel"`、每份只带一条 `w:lvl w:ilvl="0"`（那一级根本不存在）；最后一段点 `numId="77"`，`numbering.xml` 里没有这一条。另外三处实测：`numId 1 → abstractNumId 8`（**两本号分开编**）、圆点那级的 `w:lvlText` 是 **Symbol 字体的 `U+F0B7`**（字体名写在同级的 `w:rPr/w:rFonts` 上），而 `w:lvl` 里还有一条 `<w:pStyle w:val="ListNumber"/>` 反指回样式表 —— 样式与编号是一个环 |
| `lists-lo.docx` | LibreOffice（`lists.docx` → .docx，同一个格式重写） | 重写一次每段都换了样子：编号**段上写一份、样式里那份也留着**（`both` 三段），级别补齐九级、`ilvl` 也写出来了，`w:ind` 从 `left` 换成 `start`、`lvlJc` 也从 `left` 换成 `start`，抽象上那三个 `nsid`/`tmpl`/`multiLevelType` 一个都不写（`written` 是空表），号改成自己那本（`numId N → abstractNumId N`），**而那个不存在的 77 被改写成 `numId="0"`**（0 这一条同样不存在）|
| `lists.odt` | LibreOffice（从 `lists.docx`） | 换 ODF 的形状：第二级是**套两层 `text:list`** 表达出来的（套在里面那一层连样式名都不写，`chain` 交 `[WWNum3, null]`），段上只剩样式名 P1..P4 而列表样式名挂在样式上（`text:list-style-name`），级别是 **1 基的 `text:level`**（docx 那边是 0 基的 `w:ilvl`），十份 `text:list-style` 定义**全在 styles.xml**（`in_content` 0、`in_styles` 10），而那个解不开的 77 在这里写成 `text:list-style-name=""` —— 空串是这一族说「不套列表」的写法，与「点了一个没有的名字」不是一回事 |
| `lists.rtf` | LibreOffice（`lists.docx` → .rtf） | 列表的第三种存法：号写在**段上**（`\ilvl0\ls4` 五段各一对，紧挨着还有一份段自己的 `\li360\fi-360`），段的号先落 `{\listoverride\listidN\listoverridecount0\lsN}` 那本号（`listoverridecount` 说的是「这一条覆写了几级」，实测 0），号本再点 `{\list\listtemplateidN …}` 那一份定义 —— 而**那份群自己的 `\listid` 写在最后**（按 `{\list\listid` 抓一条也抓不到）。级上不写 `\ilvl`（级别号＝第几条 `{\listlevel`），九级全写（7 × 9 = 63 级）；每段正文前还有一句 `{\listtext\pard\plain  1.\tab}` —— 那是**生产者算好写进流的标签**，圆点那一段写的 `\f7` 与定义里点的 `\f1` 还不是同一个号。见事实 58 |
| `tables-lo.docx` | LibreOffice（`tables.docx` → .docx，同一个格式重写） | 那三本账里第一本变了：`w:tblW` 从 `type=auto w=0` 换成实数 **`8640 dxa`**，另外补出 `w:jc=start`、`w:tblInd=108`、`w:tblLayout=fixed` 与一个**空的** `w:tblCellMar`；网格与每格那两本**一字不差**（`4320` 与 `4320`），`w:tblLook` 的 `val` 从 `04A0` 变成小写 `04a0` |
| `shaded.docx` | python-docx（`write_shaded_docx`） | 一张 2×3 的表，三格各带一样：`w:shd`（`val=clear color=auto fill=FFFF00`）、`w:tcBorders/w:top`（`double sz=6 space=0 color=FF0000`）、`w:vAlign="bottom"`；另外三格只有 `w:tcW` —— 「什么都没设」必须留着当对照 |
| `shaded-lo.docx` | LibreOffice（`shaded.docx` → .docx，同一个格式重写） | **每一格**都被补上一个**空的** `<w:tcBorders></w:tcBorders>`（六格里五格是「元素在而一条边都没有」），`w:shd` / 那条 `w:top` / `w:vAlign` 的值一字不改（连 `FFFF00` 的大小写都保住了），只把属性顺序换了 —— 所以 `borders_present` 与 `borders` 要分两个键交 |
| `shaded.odt` | LibreOffice（`shaded.docx` → .odt） | 同一批字的第三副账：格子上只有 `table:style-name`（按地址起的自动样式 `表格1.A1`…`表格1.C2`，六份全在 content.xml），底色变成小写带 `#` 的 `fo:background-color="#ffff00"`，那条双线变成 `fo:border-top="2.25pt double #ff0000"` 加一条记每根线多宽的 `style:border-line-width-top`，垂直对齐是 `style:vertical-align="bottom"`，而**六格都带一份 `fo:padding-*`**（默认值也写出来）—— 见事实 57 |
| `toc.docx` | LibreOffice 的 **docx 导出器**（把目录注进 `notes.docx` 再让它照抄） | 真目录：`<w:sdt>` + `<w:docPartGallery w:val="Table of Contents"/>`，级别在域指令文字里 —— LibreOffice 把引号写成 `&quot;`，所以 `TOC \o "1-2" \h` 要还原实体才读得对 |
| `toc.odt` | LibreOffice（从 `toc.docx`） | 同一件东西的另一副面孔：`text:table-of-content`（名字 `目录1`）、级别在 `text:table-of-content-source/@outline-level="2"`，另外**十级条目模板全写出来**（`entry_templates` 报的是文件写了几个，不是用上了几级） |
| `toc.rtf` | LibreOffice（从 `toc.docx`） | 目录的第三种写法：没有 OOXML 那个 `w:sdt` 壳，也没有 ODF 那个 `outline-level` 属性，只有流里的一条域 `{\*\fldinst { TOC \\o "1-2" \\h}}` —— 开关前面的反斜杠**成对写**（单个会开出一个控制字），解掉那一对之后与 `toc.docx` 的 `w:instrText` 逐字相同。全文两条域（这一条 TOC 与目录条目上那一条 HYPERLINK）、`line_count` 9、`skipped_destinations` 120 |
| `comments.docx` / `comments.odt` / `comments.rtf` | python-docx 写两条批注，LibreOffice 转 ODF 与 RTF | 批注的三种存法：docx 有 `word/comments.xml` 那个部件（作者与 ISO 日期都在 `<w:comment>` 的属性上）、odt 的 `office:annotation` **嵌在正文段里面**、RTF 分两格写 —— `{\*\atnauthor 名字}` 在前、`{\*\annotation 正文}` 在后，注自己带一个号 `{\*\atnref N}`（与锚区两头 `{\*\atrfstart N}` / `{\*\atrfend N}` 同一个数）。两条注故意让第二个作者是中文名「刘奇」：**LibreOffice 的 RTF 导出把这个名字写成两个问号**，而它自己的 docx 导出照抄 —— 生产者的差，按各家的文件交 |
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
| `forms-hier.pdf` | **pikepdf 挂出来的**（`office_fixtures.py` 的 `write_forms_hier_pdf`，底本 `notes.pdf`）；写完用 pypdf 独立读回一遍 | 表单那一份账要的几种形状，编辑器没一个肯写：`/FT /Tx` 与 `/Ff 4` 只写在祖父 `Person` 上（`Address` 往上跳一跳、`City` 跳两跳才拿到）、`/Kids` 三层、`/Opt` 的两种合法写法各一份（成对 `[[1 一] [2 二]]` 与摊平 `[(甲) (乙) (丙)]`）、`/V` 的三种情形（写空串 / 写成数组 / 整个没写）、两条 Widget 同时挂在页的 `/Annots` 上。**这一份不是编辑器导的** —— 见事实 62 |

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
40. **那张纸：三家三种单位，换成 0.01mm 的整数才能对表**（五份件 × 三家 = 十四对）。
    docx 与 RTF 写 twips（1/1440 英寸）：`w:pgSz w="12240" h="15840"`、
    `\paperw12240\paperh15840\margl1800`；odt 写**自带单位**的十进制串：
    `fo:page-width="21.59cm"`。换算不用浮点 —— 两个读者会在最后一位上各说各话，
    所以两边都走「十进制精确展开 + 乘分数单位 + 逢半进一」的整数式子
    （`round()` 在 python 里是**逢半取偶**，正好会在 .5 上分家，故不用它）。
    12240 twips 与 21.59cm 都换成 21590（0.01mm），十四对全部如此 —— 这是这条链的地基。
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
    A4 的短边 OOXML 与 RTF 写 **11906 twips** → 21001（0.01mm），LibreOffice 的 ODF 又
    照抄成 `21.001cm` → 同一个 21001；长边 16838 twips → 29700。
    **这三份 A4 件全部落在 21001×29700 上，三家互相一致** —— 但「210mm×297mm」那个名字谁都够不着。
    这就是这一支**不报纸张叫什么**的原因：查表认「A4」要在 0.01mm 上开容差，
    而开了容差就得回答「那 JIS B5 与 ISO B5 差 4mm 算不算同一张」，
    不如把 21001×29700 这个数交出去，让人自己认。
    同一份件还量出两件事：横过来的那一节在 OOXML 与 ODF 里都写着
    （`orient="landscape"`、宽高对调、边距 1.5cm → 1499），
    而 LibreOffice 的 **RTF 导出整份文件一个 `\landscape` 都没有** ——
    那一副里只剩文档默认的纵向，所以它的 `orient` 只能是 null、`papers` 只有一条。
    两份件的这种差是**文件的差**，不是读者的差：第二个读者与 Rust 在同一份件上读到同一个东西。
42. **合并格让「这张表几个格子」变成两家不同的答案**（`tables-merged.docx` / `.odt`）。
    两张视觉上完全一样的表（2×3，第一行的前两格合成一格）：
    OOXML 把被合掉的那一格**整个不写** —— 第一格带 `w:gridSpan="2"`，那一行只有 2 个 `w:tc`；
    ODF 把被盖住的那一格照样写出来（一个空的 `table:covered-table-cell`），
    同时在第一格上写 `number-columns-spanned="2"` —— 同一行 3 个格。
    纵向合并也是两种写法：OOXML 在**两头上**都写（起头 `vMerge val="restart"`、
    接上去那格 `vMerge` 不写值 = `continue`，那一格照样在、字是空的），
    ODF 只在起头那格写 `number-rows-spanned="2"`，接上去的那格是 covered、什么都不带。
    所以 `office-doc` 交两本账：`tables[].rows` / `cells` 是 `descendants` 数出来的
    （那是「这份文件里有几个行/格标记」，嵌套表也算进来），
    `tables[].grid` 走**直接孩子**（这张表自己几行、每行几个格、每格的字与合并）。
    没合并的 `tables.docx` / `tables.odt` 两本账恰好一致 —— 那份合并件才是这条分别的出处。
    合并与重复的数只交文件写了的：没写 null，不补 1。

43. **域指令里的开关成对写反斜杠，解掉之后 RTF 与 OOXML 是同一串字**（接 33，`toc.rtf`）。
    第 33 条说「两家的写法毫无共同点」，那份对照缺了一副：RTF。它既没有 `w:sdt` 那个壳，
    也没有 `outline-level` 那个属性，只有一条自报家门的域：
    `{\field{\*\fldinst { TOC \\o "1-2" \\h}}{\fldrslt {…}}}`。
    关键是那两个反斜杠**不是笔误**：RTF 里单个反斜杠开一个控制字，`\o` 就不是指令里的
    字母 `o` 了，所以文件必须写 `\\o`。读者把 `{\*\fldinst …}` 那一群按「不认识就跳」
    跳过（一个字不进正文），只**前瞻**解一遍，解完交出来的就是 `TOC \o "1-2" \h` ——
    与 `toc.docx` 那条 `w:instrText`（还原 `&quot;` 之后）**逐字相同**。
    于是「收几级」这把读取器两家共用一把，`levels` 两边都是 `1-2`；
    ODF 那边还是 `outline-level="2"` 那个数，不强行归一。
    这一族另有两条口径钉住：`fields` 数的是 `\field` 控制字出现几次（这份件 2 次：
    一条 TOC 与目录条目上那条 HYPERLINK），`contents.fields` 只留以 `TOC` 开头的那几条；
    有域却没指令（只有 `\fldrslt`）时空着交出来，不替文件编一条。
    `skipped_destinations` 仍然是 120 —— 前瞻不改游标也不改跳过标记，那笔诊断数才可比。

44. **批注的第三种存法：作者与正文分在两格里，日期谁都解不出来**（`comments.*` 三份 + `notes.rtf` / `toc.rtf`）。
    docx 把注放在 `word/comments.xml` 那个部件里（作者、时间都是 `<w:comment>` 的属性），
    ODF 把 `office:annotation` 嵌在正文段里面，RTF 则是流里的两格：
    `{{\*\atnid L}{\*\atnauthor 名字}\chatn{\*\annotation{\*\atnref 0}{\*\atndate 1743371367}正文}}`。
    三条口径都是量出来的，不是推的：
    **一、作者与注按文件的顺序配**（作者那一格在注之前），两家的条数各交一份
    （`comments` 与 `structure.annotation_authors`），配不上时看得见，不替文件对齐；
    **二、注自己带一个号** `atnref`，与锚区两头的 `atrfstart` / `atrfend` 是同一个数
    （两份件的 0 / 1 都对上了），所以「这条注钉在哪一段」有文件自己的号可查；
    **三、`atndate` 解不出来** —— 这两份件写的 `-2014723526` 与 `1743371367`
    都对不上同一批字的 docx 里那个 `w:date="2026-09-24T08:58:48Z"`（按 epoch 秒解一份
    得到 2042-04-04、另一份得到 2025-03-30，都不是），所以日期交 null、原样那串交在
    `date_written` 里，不挑一个历法冒充读懂了。
    还有一条是**生产者的差**：第二个作者的中文名「刘奇」在 LibreOffice 自己的 docx 导出里
    完好，到了 RTF 那一族就成了两个问号（`{\*\atnauthor ??}`）—— 那两个字面问号就是文件写的，
    照交，不去猜回来。注的字一个也不落进正文（那一群照旧整群跳过，`skipped_destinations` 没动）。

45. **换页在这一族有三种写法，而子串数会骗人**（`notes.rtf` / `toc.rtf` / `notes-hf.rtf` / `paper-a4.rtf`）。
    Word 的 `<w:br w:type="page"/>` 到了 LibreOffice 的 RTF 导出里是 `\pagebb`（「这一段之前换页」），
    整份文件一个 `\page` 都没有 —— 七份 RTF 件里 `\page` 全是 0 条，而 `notes.rtf` / `toc.rtf` /
    `notes-hf.rtf` 各有 1 条 `\pagebb`，与同一批字的 `notes.docx` / `toc.docx` 那一条 `w:br type=page`
    对得上（两家数法不同、答案相同）。子串数会把这些数字全弄错：`\pard` 里含 `\par`
    （`notes-hf.rtf` 全文 `\par` 子串 20 个，而**没被跳过的那一层**只有 4 条），`\sectd` / `\sectx`
    里含 `\sect`。所以这一支按**词边界**数六个词（par / line / page / pagebb / pbb / sect），
    六个键固定交出来（一个都没出现也交 0 —— 数过了没有，与没数是两件事），
    并且只在没被跳过的那一层数（页眉里那条 `\par` 不是正文的一段）。
    `page_breaks` 是三种换页词的和；`\sect` 是分节的**收尾**符，最后一节自己不带一个，
    两份两节的件（`notes-hf.rtf`、`paper-a4.rtf`）都只写 1 个 —— 所以只交 `section_breaks`，
    `sections` 仍然 null（+1 是推断，不是文件写的）。
    ODF 那一族的换页住在段落样式上（`fo:break-before="page"`），不在正文元素里 ——
    那一条见下一条（46），两家以前都读成 0。

46. **ODF 的换页写在段落样式上，不写在正文里**（`notes.odt` / `toc.odt` / `notes-hf.odt` / `protected.odt` 各 1 处）。
    正文里的 `text:p` 只带一个 `text:style-name="P2"`，`fo:break-before="page"` 坐在同一个
    content.xml 里那个 `style:style` 的 `style:paragraph-properties` 上（与 .ods 的数据样式
    同一类两跳）。以前这一条数的是 `text:soft-page-break` —— 那是渲染时落下的位置，
    七份 odt 件里一个都没有，于是四份明明换了页的件两家读者一起报 0（同一批字的
    `notes.docx` 报 1，因为 Word 把它写成正文里的 `w:br w:type="page"`）。
    现在两个键分开：`page_breaks` 走样式那一跳，`soft_page_breaks` 保留原来那条数。
    父样式链（`style:parent-style-name`）上也可能写这条属性，但四份件都写在自己身上，
    没有样本就不跟那条链。docx / odt / rtf 三家现在对 `notes` 与 `toc` 都报 1。

47. **打印设置按表交，缺的元素是 null 不是 false**（11 份 xlsx，两个生产者正好相反）。
    同一张表的打印那份东西在 `sheetN.xml` 里是三个平铺的元素：`pageMargins`、`pageSetup`、
    `printOptions`。openpyxl 只写第一个（左/右 0.75、上/下 1、页眉页脚 0.5 英寸），后两个
    **整个不存在** —— 那里面的开关是「没说」，不是 false，所以缺的元素交 null，
    属性一个也不补。LibreOffice 重写同一份东西把十二个 `pageSetup` 属性全写出来
    （连 `paperSize="9"`、`horizontalDpi="300"` 与 `verticalDpi="300"` 都不省），
    `printOptions` 另写五个。边距这一族照文件原样交，单位在每张表上说一次（`margin_unit: inch`），
    **不**折算成 office-doc 那一族的 0.01mm 整数：`header="0.5"` 与 `header="0.511811023622047"`
    正是两个生产者的差（后者是 13mm 换算成英寸的浮点串），折算一次就抹平了。
    另两族量过之后**没读**，键整个不在（不是空对象）：五份 LibreOffice 写的 .ods 里
    `style:page-layout-properties` 确有打印属性（`print-orientation` / `print-page-order` / `print`…，
    而且只有 Mpm3 那一份写了），可**表元素不点名自己的版式** —— `table:table` 上根本没有
    `style:page-layout-name`，剩下的只有 `PageStyle_5f_表名` 那条该生产者自己的命名约定，
    那不是规格里的跳；.xls 每张表确实写了一条 SETUP(0x00A1)（长 34 字节 = 17 个 u16，
    基准值是 9 100 0 1 1 2 300 300 + 两个 double + 1），但来回量过之后只认得出三个字段位：
    **改 xlsx 的属性让 LO 写成 .xls**，只有 `paperSize`（槽 0）、`scale`（槽 1）与那个旗标字
    （槽 10：`0x2` 那一位是竖排，`0x1` 那一位是 overThenDown）跟着动，dpi / copies / 边距 /
    draft / blackAndWhite / gridLines 六样 LO 的导入根本没接住；**反过来改 .xls 的槽位再让 LO 读回
    xlsx**，也只有这三样跟着动（槽 0 改 1 就报 `paperSize=1`，改 8 就报 8；槽 1 改 150 就报
    `scale=150`；槽 10 改 0 就横过来、改 1 就横排 + overThenDown、改 3 只换排序），槽 6/7 那两个
    看着像 dpi 的 300 与槽 2..4 与末尾那个 1 两个方向都不动 —— 只有一个读者（还是写出它们的那个）
    认得的字段，就还是没量过，所以这一族的键整个不在。

48. **图要跳三跳，而「图里画的是哪些数」有一半是文件没说的**（`chart.xlsx` 与 `chart-lo.xlsx`）。
    图不在 `sheetN.xml` 里，也不在表的关系表里一步就到：表 →（自己的关系表，`Type` 结尾是
    `drawing`）→ `xl/drawings/drawing1.xml` →（那张关系表，`Type` 结尾是 `chart`）→
    `xl/charts/chartN.xml`。两个生产者三种 Target 写法都在这两份件里（openpyxl 一路绝对
    `/xl/drawings/…`，LibreOffice 一路相对 `../drawings/…`、`../charts/…`）。归位是按表的：
    两张图（柱形两条系列、折线一条）都挂在 `数据` 那张表上，另一张表报 0。
    跨生产者对不齐的三件事都照文件交，不归一化：
    * 同一段格子的引用串 —— openpyxl 写 `'数据'!B1`（表名带引号、地址不带 `$`），
      LibreOffice 写 `数据!$B$1`；
    * 同一批文本类目的走法 —— openpyxl 写成 `c:numRef`，LibreOffice 写成 `c:strRef`；
    * 轴 id —— 一家写 10/100，另一家写 27806046/9803817，那是各自内部的编号，
      所以只交个数，不替它们配。
    缓存才是这一条的真岔口：**openpyxl 一条 `c:pt` 都不写**（`cached: false`，于是
    「图里画的是哪些数」判不住，只能交出引用），LibreOffice 把 2 个点连值一起写进来
    （`一月`/`二月`、10/25）并自报 `ptCount`；两家都自报过的写法才谈得上对不上，所以
    `written` 与 `points` 一起交，`whole` 是它们合不合。标题目前两家都写成 `c:rich` 的字面量
    （`逐月收支`），`c:strRef` 那一条支路只有镜像与单测走过，没等到真生产者写的件。
    ODS 的图是嵌入对象（`Object 1/content.xml` 那一堆部件），.xls 走 BIFF 的对象链 ——
    两家都没有第二个读者量过，所以那一族的 `charts` 键整个不在。

49. **规则那一份账里，两样东西不能替文件补：`dxfId` 与 `priority`**（`rules.xlsx` 与 `rules-lo.xlsx`）。
    条件格式的规则**不写样式**，只写一个下标 `dxfId`，字与色住在 `xl/styles.xml` 的 `dxfs` 里
    （与格子的 `s=` 指向 `cellXfs` 是同一类两跳）。所以一条规则要同时说清三件事：文件写的下标是几
    （`written`）、这一跳指没指到（`found`，`dxfId="5"` 而 dxfs 只有两条时它就是 false）、
    指到的那一条里有哪些元素路径（`font/b`、`font/color` 这种一层到底的写法）。
    两份件对同一条 dxf 给的东西不一样：openpyxl 两个元素，LibreOffice **五个**（自己补了
    `name`/`family`/`sz`）—— 这正是不能把两边折成「加粗的深红」那种共同形状的原因，折了就看不见
    谁补了谁。
    `priority` 更是各排各的：同一批四条规则，一份写 1/2/3/4，另一份写 2/3/4/5（LibreOffice 重排过），
    所以只交不比 —— 两家一致的那部分是**类型、范围与公式**，测试钉的也只有这些。
    颜色的 `rgb` 串按文件写的交：同一个白，一份 `00FFFFFF`、一份 `FFFFFFFF`，alpha 那两位不归一化。
    数据验证这一份更直白：容器自报 `count="3"`，与实际条数一起交（`whole`）；每条交范围、`type`、
    `operator` 与 `formula1`/`formula2` 的**原文**，另把写着的属性整个交出来，于是这几件事都看得见 ——
    `allowBlank` 一家写 `1` 一家写 `true`（与保护那份账同一种分歧）、一家给 list 写了
    `operator="equal"`（规范里 list 不需要）、一家给自己没有第二条公式的那两条补了 `formula2=0`、
    还有一家把 custom 的公式写成 `ISNUMBER(B2)` 而 openpyxl 写 `=ISNUMBER(B2)`。
    第二条范围那条是**一条 `sqref` 里两段区间**（`A2:A6 B2:B4`）：空格分开，照文件交，不替它拆成两块。
    第二张表两样都没有 → `conditional` 是空表、`validations.written` 是 null（「没写」）而 `found` 是 0。
    ODS 的条件格式住在 number 样式与 table 样式那一套上，.xls 是 BIFF 的 CONDFMT/DCON 记录 ——
    两边都没有量过的第二个读者，所以那一族的 `rules` 键整个不在。

50. **同一页的图按关系表认，引用串跨生产者不可比**（`deck-chart.pptx` 与 `deck-chart-lo.pptx`）。
    pptx 的图与 xlsx 那一份共用同一套 `c:ser` 形状，但换了宿主就多两件事：
    * **只认页自己关系表里 `Type` 结尾是 `chart` 的那几条**。LibreOffice 重写这一族时往
      `ppt/charts/` 里另塞了 `style1.xml`、`colors1.xml` 这些部件（那份件里这个目录一共 8 个条目），
      按目录数就会数出六张图而实际两张；两家的 Target 都是相对的（`../charts/chartN.xml`），
      但顺序不同（LO 把 chart 排在 slideLayout 前面），所以归位靠 kind，不靠下标。
    * **引用串指的是哪张表**：python-pptx 写 `Sheet1!$B$1` —— 那是图自带的那张内嵌工作簿
      （`ppt/embeddings/Microsoft_Excel_Sheet1.xlsx`）里的表名，不是演示文稿的表；而 LibreOffice
      的 pptx 导出在同一个位置写字面量 `label 0`、`categories`、`0`、`1`。同一条系列的
      `c:val/c:numCache` 里 10 与 25 一字不差 —— 所以两边都交、不替它们对成一个答案：
      一家丢了引用留着数，一家两样都有，这正是要「按文件自己写的报」的地方。
    * 轴 id 更不可比：python-pptx 写**负数**（`-2068027336`），LibreOffice 写正数，饼图两家都不写轴，
      于是只交个数。
    `deck.pptx` 与 `deck-lo.pptx` 那一对另外钉住两件事：同一段字在一家是一个 `a:t`、在另一家是三个
    （`新增两台 ` / `64 ` / `核应用服务器`），所以 `text_runs` 一份 3 一份 5，而 `paragraph_total`
    两份都是 3 —— 段落这一级才是稳定的，把 run 并成段里的字才谈得上比对；还有 `p:sldSz` 那个
    `type` 属性，python-pptx 写 `screen4x3`、LibreOffice 同样的 cx/cy 把它省掉，那一家就交 null
    （以前替它编一个 "custom"，那是把没写的当成写了）。
    ODP 的图已经走进那个目录了（见下面第 51 条），只有 `.ppt` 还住在记录树里 —— 所以那一族的
    页上不带 `charts` 键。

51. **ODF 的图要先走进那个目录**（`chart.ods` 与 `deck-chart.odp`）。
    宿主文档里只有一条 `draw:frame` → `draw:object`，`xlink:href` 指着 `Object N`（一个目录），
    图的内容在那个目录自己的 `content.xml` 里 —— 与 OOXML 那两家「表 → 关系表 → 画法部件 → 图部件」
    是两种链接法。`draw:frame` 住在**它所属的** `table:table` / `draw:page` 里面，所以按表、按页归位
    做得到；备注那个 frame 没有 `draw:object`，于是不会被当成图（但它的名字占了号：ODP 里第一张图的
    frame 叫 `Chart 2`）。这一族的三件事与 OOXML 不同，都不替它对齐：
    类型写在每条 `chart:series` 上（`chart:bar`，饼图是 `chart:circle`），`chart:chart` 自己身上没有；
    点数有一条自报的 `chart:data-point@chart:repeated`（一条顶两个点 —— 与 `number-columns-repeated`
    同一个惯例），所以 `point_elements` 与 `points_written` 分开交，两份件的同一批点一边写 1/2、一边写 2/2；
    地址是第三种写法：ODS 写 `数据.B2:数据.B3`（点分隔、不带 `$`），ODP 那份干脆指到图自己抄的
    `local-table.$B$2:.$B$3`（转一圈之后回不到稿子的数据了）。
    那张 `local-table` 按格子抄，`number-columns-repeated` **不铺开**（一个自报 16384 的文件会凭空长出
    上万格），自报的那个数原样跟着交。
    .xls 的图还在 BIFF 的对象链上，本机没有一个读者量过 —— 那一族的表不带 `charts` 键。

52. **窗口冻在哪里、打出来页眉上有什么**（`view.xlsx` 与 `view-lo.xlsx`）。
    这两件事住在同一张表上的两个元素里（`sheetView` 与 `headerFooter`），而这两份件是
    **同一个格式重写同一个格式**得到的（xlsx → xlsx，不经 .ods），所以差的全是导出器自己的手笔：
    * 同一个开关的第三种布尔拼法又出现了：openpyxl 写 `showGridLines="0"` 与 `tabSelected="1"`，
      LibreOffice 写 `"false"` / `"true"`，而且把文件里根本没提的十几个开关也全写出来（一份四个属性、
      一份十五个）—— 所以属性照文件交，不折成「同一个布尔」。
    * `pane` 的 `state` 把**冻结**与**拆分**分开：`frozen` 与 `split` 是两回事（Excel 里两个不同的命令）。
      实测 LibreOffice 的 xlsx 导出**把 split 那一个 pane 整个丢掉**，而同一份件里 frozen 那条一字不动地
      留着（`xSplit="1" ySplit="2" topLeftCell="B3"`）—— 这一条是量出来的重写损失，不是从规范里推的，
      所以两份件都照各自的文件交，不替 LibreOffice 把丢了的说成「本来就没有」。
    * `selection` 一份三条、第一条例外不点 `topLeft`，另一家四条人手一份、每条多一个 `activeCellId="0"`。
      条数与点名都交，不数成一个答案。
    * 页眉页脚那一串字是一个小型标记语言，但只认文件自己标的：`&L` / `&C` / `&R` 是分段记号，
      **`&&` 是一个货真价实的 `&`**（不是记号），`&"Calibri"` 是一整码（里面带引号，按字符数会切错），
      `&A`/`&P`/`&N`/`&D` 这些是字段。所以按码位扫一遍，分段与字段分开交，含义一个不猜。
      LibreOffice 在每一段前面补一个 `&"Calibri"`（openpyxl 一个不写），两份件的**字面量不同而
      「写了几段字」相同**（都是 3）—— 这种「一层可比一层不可比」正是两边都交的理由。
    * 「没写」与「写了空的」不能并成一谈：第三张表 openpyxl 连 `headerFooter` 这个元素都不写
      （`present: false`、`text: null`），LibreOffice 给每张表都写出这个元素、段落里是空的
      （`present: true`、`text: ""`）。同一家内部也不一致 —— 它写出空的 odd/even 两段，
      却把 `firstHeader` / `firstFooter` 省掉，因为 `differentFirst` 是 false。
    ODF 的窗口状态与页眉页脚在页面样式那一套里（与 `print_setup` 是同一道没有的跳），
    .xls 的那些开关在 BIFF 的 WINDOW1 / WINDOW2 与 HEADER / FOOTER 记录里，两边都没有能核对的
    第二个读者 —— 所以这两族的两份账都是**键整个不在**，不是 0、也不是 null。

53. **列宽、行高、筛选与表对象：重写一次就换一套数**（`size.xlsx` 与 `size-lo.xlsx`）。
    这两份也是 xlsx → xlsx 的直接重写，量出来的第一件事就是**没有一个数是对得上的**：
    * 同一列宽 `22.5` 变成 `20.47`、藏着那列的 `4` 变成 `3.64`；同一行高 `40` 变成 `39.75`、
      `8` 变成 `7.5`。所以「这一列到底多宽」没有唯一答案，只能把文件自己写的数交出去 ——
      折算或取整就是替两家编一个共同的数。
    * 「默认列宽」两家写的**不是同一个属性**：一家 `baseColWidth="8"`，另一家
      `defaultColWidth="7.7734375"`，各自都没有对方那个键。
    * 一家只给说过话的行写高度（三行里两行），另一家**三行全写**（连没说过话的那行也补 `ht="18"`），
      所以这里有三本账：`elements`（几行）、`with_height`（写了 `ht` 的几行）、`spoken`
      （`ht`/`customHeight`/`hidden` 里说过任何一个的几行），另附说过话的那几行原样。
    * `col` 一条可以顶很多列（按 `min`/`max`），所以「几条」与「盖住几列」分开交，
      而且**一条也没有时交 null 而不是 0** —— 没有 col 元素时「这张表几列宽」是判不住的。
    * 筛选有两种范围：表上那一条 `autoFilter ref="A1:C3"` 与表对象自己带的
      `A1:B3` 在同一张表上并存，**它们不是一回事**，两个都交；被筛掉的值（「甲」）两家一字不差，
      但 `filterColumn` 上那两个开关（`hiddenButton` / `showButton`）只有一家写，
      而 LibreOffice 还会在 `sheetPr` 上写一个 `filterMode`（没筛的那张表也写 `false`），
      openpyxl 一个都不写。
    * 表对象那一跳是关系表那一类（`tableParts` 只写 id）：名字 `台账`、范围、`headerRowCount`
      两家都一样，LibreOffice 多补 `totalsRowCount`/`totalsRowShown` 与三个等于默认的样式开关
      （2 个属性变 5 个），**反倒不写 `tableParts` 那个 `count="1"`** —— 自报数与实际条数并排交，
      没有自报数时 `whole` 为 true（那句「没说」不算说错）。
    * 表对象的**列名是文件自己写的**：openpyxl 拿范围第一行的字当列名，于是第二列叫 `10`，
      两家都照抄 —— 这不是笔误，是这一族真的会写成这样。
    ODF 与 .xls 这三份账同样没读（键整个不在）：ODF 的列宽行高与页眉页脚一样住在样式里，
    .xls 的在 BIFF 的 COLINFO / ROW 与 FILTER 记录里，没有一个读者能核对。

54. **这一段自己排成什么样、这一节排成几栏**（`para.docx`、`para.odt`、`para.rtf`）。
    同一份四段话在两家手里住在**两个地方**：OOXML 就写在段自己的 `w:pPr` 上，ODF 段上只留一个
    样式名、属性在样式表的 `style:paragraph-properties` 里（一跳），所以一份账要按两种走法各记：
    * 「没写」这一种必须有，否则「这一段什么都没说」与「读成 0」分不开：docx `checked` 6 而 `listed` 4
      （差的那两段一条 `w:pPr` 都没有）；ODF 那边第三段点名的 `Standard` 住在 styles.xml，
      而这一族只看得见 content.xml 里的那一份 → `resolved: false`、`written: null`，不借邻居的数。
    * 对齐的**值照文件交**：同一段 docx 说 `both`、odt 说 `justify`，第二段 docx 说 `right`、odt 说 `end`
      —— 折成一个词就是替两家编一个谁都没写过的词。
    * 「1.5 倍」与「固定 18 磅」这三家各用一个不同的东西去分别它：docx 两处都是 `w:line="360"`，
      分别靠 `lineRule`（`auto` / `exact`）；ODF 换成 `fo:line-height="150%"` 与 `"0.635cm"`，
      分别靠单位（同一个数换个单位就不是同一件事）；RTF 用**符号加另一个开关**
      （`\sl360\slmult1` 与 `\sl-360\slmult0`）。三个都不换算。
    * 缩进有第二种单位，而那第二种单位只有一家在写：docx 把 `w:left="0"` 与 `w:leftChars="200"` 并排
      （`chars_written` 只说「出现了 Chars 后缀」，不替它换成毫米），ODF 那一段干脆改用 `loext:` 前缀
      （`loext:margin-left="2ic"`、`loext:text-indent="1.5ic"`，`fo:` 上什么都没有 —— 只按 `fo:` 挑就当这段没缩进）。
      所以 ODF 那份属性表**留着前缀交**：`fo:text-indent` 与 `loext:text-indent` 是两个属性，
      按局部名合并就会互相盖掉。悬挂缩进也一样，ODF 是一个**负的** `fo:text-indent`（`-0.635cm`），
      docx 另开一个 `hanging="360"`。
    * 分栏两份账不同形：docx 一节一条 `w:cols`（模板自带那一节只有 `w:space="720"`、没有 `num` ——
      那就是「一栏」的写法，于是 `sections` 2、`written` 2、`multi` 1 是三本账）；ODF 是 `text:section`
      点名 family=section 的样式，栏在它的 `style:section-properties` 里（`fo:column-count="2"`、
      `fo:column-gap="0.751cm"`），每栏还各写一份 `style:column`，相对宽度是 `32767*` 与 `32768*`
      （两半不相等，是生产者自己凑的整数，不是我们摊的）。`style:dont-balance-text-columns="true"`
      交在 **`dont_balance`** 这个键上 —— 键叫 `balance` 就会把那个 `true` 读成反话。
    * RTF 这一族**两个键都不交**，理由就量在这份件里：正文五段每段前面都把样式默认重发一遍
      （`\sl276\sb0\sa200\ltrpar…` 段段都有，「这一段写了什么」与「它继承了什么」分不开）；
      第二段那个右对齐整个没写出来（全文 `\qr` 0 次，`\qj` 与 `\qc` 各 1 次）；按字数的那两个缩进被抹平成
      `\li0\fi0`；而全文 `\cols` 0 次 —— 两栏在这条流里不存在。归属判不住就不交：键整个不在，
      不是 0、也不是 null。`.doc` 同理（段格式住在那张表流里，这一族的读者不走过去）。
    * 「这份文档有几段」在 ODF 有**两个诚实的数**：注是坐在带它的那一段**里面**的
      （`text:p > text:note > text:note-body > text:p`），走到 `text:p` 就停数得 4，
      整棵树数得 7（`notes-end.odt` 实测：三段正文里嵌着三条注，每条注自己也是一段）。
      LibreOffice 自己写在 meta.xml 的 `paragraph-count` 是那个 **7** —— 所以这一族的段格式
      账取 4 这一份清单（与它自己的 `structure.paragraphs` 同一份，否则 `index` 对不上），
      而生产者那个 7 原样留在 `statistics.producer` 里并排交。这一条是这批对账唯一的失败项
      量出来的：两边各数各的时，镜像数到 7、读者数到 4。

    命名空间声明在这两族都不算属性（`xmlns=` 与 `xmlns:fo=` 说的是「这个名字怎么读」，不是文件给的属性），
    标准库那个 XML 读者本来也不把它们放进 attrib —— 这条口径是上一批对账失败量出来的：
    `<table xmlns="…spreadsheetml/2006/main">` 让 Rust 多报一个 `xmlns` 键，而 `size.xlsx` 与
    `size-lo.xlsx` 的表根元素上都写着它（一家把它放在最后一个属性、另一家放在第一个）。

55. **这一段是不是列表项、第几级、编号从哪来**（`lists.docx`、`lists-lo.docx`、`lists.odt`）。
    「把文档里的列表读出来」是常见需求，而这一问在 OOXML 里**没有唯一的地方**可看：
    * 三个来源。段自己写 `w:numPr`；或者段点名的那份样式替它写（Word 的 `List Number` 就是这样，
      python-docx 照抄 —— 这份件六段里三段自己一个字都没写）；或者**两边都写**
      （LibreOffice 的 docx 导出把编号搬到段上、样式里那份留着，于是 `from` 读到 `both`）。
      只看段就会漏掉一半，所以每条都写清是从哪来的。
    * 解一个号要跳三跳（`w:num` → `abstractNumId` → `w:abstractNum` → 对得上的那条 `w:lvl`），
      而**两本号是分开编的**：实测 `numId 1 → abstractNumId 8`、`5 → 7`。
      每一跳各给一个布尔（`resolved` / `abstract_found` / `level_found`），
      拿上一跳的「到了」当下一跳的「到了」就是把没读到的东西当成读到了。
    * 文件没点的那一级不替它填。模板那九份抽象全是 `singleLevel`、只带一条 `ilvl="0"` 的 `w:lvl`，
      所以点 `ilvl="1"` 的那段拿 `level_found: false`，而不是第 0 级的数；
      点名点空也分两种写法：这边是 `numId="77"`，同一份件被 LibreOffice 重写后变成 `numId="0"`，
      两条 `numbering.xml` 里都没有 —— 两家各指各的「没有」，都按写的交。
    * 圆点不是 `•`：那一级的 `w:lvlText` 写的是 Symbol 字体里的私用区码位 `U+F0B7`，
      字体名挂在同级的 `w:rPr/w:rFonts` 上。不带字体名交回去，那就是一枚看不见的方块。
      而 `w:lvl` 里还有一条 `w:pStyle` 反指回样式表 —— 编号与样式是一个环，
      读者只能挑一条边走，挑了哪条要写在账上（这里走「段 → 样式 → num → abstract → lvl」）。
    * ODF 换了形状：第二级不是属性而是**套两层 `text:list`**（深度由读者数出来，
      套在里面那一层连样式名都不写，`chain` 把每一层写了什么照实交）；两家的级别**数法不同**
      （`text:level` 从 1、`w:ilvl` 从 0），所以两个都不换算；那十份 `text:list-style` 定义
      全在 **styles.xml**，段样式在 content.xml —— 这一跳跨部件，每条都带 `style_part` / `list_part`
      说清在哪份件里解到的（`in_content` 0、`in_styles` 10）。顺带一提：已有的
      `structure.list_styles` 数的是 content.xml 里的那一份，所以它一直是 0，不是这份文档没有列表样式。
    * 「定义了但没人用」是常事：`notes.odt` 一份列表都没套，样式表里躺着十份定义；
      `lists.docx` 的 `numbering.xml` 带着九条，正文只点了四条（`referenced` 逐条说）。
    RTF 那一族的列表住在 `\listtable` / `\ilvl` / `\ls` 那一套里，每段前面还带一个
    `\listtext` 群（实测 `lists` 那批件转出的 RTF：`\ls4`、`\ilvl0`、标签 `1.` 就写在流里，
    `\listtable` 里 7 条 `\list` × 9 级 `\listlevel`）—— 量过了但还没读，所以这个键整族不交。

56. **这张表说自己多宽**（`tables.docx`、`tables-merged.docx`、`tables-lo.docx`、两份 `.odt`）。
    「把表排多宽」这件事在 OOXML 里有**三本账**，而且它们天生不会相等：
    * `w:tblPr/w:tblW` 是表自己说的。python-docx 写的是 `type="auto" w="0"` ——
      那是一个**写了等于没写**的值，但它确实写在文件里，所以照交（`kind` 与 `w` 各一个键，
      连 `mm=0` 也不当成「没说」）；LibreOffice 把同一份件重写一遍之后这里变成 `8640 dxa`，
      还顺手补出 `w:jc`、`w:tblInd=108`、`w:tblLayout=fixed` 与一个**空的** `w:tblCellMar`。
    * `w:tblGrid/w:gridCol@w` 是网格那一本：两家**一字不差**（两条 `4320`）。
    * 每一格自己的 `w:tcPr/w:tcW` 是第三本。`tables-merged.docx` 最能说明问题：
      网格是三条 `2880`，而横向合并那一格自己写 `5760`（它盖住的两列之和，`w:gridSpan="2"`）——
      「这张表几列、每列多宽」与「这一行几个格、每格多宽」是两个问题，两本都要交。
    * 行与格只数这张表**自己的直接孩子**：套在格里的另一张表不会把上面那本的账撑大。
    * twips 走「那张纸」同一条整数式子换成 0.01mm，于是跨家族可比：
      docx 的 `4320` 与 ODF 的 `7.62cm` 都是 `7620`，`8640` 与 `15.24cm` 都是 `15240` ——
      这是这一批里唯一一组两家读者、两种单位算到同一个数的 pin。
    ODF 那一族没有表级宽度元素：
    * 一条 `table:table-column` 可以**顶好几列** —— 实测两份列写成**一条**元素带
      `number-columns-repeated="2"`（合并那张是 `3`），所以 `column_elements`（几条）与
      `covered`（盖住几列）分开数，差一条就是差几列。
    * 宽度一跳在 `style:style`（family=table-column）的 `style:table-column-properties/@style:column-width`，
      表自己的总宽在 family=table 的样式的 `@style:width` 上。
    * 与编号那一批**正相反**：这里的列样式与表样式都写在 **content.xml**（`style_part` 每条都点名），
      所以上一批学到的「定义在 styles.xml」不是一条家族规律，只是那一族生产者的选择。
    * 列样式名是照表名拼的（`表格1.A`、`表格2.A`）—— 生产者的命名约定，照原样交，不当成结构。

57. **这一格的底色、边与垂直对齐：一家写在格子上，一家要再跳一跳**（`shaded.docx`、`shaded-lo.docx`、`shaded.odt`）。
    OOXML 把三样都写在 `w:tcPr` 里，属性名按文件写的交（`w:shd` 三个属性、`w:tcBorders` 下面每条边一个元素、
    `w:vAlign`）；LibreOffice 重写同一份件时给**每一格**都补一个**空的** `<w:tcBorders></w:tcBorders>`
    （六格里五格是「元素在而一条边都没有」），所以 `borders_present` 与 `borders` 分两个键交，
    而值本身一个字都没改（`FFFF00`、`FF0000`、`sz="6"` 的双线都照抄，只换了属性顺序）。
    ODF 那一族格子上**只有名字**（`table:style-name`），三样都在一跳之外的 `family=table-cell` 样式里：
    * 样式名是**按地址**拼的自动样式：`表格1.A1` … `表格1.C2`（六格六份），与列样式同侧，
      实测**全在 content.xml**（`cell_styles_in_content` 6、`cell_styles_in_styles` 0）——
      编号那批把定义搬去 styles.xml 不是家族规律，是那一位生产者的选择，这一条又一次证明它。
    * 底色是 `fo:background-color="#ffff00"`：**小写、带 `#`**，而 docx 那边是 `w:fill="FFFF00"` 大写不带 `#`。
      同一个格式两种写法，两边各按各的交，谁也不替谁归一化。
    * 边是 `fo:border` 或四条 `fo:border-<方位>`，一条值里塞着「宽度 样式 颜色」三段。同一条双线两种记法：
      docx 记**每根** `sz="6"`（八分点），ODF 记**三根合起来** `2.25pt`，再另写一条
      `style:border-line-width-top="0.026cm 0.026cm 0.026cm"` 说每根多宽 —— 后者不是边，所以不收进
      `borders`（只留在 `written` 里）。因为 shorthand 也在边的名字里，「写了几条边」与「有没有一条真有字」
      要分开：实测六格每格都写了一条边，而其中五格写的都是 `none`，于是 `borders_present` 全 true、
      `lined` 只有一格 true。
    * `padded` 说这份样式有没有写 `fo:padding-*` —— LibreOffice **六格全写**，连 0 的默认值也不省
      （`tables.odt` 那张什么都没设的表十格也全写），所以「有几格写了底色」与「有几格被写过东西」是两个数。
    * 被合并掉的格子是 `table:covered-table-cell`，**另数一本**：`cell_elements` 是两种格子元素的总数、
      `covered_cells` 只数其中占位的那些。`tables-merged.odt` 实测两种占位写法：横向合并那格**连属性都不写**
      （`attrs: {}`、`style: null`、`written: null`），纵向那格还留着样式名 —— 「一格什么都没写」与
      「一格没被数」又不是一回事。跨了几列几行照原样留在 `attrs` 的 `number-columns-spanned` /
      `number-rows-spanned` 里，不并进「这一行几个格」。

58. **RTF 的号写在段上，那一句标签是生产者算好写进流的**（`lists.rtf`，LibreOffice 从 `lists.docx` 转的）。
    四步一跳一步布尔：段上的 `\ls4` → `{\*\listoverridetable` 里那一条（`{\listoverride\listid4\listoverridecount0\ls4}`，
    `listoverridecount` 是「这一条覆写了几级」，实测 0）→ 它点名的 `\listid` → `{\list\listtemplateidN …}` 那一份定义。
    * **定义群的 `\listid` 写在最后**：按 `{\list\listid` 去抓一条也抓不到（实测 0 条），
      而全文 `\listid` 有 14 次 —— 两份表各 7 次。整群读完才拿得到号。
    * 同一份文件里 `{\*\listtable` **带星号**而 `\listoverridetable` **不带** ——
      前瞻只挂一条路径就会一份读到、一份读不到；两条都挂之后 `skipped_destinations` 一个字也没变。
    * 级上**一个 `\ilvl` 也不写**，所以级别号是「这份 list 里第几条 `{\listlevel`」（读者按顺序给的号），
      而且每份定义**九级全写**（7 × 9 = 63 级），每一级的 `levelnfc` / `leveljc` / `levelstartat` /
      `levelfollow` 与 `{\leveltext …}`、`{\levelnumbers …}` 按写的字节交（`\'02\'00.;` 那种占位记法
      不替它解成格式串）。
    * 圆点那一段是跨家族最干净的一条对照：`lvlText` 在 docx 是 Symbol 字体的 `U+F0B7`，
      在这里定义里点 `\f1`（字体表里那一个才是 Symbol），而**文件自己算出来的标签点的是 `\f7`**
      （`{\listtext\pard\plain \f7 \u-3913\'3f\tab}`）—— 两个号各按各的交，不挑一个当准。
    * 那一句标签是**算好写进流的**，所以 `label`（解出来的字）、`label_written`（那一句原样，
      连前面那个空格）、`label_tab`、`label_font` 一起交 —— 这一族的「第几号」有两个出处。
    * 段上还另写了一份 `\li360\fi-360`，那是**段自己的缩进**，与级别定义里的 `li` 不是一回事
      （这份件里两处都是 `360`，但两家可以不同），所以并排放着。
    * 跨家族最狠的一条：同一段点第二级（`\ilvl1`），docx 那边抽象是 `singleLevel` → `level_found: false`，
      而 LibreOffice 的 RTF 导出把九级都写全 → `level_found: true`。
    * 「定义了没人用」在这里是常态：`notes.rtf` / `para.rtf` / `tables.rtf` / `comments.rtf` …
      都带着整个模板的 7 份定义、63 级，而 `listed: 0`；`notes-end.rtf` 只带一份 ——
      与 `notes.odt` 的「十份定义一段没套」同一类事实，各按各的交。

59. **.ods 的宽与高不在列/行元素上，而且每张表都被补到整 16384 列**（六份 .ods 全量过）。
    一条 `table:table-column` 写的只有「我顶几列」（`number-columns-repeated`）与偶尔一句
    `table:visibility`，`style:column-width` 住在它点名的那份自动样式里 —— 与 .ods 的数据样式、
    odt 的段落格式同一类账：值在别处。
    * **每张表都是 16384 列**：`book.ods/预算表` 是「2 条元素 = 2 + 16382」，同件的另两张是
      1 + 16383；`formats.ods/格式` 是 3 + 16381；`hidden.ods` 是 2 + 3 + 16379。
      所以「几条元素」「盖住几列」「有内容的最右一列」
      是三本账：`book.ods/预算表` 是 2 / 16384 / 2，谁也不替谁圆场。
    * 行也一样，而且更明白：`chart.ods/数据` 5 条行元素盖住 **20** 行（其中一条 `repeated="16"`），
      而**有内容的只有 3 行** —— 那一片空白是一整条元素，不是一行一行写的。
    * 那两堆样式**全在 content.xml**：六份件的 `styles.xml` 里 `family=table-column` 与
      `family=table-row` **一条都没有**（只有 graphic 与 table-cell 两族）—— 所以这一族只跳一跳、
      就在同一份件里，与编号那批「定义整个搬到 styles.xml」正相反。
    * **行两根都写、列一根不写**：每条行样式既有 `style:row-height="0.529cm"` 又写
      `style:use-optimal-row-height="true"`，而列样式只有 `style:column-width="1.672cm"`，
      没有 `use-optimal-column-width` —— 于是那两栏的 `optimal` 一个是 5、一个是 0。
    * 隐藏的三列是**元素自己**说的（`hidden.ods` 里那条 `table:visibility="collapse"` + `repeated="3"`），
      它点名的 `co2` 样式里反而什么都没有 —— 所以 `element_visibility` 与 `style_visibility` 分两个键，
      合成一个就把「谁说的」这件事读丢了。
    * 表级那四个自报的数（`number-columns` / `number-rows` / `default-column-width` /
      `default-row-height`）六份件**一个都不写** → 四个 null，而不是四个 0。
    * 换算用与「那张纸」同一条整数式子：`1.672cm` / `0.529cm` / `2.545cm` 落在
      `1672` / `529` / `2545`（0.01mm），两家读者逐位一样。

60. **一张表在演示稿里有三种写法，而「几个格」与「跨度之和」不是一个数**（`deck-tables.pptx`、`-lo.pptx`、`.odp`）。
    * **`a:tblPr` 的「在而没说」**：python-pptx 写 `firstRow="1" bandRow="1"` 并且里面带一条
      `<a:tableStyleId>{5C22544A-…}</a:tableStyleId>`；LibreOffice 重写同一张表写成
      **空的** `<a:tblPr></a:tblPr>` —— 一个属性都没有、也没有样式 id。所以「元素在不在」与
      「说了什么」分两个键（与 docx 那面空的 `<w:tcBorders>` 同一条道理）。
    * **合并是第三种存法**：起点那格写 `gridSpan="2"` / `rowSpan="2"`，而被合掉的那一格
      **照样在场**，只多一个 `hMerge="1"` / `vMerge="1"`，字是空的、一个 `a:r` 也没有。
      docx 是「不写被合掉的那格」、ODF 是「另写一格 `table:covered-table-cell`」，三家三样。
    * 于是**一行的三个数**：第一行 3 个格、跨度之和 **4**、网格只有 3 列 —— 整张表 9 个格、
      跨度之和 10。谁也不替谁圆场，三个都交（`cell_elements` / `span_sum` / `column_elements`）。
    * **重写换了写法没换换算**：没说过话的那两行一家写 `h="609600"`、另一家写 `609480`，
      而换到 0.01mm 两边都是 `1693`；说过话的那一行两家都是 `914400` → `2540`。
      列宽两家一字不差：`2743200` + `1828800` + `1828800` = `6400800` → `17780`。
    * **EMU 那条换算是第三方对过的**：同一张表 LibreOffice 转成 odp 之后列宽写 `7.62cm`、
      `5.08cm`，行高写 `1.693cm`、`2.54cm` —— 换算到 0.01mm 是 `7620 / 5080 / 5080` 与
      `1693 / 2540 / 1693`，与两副 pptx 完全一样。`127/45720` 那个分数不是自己凑的。
    * **格子的边距有两处**：`a:tcPr` 的 `marL/marR/marT/marB` 与 `a:bodyPr` 的
      `lIns/tIns/rIns/bIns`。python-pptx 两处都不写（`<a:tcPr/>` 是空的），LibreOffice 每格补满
      `tcPr`（外加 `lnL/lnR/lnT/lnB` 与一个 `solidFill`），而**只有个别格**把同一份边距又抄进
      `bodyPr` —— 实测：被合掉的那两格抄了（还是 90000 / 45000 这一组，别的格是 91440 / 45720），
      我手工设过 `marR` 的那格也抄了一个 `rIns`。两个键分开放，不替它们合并成「一个边距」。
    * 一格两段的字用换行拼（`网络\n设备`），段数与 run 数各交一个键；`a:tab` / `a:br` 在两副
      读者里都还原成制表与换行（只拼 `a:t` 会把它们读没了）。

61. **odp 的表名不写在表上，而在装它的那个 `draw:frame` 上**（`deck-tables.odp`、`deck.odp`）。
    页上的表与 .ods 里的表是**同一种元素**（`table:table`），所以这一份账直接走 .ods 那条读法；
    差别全在容器与应用上。
    * `table:table` 自己的属性是**一个都不写**（`written: {}`，表名那个位置是空串），
      而 frame 写着 `name="Table 2"`、`style-name`、`layer="layout"`、`x="2.54cm"`、`y="5.08cm"`、
      `width="17.779cm"`、`height="6.097cm"` —— 「这张表叫什么、放在哪、多大」三个问题都只能从这儿答。
      两份 odp 件里那张表都叫 `Table 2`（frame 的序号连标题框与备注框一起数，与图同一件事）。
    * **Impress 不把列补齐**：3 条列元素盖 3 列（.ods 那边每张表都是 16384 列），
      行同理 3 条盖 3 行 —— 同一个生产者换个应用，「几条元素」与「盖住几列」这两本账就都变了。
    * `use-optimal-column-width` 在这里**写了**（每一列都写 `false`）、`use-optimal-row-height` 也写 `false`；
      而 .ods 的列上**根本不写**这个属性、行上写 `true`。同一个属性名在三个地方三种说法。
    * 换算是同一把尺：`17.779cm` 这个 frame 宽与两副 pptx 里 `2743200 + 1828800 + 1828800 = 6400800` EMU
      换成 0.01mm 都是 `17780`；行高 `1.693cm` / `2.54cm` = `1693` / `2540` 与 pptx 的 `609600` / `914400` 对上。
    * 合并的第三种写法：起头那格 `number-columns-spanned="2"` / `number-rows-spanned="2"`，
      被盖住的那格**另写一个** `table:covered-table-cell`（点的样式是 `standard`）——
      于是 `cells` 7、`covered` 2、`merged` 2 是三本账（pptx 那边是「照样在场但打上 hMerge」，docx 是「整个不写」）。
    * 格子的类型这一族**一个字都不写**：`office:value-type` 在 9 个格子元素上全都缺席
      （其中 7 个是有字的），所以 `kind` 交 null。这一条是两份读者撞出来的：
      .ods 那边一份按「有没有字」推 string / empty、另一份一律兜成 "empty"，
      而六份 .ods 里进了账本的格子**恰好全部写了**这个属性，两种猜法一直没撞上；
      odp 一有字又没写类型的格子就把分歧翻了出来（同一件事一边报 string、一边报 empty）。
      现在两边都只交文件写了的：没写就是 null，`.ods` 那一份账的输出一个字没变（全量对过）。
    * 格子点的样式只交**名字**：`ce2` / `ce4` / `ce5` 三份，另有四格什么都没点。
      实测那些样式里写着 `draw:fill-color="#ffff00"`（就是 pptx 那面 `a:tcPr` 里那个填充）与
      `style:textarea-vertical-align="bottom"`（就是那面的 `anchor="b"`）—— 但属性住在
      `style:graphic-properties` 而不是 odt 表格用的 `style:table-cell-properties`，
      这一跳在演示稿这一族还没量准，所以只交名字、不猜值。

62. **PDF 的表单可以把类型只写在祖父上，而 `/Opt` 按规范只有数组一种写法**（`forms-hier.pdf`）。
    这一份**不是任何编辑器导的**：量过的几个生产者（Word 2013、LibreOffice、手搓的 `risk.pdf`）
    都没在父字段上写过 `/FT` 或 `/Ff`，继承那几条分支因此一直停在「数过了，没有」。
    现在由 pikepdf 把形状挂出来，让分支真的走一遍；第三个读者 pypdf 独立数过。
    * 形状：`/Fields` 上四条根、连子字段七条、最深第三层
      （`Person` → `Address` → `City`）。全名 `Person.Address.City` 里的点号**是我们拼的**，
      规范只定义了拼法；每条另交自己写的 `/T`。
    * `/FT /Tx` 与 `/Ff 4` 只写在 `Person` 上：`Address` 往上走一跳拿到、`City` 走两跳，
      于是 `inherited_type` 2、`inherited_flags` 2 —— 这两个数以前在每一件上都是 0。
      `First` 自己写 `/Ff 1` 把父上那个 4 盖掉（继承不是叠加）。
      pypdf 数同一份时这两条的 `/FT`、`/Ff` 显示为 None：它不把继承摊开，这条路只能自己走。
    * `/Opt` 的两种合法写法**各留一份**：成对 `[[1 一] [2 二]]`（导出值与显示值分开）与
      摊平 `[(甲) (乙) (丙)]`（同一串）。这一条是这份件量出来的**缺陷**：两个读者的第一版
      都只认 `/Key (…)` 与 `/Key <…>` 两种值，而 `/Opt` 永远是数组 —— 于是**任何** choice
      字段读出来都是空候选，且不报错。现在 `options` 交摊平的串、`options_shape` 说怎么写的
      （`flat` / `pairs` / `mixed` / `empty`），整个没这个键是 null 而不是空数组。
    * `/V` 是三件事，不是一件：`Ghost` 写了 `/V ()`（值就是空串）、`Flags` 把值写成一个数组
      （多选列表框，`/Ff` 第 22 位 = 524288）→ `value_present` true 而 `value` null，
      几段值不摊平成一个串、判不住就交 null、`Person` 整个没写是第三种。
    * 两条控件（`First`、`City`）**同时挂在页的 `/Annots` 上**（那一页三个注记：两条 Widget
      加一条链接），字段树只从 `/Fields` 走，所以是七条不是九条 —— 这是那条规则第一次有件可走。
    * 文档级那三个开关也第一次有了非 null 的样本：`/NeedAppearances true`、`/SigFlags 1`、
      `/DA (/Helv 0 Tf 0 g )` —— 有 AcroForm 的另一份（`risk.pdf`）三个都没写，
      其余五份连 AcroForm 都没有。
    * 字符串全带 `FE FF`：没有 BOM 的 `/V (李)` 会被两个读者都按 PDFDocEncoding 读成两个
      拉丁字母 —— 写的时候就得按规范写。

## 这些数字从哪来

Rust 测试里每个期望值都来自第二读者对这些文件的独立读取：
`scripts/acceptance/office_reader.py`（OOXML / ODF / MS-CFB / OLE 属性集，只用标准库）、
`lyco_rtf.py`（RTF）、`lyco_legacy.py`（`.doc` piece 表、`.xls` BIFF8、`.ppt` 记录树）、
`lyco_formats.py`（`.xlsx` 的数字格式与日期换算），以及 `office_reader.py` 里的
`ods_facts()`（`.ods` 的重复计数、覆盖格与自动样式可见性）。
`.ods` 的列宽与行高也在 `ods_facts()` 里（`layout` 那一份）：一跳的样式表、`repeated` 的累加、
两条 visibility 来路与那四个表级的数，全部照 Rust 那边一条一条写，两边交回的
`layout` 整份相等（含 `listed` / `shown` 这两个「看见多少」）。
页上那张表是 `office_reader.py` 的 `slide_tables_of()`：`tblPr` 在不在、网格与行高按写的交
再换一次算、每格的 `tcPr` 属性与孩子名、`bodyPr` 那份第二处边距、段数与 run 数；
段里的字用 `ooxml_para_text()` 按文档顺序拼 `.text` 与孩子的 `.tail`（把 `a:tab` / `a:br`
还原成制表与换行），与 Rust 的 `run_text` 走的是同一条路。同一张表的第三副账在
`lyco_grid.py`：`odp_table_sizes()` 从 odp 的列样式里读 cm 再换成 0.01mm，`grids_of()` 现在也认
`.odp`（那一族的合并是另写一格 `covered-table-cell`）。
表单那一份是 `lyco_pdf.py` 的 `form_of()`：`/Fields` → `/Kids` 那条链、沿 `/Parent` 往上取
`/FT` 与 `/Ff`、`/Opt` 的两种数组写法（`options_of()` 与 Rust 那边同一条「一路认串、一路配对」
的扫法）都一条一条照写。`forms-hier.pdf` 还另有**第三个**读者：pypdf 自己那套字段枚举数到
同样七条、同样的两种 `/Opt` 与同样的 `/V` 三件事，但它不把继承摊开（`Address` / `City` 的
`/FT`、`/Ff` 在那边是 None），所以那一条只能我们自己量出来。
每张表的打印设置是同一份文件里的 `xlsx_print_setup()`：三个元素各自取第一个（与 Rust 那边
`descendants(name).first()` 同一条规则），属性名去掉前缀原样交，缺的元素留 null。
规则那一份是同一份文件里的 `sheet_rules()` 与 `dxf_table()`：与 Rust 一样先看 `cfRule` 的
直接子元素、把 `dxfId` 解到 `dxfs` 的第几条上，属性一个也不替它补。
图那一份是 `xlsx_charts()` 与 `rels_of_parts()`：走的就是「表 → 关系表 → 画法部件 → 它的关系表 →
图部件」那三跳，`Type` 结尾与 `Target` 的解法与 Rust 的 `rels_of` / `resolve_target` 一条规则，
两边对同一批件交回的 `chart_list` 整份相等（含 `whole` 与引用串）。
那张纸（纸面尺寸与四边）另有 `scripts/acceptance/lyco_pages.py`：同样只吃标准库，
docx 用 ElementTree 找 `w:sectPr` 的 `w:pgSz` / `w:pgMar`，odt 找 `styles.xml` 里真写了
`fo:page-width` 的那些页布局，两边都按同一条整数式子换成 0.01mm（RTF 的那一串在
`lyco_rtf.py` 的走查里，交给 `lyco_pages.rtf_entry` 换算）。
修订那一份另有 `scripts/acceptance/lyco_revisions.py`：ElementTree 的 `.tail` 天然带着
「插入的字夹在两个标记之间」那个顺序，而 Rust 那边靠 xmlscan 的 `#text` 子节点走同一条路 ——
同一份 `revisions-lo.docx` 与 `revisions.odt` 两边逐条对得上，才对得起「合成规则」这四个字。
保护那一份另有 `scripts/acceptance/lyco_protect.py`：同样只吃标准库，
按 ElementTree 的属性取法把四家的开关与两层结构各读一遍，与 `protect.rs` 逐字段对。
编号那一份也在同一份 `office_reader.py` 里（`docx_numbering` / `odf_numbering`）：
跳数、去重（同名取第一条）、`limit` 封顶与三个布尔的判据都照 Rust 那边一条一条写，
ODF 那一路的 `text:list-style-name` 与 `text:list` 上的 `text:style-name` 是分开的两处，
而前缀还原要按**各自那份件**自己声明的 `xmlns:` 来（样式定义在 styles.xml，段样式在 content.xml），
所以两张前缀表并起来用、content 优先。
段落格式与分栏那一份在同一份 `office_reader.py` 里（`docx_paragraph_formats` / `docx_columns` /
`odf_paragraph_formats` / `odf_columns`）：ODF 那边要按前缀交属性，而 ElementTree 会把
`fo:margin-left` 折成 `{uri}margin-left` —— 于是先从这份 `content.xml` 自己的 `xmlns:fo=` 那几条声明里
建一张 uri→前缀 的表再还原名字（前缀是文件自己起的，不是抄来的）。两边都不把
`xmlns` / `xmlns:*` 当属性，样式的解析都只一跳、同名取第一个（与 Rust 的 `find` 同一条规则）。
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
