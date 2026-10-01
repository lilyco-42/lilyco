# `lbin office-*` 的 fixture：每一份都有署名的生产者

这些文件**不是手搓的字节**。断言如果来自我自己写的字节，那就只证明了「我写的东西能被我自己读回来」，
不证明能读真实世界里的办公文件。所以这里的每一份都由一个独立程序写出，
重跑 `python scripts/office_fixtures.py --force`（需要 LibreOffice；python-docx / python-pptx /
openpyxl 装在 `D:/app/scoop/apps/python/current/python.exe` 那套解释器里，PATH 上的 uv python 没有）。

**用哪一套解释器要写清楚**：那套装了 lxml，openpyxl 就用 lxml 的序列化器（非 ASCII 写成
`&#31532;` 这种数字引用、行尾是 LF）；换一套没装 lxml 的（例如 anaconda 那套），openpyxl 退回
标准库 ElementTree 写，同一份值就变成原样 UTF-8 加上串里的 `CRLF` —— 版本号一样、字节不一样。
`pipes.xlsx` 与它的两份重写是后一种解释器产出的（这一批只有这三份），那三个 CRLF 就是
事实 121 的凭据；其余件都出自前一种。重做谁之前先看这一条。

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
| `deck-links.pptx` | python-pptx（`write_pptx_links`） | 页上三条链接（站外 http 且字与地址不同、`mailto:`、字就是地址）+ 一页一条也不链；链接不住在字里，只在 run 的 `a:rPr/a:hlinkClick/@r:id` 留一个号 |
| `deck-links-lo.pptx` | LibreOffice（从 `deck-links.odp` 回转） | 同样三条链接、同一个地址，但号被重排成 `rId1/2/3`（一家从 rId2 起，一家从 rId1 起）—— 号是生产者自己排的，只交不比 |
| `deck-links.odp` | LibreOffice（从 `deck-links.pptx` 导出） | 第三种写法：地址直接挂在字上（`text:a/@xlink:href` + `xlink:type="simple"`），没有第二跳也没有「站内/站外」那个开关；而那个文本框在这里成了 `draw:custom-shape`，不再是 `draw:frame` |
| `deck-hidden.pptx` | python-pptx（`write_pptx_hidden`） | 第二页根元素上 `show="0"`（= PowerPoint 那句「隐藏幻灯片」；python-pptx 没这个开关，包是它写的，这里只设 UI 会设的那一个属性），第一页什么都不写 |
| `deck-hidden-lo.pptx` | LibreOffice（从 `deck-hidden.odp` 回转） | 同一句话过了一遍 ODF 再回来，`show="0"` 一字不动 —— 隐藏不是会被重写吃掉的那种信息 |
| `deck-hidden.odp` | LibreOffice（从 `deck-hidden.pptx` 导出） | ODF 的第三种摆法：页上只有 `draw:style-name="dp1"` / `"dp3"`，那句 `presentation:visibility="hidden"` 在 dp3 那份 drawing-page 样式里；**同一份文件里 dp2 也写着 hidden 而没有任何页点它的名** —— 只 grep 全文就数错 |
| `deck-tables.odp` | LibreOffice（上面那一转的中间件） | 同一张表的第三种写法：列宽换成 `7.62cm` 与 `5.08cm`、行高换成 `1.693cm` 与 `2.54cm`，而合并改成**另写一格** `table:covered-table-cell`（既不是 docx 的不写、也不是 pptx 的 `hMerge`） |
| `fonts.docx` | python-docx | 字体那份账要的四种点法各一段：点表里有的（Courier，`w:rFonts` 一次写 `ascii` 与 `hAnsi` 两遍）、点表里**没有**的（Courier New）、只点主题那一路（`asciiTheme`/`hAnsiTheme` 而没有 `ascii`）、只点东亚那一路（`eastAsia="ＭＳ 明朝"`），最后一段一个字都不点。`word/fontTable.xml` 是模板那八条，**一个字体名都没嵌进包** |
| `fonts-lo.docx` | LibreOffice（`fonts.docx` → .odt → .docx） | 重写那份的三件事：主题那一跳被**就地解开**（同一条既写 `ascii="Cambria"` 又留 `asciiTheme="minorHAnsi"`）、字体表补上 Courier New 而把两个日文字体从表里去掉（于是「点了没声明」从 1 个名变成 4 个）、styles.xml 里 26 条 `cs=""`（写了空话），另有一个正文里的东亚点法整个不见了 |
| `fonts.odt` | LibreOffice（`fonts.docx` → .odt） | 第三种存法：表是 11 条 `style:font-face` 而**两份件各写一份一模一样的**，名字与族名是两个键（`Cambria` 与 `Cambria1` 同族只靠 `style:font-charset="x-symbol"` 分开，`name="F"` 那条族名是空串，带空格的名字写作 `'Liberation Sans'` 而 `Calibri` 不带引号）；点它的地方也分两种指针 —— 见事实 83 |
| `deck-autofit.pptx` | python-pptx | 四个框，`a:bodyPr` 各写一种：`a:noAutofit`、`a:spAutoFit`、`a:normAutofit fontScale="75000" lnSpcReduction="20000"`，第二页那一个没人设过而 python-pptx 自己补了 `a:spAutoFit`；第一页三框 `wrap="square"`，第二页 `wrap="none"`。**内边距那四个属性一个都没写** —— `written` 里就只有 `wrap` 一个键 |
| `deck-autofit.odp` | LibreOffice（`deck-autofit.pptx` → .odp） | 同一个问题的第三种写法：答案在框点名的 family=graphic 样式（`gr1`…`gr5`）的 `style:graphic-properties` 上，而 pptx 的 `noAutofit` 与 `spAutoFit` 两档在这里**写成一模一样的一条**（`style:shrink-to-fit="false"` 加 `draw:fit-to-size="false"`）—— 见事实 82 |
| `print-area.xlsx` | openpyxl | 四张表，一次只改一个变量：`区域与标题` 只给打印区域、`区域加标题` 给区域 + 重复第 1 行、`两段区域` 把区域给成**两段**（一条 definedName 里逗号分隔）、`什么都没给` 只给重复**列**。五条件都写成 `_xlnm.Print_Area` / `_xlnm.Print_Titles` 两条保留名，sheet 名一律带引号 |
| `print-area-lo.xlsx` | LibreOffice（`print-area.xlsx` → .xlsx） | 同一份的五条**一字不差地少了一层**：引号全没了（5 条带引号 → 0 条），条目顺序也换了（LO 按自己的分组重排），而 `localSheetId` 与范围串本身不变 —— 见事实 86 |
| `print-area.ods` | LibreOffice（`print-area.xlsx` → .ods） | 同一问的第三种存法：表自己身上 `table:print-ranges`（分隔符换成空白、地址是 `表名.A1:表名.C10`），另有一份为与 Excel 来回留的 `table:named-*` 五条 —— 四样 `named-range` 而两段那一样是 `named-expression`，五样的 `base-cell-address` 全是同一个 |
| `workbook-settings.xlsx` | openpyxl 打底 + `zipfile` 按 MS-XLSX 的序列手写（`office_fixtures.py` 的 `write_workbook_settings_xlsx`） | 这一本工作簿自己的设置：`workbookPr` 四格（`codeName` / `backupFile="1"` / `showObjects` / `defaultThemeVersion`）、**两枚** `fileVersion`（`xl15` 与 `GenuineMicrosoftOffice`，各带 `lastEdited` / `lowestEdited` / `rupBuild`）、`calcPr` 七格（`calcMode="manual"` + `iterate="true"` + `iterateCount` / `iterateDelta` + `refMode="row"` + `fullCalcOnLoad="1"`）、`workbookView` 十三格而 `activeTab="2"`（打开停在三张表里的第三张）且 `autoFilterDateGrouping="false"`、`customWorkbookViews` 一枚带 `name` 与 `guid` 的共享视图 —— **同一份件里两种布尔拼法同时存在** |
| `workbook-settings-lo.xlsx` | LibreOffice（`workbook-settings.xlsx` → .xlsx） | 同一层每一处都换了写法：两枚 `fileVersion` **合成一枚**而 `appName` 变成 `"Calc"`、`lastEdited` 与 `rupBuild` 都不写而 `lowestEdited="5"` 从第二枚留下；`calcPr` 的 `calcId` / `calcMode` / `fullCalcOnLoad` 三格不见，而 **`refMode` 从 `"row"` 变成 `"A1"`**（同一格两个意思）；`workbookPr` 那四格换成它自己补的三格（`codeName` 与 `defaultThemeVersion` 没了、`date1904="false"` 多出）；`workbookView` 的窗口四格值被改（120/90/18000/9000 → 0/0/16384/8192）而 `visibility` / `minimized` / `autoFilterDateGrouping` 整枚不见，只有 `activeTab="2"` 与 `firstSheet` / `tabRatio` 原样穿过；`customWorkbookViews` **整层不留** |
| `workbook-settings.ods` | LibreOffice（`workbook-settings.xlsx` → .ods） | ODF 根本没有 `workbookPr` / `fileVersion` / `calcPr` 这三枚元素：同一问摊在 `settings.xml` 的 `ooo:configuration-settings` 里（这一份写 `AutoCalculate`、`SyntaxStringRef`、`ShowFormulasMarks`、`ImagePreferredDPI` 等），而**迭代计算那三格一份都不写**（`IterateSteps` / `IterateMinChange` 都不出现）—— 转格式丢掉的事，所以 `workbook_settings` 这个键在 .ods 的整份输出里不存在 |
| `cjk-switches.docx` | python-docx 打底 + 按 ECMA 手写九枚 | 九枚段开关一次摆开：段上 16 段写着、`autoSpaceDE` 六枚里一枚**写了空串**、两枚裸写，`docDefaults` 那处只写 `adjustRightInd` 一枚（真件一处都不写），字侧三 run 交两枚 `noProof`（一枚裸、一枚 `0`），两段是段与样式**都写而不一致**（`snapToGrid` 段 1 / 样式 0，`autoSpaceDE` 段 `0` / 样式裸），还有一段只点样式、字面一个不写 —— 那一跳要走到 |
| `cjk-switches-lo.docx` | LibreOffice（`cjk-switches.docx` → .docx） | 这一层重写后只剩四枚：`kinsoku`、`wordWrap`（含那枚 `off`）、`autoSpaceDE`、`autoSpaceDN`、`adjustRightInd` 与字侧 `noProof` **整个不再写**，`docDefaults` 那枚也不留；`overflowPunct` 两枚都改写成 `false`，`snapToGrid` 的裸写与 `0` 与 `1` 换成 `true` / `false` / `true`，`contextualSpacing` 显式关掉的那枚不见了，只有 `textAlignment` 的 `baseline` / `auto` / `center` 一字不动 |
| `cjk-switches.odt` | LibreOffice（`cjk-switches.docx` → .odt） | 九枚换成另一套词：`style:contextual-spacing` 逐段补满（15 段，false 11 / true 4）、`style:snap-to-layout-grid` 五段（false 3 / true 2）、`style:punctuation-wrap` 两枚 `simple`；`line-break` 按段解出来是**零**（那两枚 strict 写在没人点用的样式上）；`wordWrap` 那枚 `off` 换成 `fo:wrap-option` 的 `no-wrap`，而那一格这一族不读 |
| `cjk-odf.odt` | 手写（python zipfile，按 ODF 1.2） | ODF 那一头的手写形状：四枚近亲全在段落样式上（`contextual-spacing` true 与 false 各一段、`line-break` 一枚 `strict`、`punctuation-wrap` hanging 与 simple 各一段、`snap-to-layout-grid` 一枚 `false` —— 真件 64 份 .odt **一条都没写过**这一枚），另两段是「样式在而一条属性都不写」与「连样式名都不点」 |
| `cjk-odf-lo.docx` | LibreOffice（`cjk-odf.odt` → .docx） | 反方向只搬得动四件事：`kinsoku` 给 true、`overflowPunct` 给 true 与 false 各一枚（来自 `punctuation-wrap`）、`snapToGrid` 给 false、`contextualSpacing` 裸写；`autoSpaceDE`、`autoSpaceDN`、`adjustRightInd`、`wordWrap`、`textAlignment`、`noProof` **全是零** —— 两族之间没有一一对应可走，所以两边各按各的原样交 |
| `deck-ph.pptx` | python-pptx | 四页，一次只改一个变量：`第1页` 标题 + 内容占位符（内容两段字）、`第2页` 再加一个**自制文本框**、`第3页` 两个占位符都在而**字是空的**、`第4页` 只有文本框（空版式）。要点：正文占位符写的是 `<p:ph idx="1"/>` —— **没有 `type`** |
| `deck-ph-lo.pptx` | LibreOffice（`deck-ph.pptx` → .pptx） | 同一份稿子重写后：标题那一句照旧 `<p:ph type="title"/>`，正文那一句变成**空元素 `<p:ph/>`**（连 idx 都没了），形状名从 `Title 1`/`Content Placeholder 2` 换成 `PlaceHolder 1`/`PlaceHolder 2`；版式里 `dt`/`ftr`/`sldNum` 的 idx 也整个重排（模板是 10/11/12，这里 1/2/3、4/5/6…28/29/30） |
| `deck-ph.odp` | LibreOffice（`deck-ph.pptx` → .odp） | 第三家：角色写成 `presentation:class="title"`，占位符另带 `presentation:placeholder="true"` 与 `presentation:style-name="prN"`，而**文本框是 `draw:custom-shape` 且没有 `presentation:style-name`**；页的版式名不在页上，在画页样式里（`presentation:presentation-page-layout-name="AL1T11"`） |
| `table-header.docx` | python-docx（`write_table_header_docx`） | 四张表，一次只改一个变量：`只重复第一行` / `重复前两行` / `一个都不重复`（对照）/ `只重复中间一行`。这一族的答案是**行上**一枚没有值的 `w:trPr/w:tblHeader`（在场就是重复）：python-docx 这一版没有暴露这个开关（`repeat_table_header` 是个静默无效的属性），所以走它的 oxml 层写元素 |
| `table-header-lo.docx` | LibreOffice（`table-header.docx` → .docx） | 同一份稿子重写后：标在**第二行**的那枚整个丢了（4 行标了 → 3 行、`non_leading` 1 → 0），而它给每一行都补了一枚**空的** `w:trPr`（四张表的枚数 1/2/0/1 → 3/3/3/3）—— 见事实 89 |
| `table-header.odt` | LibreOffice（`table-header.docx` → .odt） | 同一问的第三种存法：`table:header-rows` 与 `table:header-rows-repeated` 坐在表身上（是两个数，不是行上的元素），而这一转**一个都没写**，四张表全 null —— 见事实 89 |
| `tabs.docx` | python-docx（`write_tabs_docx`） | 四条段，一次只改一个变量：`左对齐无引导` / `右对齐点引导` / `居中长划引导` / `小数点对齐`，每段另加一条 9cm 左对齐下划线引导的，并按两次 Tab 键 —— 「定义了哪几个位置」与「按了几下制表键」是两本账 |
| `tabs.odt` | LibreOffice（`tabs.docx` → .odt） | 同一问的第二种存法：制表位不在段上，一跳在段点的那份自动样式（`P1`…`P4`）里，位置变成带单位的串，而 **9cm 写成 `8.999cm`**（转一趟少 0.001cm）—— 见事实 90 |
| `tabs.rtf` | LibreOffice（`tabs.docx` → .rtf） | 第三种：位置回到 twip（`\tx1701` 与 `\tx5102` 各 4 条），而对齐与引导符是**只管下一个位置**的前缀（`\tldot\tqr\tx1701`）—— 规则量准了，这一支读者也读了它（第三种形状，见事实 91）；样式表那一群里另有 4 条位置，按这一族的规矩跳过不数 |
| `doc-comments.docx` | python-docx 1.2（`write_comment_thread_docx`） | 四条段、三条批注：前两条锚在**同一段**上（号 0 与号 1），第三条另起一段，第四段没人锚。内容住在 `word/comments.xml`（`w:id` / `w:author` / `w:initials` / `w:date` 带 Z），锚点在正文里（`commentRangeStart` / `End` / `commentReference` 三处，只带号） |
| `doc-comments-lo.docx` | LibreOffice（`doc-comments.docx` → .docx） | 同一份重写后：条数、作者、六个锚点数一字不差，而 `comments.xml` 里那三条排成 **1,0,2**（原来 0,1,2），正文那九个锚点一字没动 —— 「第几条」按部件与按正文是两个答案，见事实 92 |
| `doc-comments.odt` | LibreOffice（`doc-comments.docx` → .odt） | 同一问合一处：`text:annotation` 坐在所属那一段里，作者是孩子元素 `dc:creator`、时间 `dc:date`（**没有 Z**）；全文 6 个 `text:p` 里 3 个住在批注里 —— 见事实 92 |
| `keep.docx` | python-docx（`write_keep_docx`） | 五条段，一次只改一个变量：基线（四个开关都不写）/ `keepNext` / `keepLines` / `pageBreakBefore` / `widowControl=False`。前三家写出来是**空元素**（没有值），第四个写出来是 `w:val="0"` —— 三种状态（没元素 / 有元素没值 / 有元素有值）分开交 |
| `keep-lo.docx` | LibreOffice（`keep.docx` → .docx） | 同一份重写后：每段都被补了 `w:pPr`（4 → 5 枚），值改成 `w:val="true"` / `w:val="false"` 这种拼法，而**`w:pageBreakBefore` 整个没了**（带着它的段从 4 段掉到 3 段）—— 见事实 93 |
| `keep.odt` | LibreOffice（`keep.docx` → .odt） | 同一问在 ODF 全在一跳之外：段只点样式名，`fo:keep-with-next` / `fo:keep-together` / `fo:break-before` 各在一份样式上，而**孤行控制是两个数**（关掉写成 `fo:widows="0"` + `fo:orphans="0"`，而 `Standard` 自己写着 `2`/`2`）—— 见事实 93 |
| `table-style.docx` | python-docx（`write_table_style_docx`） | 四张表，一次只改一个变量：`默认表`（不点样式）/ `内置样式`（`Light Grid Accent 1` → 写成样式 id `LightGrid-Accent1`）/ `改了 tblLook`（把 `w:firstRow` 改成 0）/ `没了 tblStyle`（那一格整个删掉，`w:tblLook` 留着）。要点：改了位之后那个十六进制缓存值 python-docx **不重算**（还是 `04A0`） |
| `table-style-lo.docx` | LibreOffice（`table-style.docx` → .docx） | 同一份重写后：样式与那六个位一个没变，而**缓存被重算了**（第三张 `04A0` → `0480`），其它几张那个值也从大写换成小写（`04A0` → `04a0`）—— 见事实 94 |
| `table-style.odt` | LibreOffice（`table-style.docx` → .odt） | 同一问在这一族只剩一个名字：四张表各点一份自动样式（`表格1`…`表格4`，family=table，都没有父样式），`LightGrid-Accent1` 与那枚 look 都看不见 —— 见事实 94 |
| `line.docx` | python-docx（`write_line_docx`） | 五条段，一次只改一个变量：`段零`（行距什么都不写）/ `1.5 倍` / `2 倍` / `固定 22 磅` / `至少 18 磅`。要点：1.5 倍与「至少 18 磅」在文件里是**同一个数** `w:line="360"`，只有紧跟的 `w:lineRule`（`auto` 对 `atLeast`）说得清那是什么单位 |
| `line-lo.docx` | LibreOffice（`line.docx` → .docx） | 同一份重写后：四个数与其单位一个都没改口，而段零被补了一份 `w:pPr`（里面**没有** `w:spacing`）—— 见事实 95 |
| `line.odt` | LibreOffice（`line.docx` → .odt） | 一跳在段点的样式里、单位写在串上（`150%` / `200%` / `0.776cm`），而 `atLeast` 那一段四个相关属性一个都没写、docx 里什么都没写的段零点的 `Standard` 样式却写着 `115%` —— 见事实 95 |
| `pborder.docx` | python-docx（`write_border_docx`，段边框没有公开属性，走 `OxmlElement`） | 五条段，一次只改一个变量：`段零`（两样都不写）/ `四边单线`（一枚 `w:pBdr` 里四条边，各带 `val`/`sz=6`/`space=1`/`color=FF0000`）/ `只有一条上边`（`double` `sz=18` `color=auto`）/ `只有底纹`（`w:shd` = `clear` + `fill=FFFF00`）/ `空壳加主题色底纹`（`w:pBdr` 在而里面一条边都没有，底纹 `solid` + `fill=00B050` + `themeFill=accent6`）|
| `pborder-lo.docx` | LibreOffice（`pborder.docx` → .docx） | 同一份重写后：每段都补了 `w:pPr`（4 → 5 枚），而那个**空壳整个被丢掉**（3 枚 → 2 枚），`w:color="auto"` 被折成一个具体色 `000000` —— 见事实 96 |
| `pborder.odt` | LibreOffice（`pborder.docx` → .odt） | 两样都在段点的那份样式上而形状换了：四边合成一条 `fo:border="0.74pt solid #ff0000"`，单边那一段四条各写、其中三条明写着 `none`（另多一份逐根的 `style:border-line-width-top`），`w:space` 搬成 `fo:padding`，而 `solid` 那段的底纹变成 `#ffffff` —— 见事实 96 |
| `tbox.odt` | zipfile 写的最小 ODF（`write_tbox_odt`） | 页上有一个文本框：`draw:frame`（`draw:name="框一"`、`text:anchor-type="as-char"`、`svg:width="5cm"`、`svg:x="1.2cm"`、`draw:z-index="0"`）里套一个 `draw:text-box`，框里两段字，框外面正文三段。这一族本机没有会写 OOXML 文本框的生产者，所以反过来走：这份 odt 是源头 |
| `tbox.docx` | LibreOffice（`tbox.odt` → .docx） | **同一个框写两份**：`w:drawing`（尺寸在 `wp:extent`，`cx="1800225"` EMU）与 `w:pict` 各带一份 `w:txbxContent`，两份的字一模一样；正文 3 段而整棵树 7 段 —— 见事实 97 |
| `tbox-lo.odt` | LibreOffice（`tbox.odt` → .odt 重写） | 重写那一遍：挂上 `draw:style-name="Frame"`、`svg:x` / `svg:y` / `draw:z-index` **整个没了**、`5cm` 换成 `5.001cm`，而那一段话一字未改 —— 见事实 97 |
| `bkmks.docx` | python-docx（`write_bookmark_docx`，书签没有公开 API，走 `OxmlElement`） | 八段各造一种情形：完整一对（`口径`）/ 跨段一对（`跨段`，起在第 2 段、止在第 3 段）/ 只有起（`断了`）/ 只有止（号 `9`）/ Word 的光标（`_GoBack`）/ **与第一段重名**的第二条 `口径` / 站内跳转 `w:anchor="跨段"`。要紧的是 `w:bookmarkEnd` 只写号不写名字 |
| `bkmks-lo.docx` | LibreOffice（`bkmks.docx` → .docx） | 同一份重写后：两个**断的整个被删**（5 起 5 止 → 4 起 4 止）、号整批重排成 0..3、重名那条改名 `口径_副本_1`，而锚一字未改 —— 见事实 98 |
| `bkmks.odt` | LibreOffice（`bkmks.docx` → .odt） | 记号换成三种：闭在同段的与 Word 那条光标都变成**一枚** `text:bookmark`（3 枚），只有跨段那一对是 `bookmark-start`/`-end`（两头写名字）；改名的那条在这里写作带空格的「口径 副本 1」 —— 见事实 98 |
| `lang.docx` | python-docx（`add_run_languages`，语言没有公开属性，走 `OxmlElement`） | `w:lang` 一层一个样：段上 `pPr/rPr` 写 `val="es-ES"`，三串字分别**只写** `val="fr-FR"`、**只写** `eastAsia="ja-JP"`、三路全写 `val="de-DE" eastAsia="zh-CN" bidi="ar-SA"` —— 一枚元素的三个属性各说一路文字，不并成「这文档几种语言」|
| `lang-lo.docx` | LibreOffice（`lang.docx` → .docx） | 正文那四条一字未动（段 1 + run 3），而它**给 `Normal` / `NoSpacing` / `MacroText` 各补了一条** `en-US / en-US / ar-SA` —— 元素 5 条变 8 条、`levels_seen` 多出一层：补的是生产者的手笔，不是稿子说过的话 |
| `lang.odt` | LibreOffice（`lang.docx` → .odt） | 换族之后只剩一格：`distinct_languages` 是 `de / en / es / fr` —— 只写 `eastAsia="ja-JP"` 那一串字**一个字都没落**（没有 ja），三路全写那串只剩 `de` + `DE`（zh 与 ar 都不见），而 `en-US` 这一族拆成 `language="en"` + `country="US"` 两个属性 |
| `nset.docx` | python-docx（`add_note_numbering`，拿 `notes-end.docx` 补设置） | 注的编号在 OOXML 写在**两处**：settings.xml 那份 `w:footnotePr` 说 `numFmt=decimal` / `numStart=5` / `numRestart=eachPage` / `pos=sectEnd`，`w:sectPr` 里那一份**只有 `pos` 与 `numFmt`**；settings 那份还带两个分隔符引用（`w:footnote w:id="0"/"1"`），节里那份没有 |
| `nset-lo.docx` | LibreOffice（`nset.docx` → .docx） | 同一份重写一遍：两处那两格**都没了**（只剩 `pos` 与 `numFmt`），`attrs_only_in_settings` 变空；编号格式与分隔符引用一字未动 |
| `nset.odt` | LibreOffice（`nset.docx` → .odt） | 一类注一份 `text:notes-configuration`（两份都在 styles.xml）：footnote 那份 `num-format="1"` + `start-value="0"` + `footnotes-position="page"` + `start-numbering-at="document"`，endnote 那份**只有前两个**；而源件明写的「从 5 开始」在这里是 `start-value="0"` —— LO 写自己的默认 |
| `pnum.odt` | zipfile 写的最小 ODF（`write_pnum_odt`） | 页码起始在 ODF 写在两处：段落属性上 `style:page-number="7"` + `style:use-page-numbering="true"`（外加 `fo:break-before="page"`），页版式上 `style:num-format="1"` + `style:page-number="1"`；两条母版页共用那一份版式，所以「几条版式」是 1 而「几份母版页」是 2 |
| `pnum.docx` | LibreOffice（`pnum.odt` → .docx） | 转过来之后 `w:pgNumType` **只带 `fmt="decimal"`**：「从 7 开始」整格没写（`start_written` 是 null，不是 0 也不是 7），而源件那个「另起一页」也没换出第二节（`sections_total` 1）|
| `restart.docx` | python-docx（`add_page_number_start`，页码没有公开属性，走 `OxmlElement`） | `w:pgNumType` 三个属性全写：`start="7"` / `fmt="upperRoman"` / `chpNum="none"` —— 这一节既说了用什么数、也说了从几起、也说了不跟章号 |
| `restart-lo.docx` | LibreOffice（`restart.docx` → .docx） | 同一份重写一遍：`start` 与 `fmt` 都活着、**`chpNum` 整格没了** —— 三个属性不是同一个待遇，账按现在这份件交 |
| `restart.odt` | LibreOffice（`restart.docx` → .odt） | 同一问换了地方也换了词汇：页版式上只剩 `style:num-format="I"`（大写罗马这一族写成一个字母），**「从 7 开始」在 ODF 侧一个字都没落**（`with_page_number` 0）；它另外写的 `style:default-page-layout` 只带网格，不进这本账 |
| `deck-tr.pptx` | python-pptx 1.0.2（`write_transition_deck`，切换没有公开属性，走 `parse_xml`） | 三页各改一个变量：`第一页`（`p:transition spd="med" advClick="1" advTm="5000"` + 孩子 `p:fade`）/ `第二页`（只写 `spd="fast"`，方向在孩子 `p:wipe/@dir="l"` 上）/ `第三页`（切换一个字都没写）。`spd`、`advClick`、`advTm` 是三句独立的话（多快、点一下换不换、几毫秒换页） |
| `deck-tr-lo.pptx` | LibreOffice（`deck-tr.pptx` → .pptx） | 同一份重写后：第一页的 `advClick` 没了、第二页连 `spd` 也没了（孩子的 `dir="l"` 留着），而第三页**原本什么都没写、它补了两条**（`{spd:slow,dur:2000}` 与 `{spd:slow}`，都没有效果孩子）—— 全篇 4 条而只有 3 页有 —— 见事实 99 |
| `deck-tr.odp` | LibreOffice（`deck-tr.pptx` → .odp） | 切换没丢而是**搬了两处并换词表**：`style:drawing-page-properties` 上写 `presentation:transition-type="automatic"` / `transition-speed="fast"` / `duration="PT5S"`（dp1 有、dp2 没有），效果本身进 `anim:transitionFilter`（`smil:type="fade"` + `subtype="crossfade"`、第二页 `barWipe` + `leftToRight`），页上已无 `p:transition` —— 见事实 99 与 100 |
| `deck-chart.pptx` | python-pptx 1.0.2（`write_pptx_charts`） | 演示稿上的图：同一页挂柱形（两条系列）与饼图（一条），第二页一张也没有；引用指向**图自己那张内嵌工作簿**（`ppt/embeddings/Microsoft_Excel_Sheet1.xlsx` 里的 `Sheet1!$B$1`），值全缓存了，轴 id 写成**负数** |
| `deck-chart-lo.pptx` | LibreOffice（`deck-chart.pptx` → .odp → .pptx） | 同一批图的第二种写法：`ppt/charts/` 里多出 style 与 colors 四个部件（按目录数会数成六张图，实际两张），`c:f` 里不再写引用而写 `label 0` / `categories` / `0` 这种字面量，**而缓存的数一字未变**；饼图那一侧另补了一个标题「占比」 |
| `deck-gr.pptx` | python-pptx 1.0.2（`write_group_deck`，组合走 `add_group_shape`） | 一页摆层级：第一页一个散框 `sp`（「散着的框」）+ 一个 `grpSp`（「三个框的组合」）套三个 `sp`（「组合里的第 1/2/3 个」），第二页**一个形状都没有**。组合自己那枚 `a:xfrm` 四份全写：`off 0,0` 而 `ext` 与 `chExt` 一模一样（页坐标一套、孩子自己的坐标一套）|
| `deck-gr-lo.pptx` | LibreOffice（`deck-gr.pptx` → .pptx） | 同一份重写：形状、组合、两套坐标与五个名字全保住，`id` 从 2..6 整批重排成 61..65，坐标走那条老换算（`100000`→`100080`、`2900000`→`2899800`），而它顺手给 `spTree` 自己的那份 `grpSpPr` 补了一个**全 0 的 `a:xfrm`** —— 见事实 104 |
| `deck-gr.odp` | LibreOffice（`deck-gr.pptx` → .odp） | 同一页还是 5 条、层级与五个名字全对得上，但分组在这里叫 `svg:g`（`draw:group` 一次都没出现）、`id` 这一族根本没有、尺寸换成 `5.555cm` / `0.278cm` 这种自带单位的串，而组合那一层**一个尺寸属性都不写**；字也不在 `draw:text-box` 里 —— `text:p` 直接挂在 `draw:custom-shape` 身上 —— 见事实 104 |
| `notes.odt` / `book.ods` / `deck.odp` | LibreOffice（从上面三个 OOXML 文件转来） | 真 ODF 写入者产出的三种 ODF |
| `crep.docx` | 从 `comments.docx` 用 zipfile 补出来（`add_comment_thread_parts`）—— 本机没有会写这两份部件的生产者 | 「哪条已解决、谁回复谁」的正例：两条批注体内各挂一个 `w14:paraId`，`word/commentsExtended.xml` 三条 `w15:commentEx`（一条 `done="1"`、一条 `done="0"` 且 `paraIdParent` 指回前者、还有一条号**对不上任何批注**），`word/commentsIds.xml` 三条里也留一条孤儿 —— 见事实 105 |
| `crep-lo.docx` | LibreOffice（`crep.docx` → .docx） | 两份部件**整个不见**，连批注体内那个 `w14:paraId` 也没了（`paras_with_para_id` 2 → 0）—— 回复与已解决两头都读不出来，这是这份件的事实 |
| `crep.odt` | LibreOffice（`crep.docx` → .odt） | 「已解决」在 ODF 换了地方也换了词（`office:annotation/@loext:resolved`），但两条都写 `false` —— 源件里那条 `done="1"` **没落过来**；回复这一问在这一族没有任何对应物 |
| `crep-r.odt` | 把 `crep.odt` 第一格 `loext:resolved` 改成 `true`（zipfile，其余字节不动） | 一份件里 `true` 与 `false` 并存（`resolved_true` 1、`resolved_false` 1）—— 「这份文档解决了几条注」在 ODF 数得出来 |
| `crep-r.docx` | LibreOffice（`crep-r.odt` → .docx） | **这一份的 `commentsExtended.xml` 是生产者自己写的**：2 条批注只给已解决那条写记录（`ext_total` 1、`done="1"`），另一条**没有记录**（不是写 `0`）；段号是它新排的 `01000000`，而 `commentsIds.xml` 整个不写 —— 见事实 105 |
| `shared.xlsx` | openpyxl 先写 16 条公式，再用 zipfile 把 B 列改写成一份共享组（`make_shared_formula_group`）—— 本机没有会写共享组的生产者 | 一列八格**一个**组：主格 `<f t="shared" ref="B1:B8" si="0">A1*2</f>`，跟随的七格只写 `<f t="shared" si="0"/>` —— **文件里没有公式正文**，另有 C 列八条普通公式做对照；16 枚 `<f>` 全都带一枚空的 `<v>`（openpyxl 没算过）—— 见事实 106 |
| `shared-lo.xlsx` | LibreOffice（`shared.xlsx` → .xlsx） | **不用共享组**：16 枚 `<f>` 各写自己的正文（`shared_elems` 0、空正文 0），而它给每一枚都写了 `aca="false"`（openpyxl 那份一个属性都不写）—— 读进去再导出，共享这一层被它摊平 |
| `shared.ods` | LibreOffice（`shared.xlsx` → .ods） | 第三种写法：公式是格子身上的 `table:formula` 属性，16 条全带正文，且**逐行平移**（`of:=[.A2]*2`、`of:=[.A3]*2`…）—— 这正是第三方对「跟随格其实是 A2*2」的独立印证 |
| `sstart.docx` | python-docx（`write_section_starts_docx`，节的起始类型没有公开属性，走 `OxmlElement`） | 三节各写一个变量：第一节「另起一页」**一个字都不写**（那是 Word 的默认，所以 `w:type` 整个不在）、第二节 `continuous`、第三节 `evenPage` —— 3 节里只有 2 节写了元素（`with_element` 2、`type_missing` 1）|
| `sstart-lo.docx` | LibreOffice（`sstart.docx` → .docx） | 同一份重写后 3 节**全写了**：第一节被补出一枚 `<w:type w:val="nextPage"/>`（`with_element` 2 → 3），另两节的值一字未变 —— 「没说」与「说了默认」在两副件里是两个答案，见事实 108 |
| `md.docx` | python-docx（`write_markdown_docx`） | 每段只管一件事的渲染凭据：一级与二级标题 / 同一句里的粗、斜、粗斜 / **长得像 markdown 记号的那些字**（`*` `_` `[]` `<>` `\` `|` 反引号）/ 句中两个连续空格 / 两句行首像记号的正文 / 圆点两条加缩进一条 / 编号两条 / 一张 2×3 表（一格里两段字、一格里有竖线与星号）/ 站外链接 / 段内硬换行 / 一张图 / 一个空段 / 一个分页符段 —— 渲染出来 18 块、359 个码位，见事实 109 |
| `md-lo.docx` | LibreOffice（`md.docx` → .docx） | **渲染一字不差**（359 个码位一个不缺），而账本说得出这一族改了什么：列表号在源件里写在**样式**上（`list_from_style` 5），重写时抄到**段上**（0） —— 搬进 markdown 之后看不出来，因为渲染只问「这一段是不是列表项」，两个数都留着 |
| `eq.docx` | python-docx（`write_equations_docx`，OMML 按原文挂进段里） | 六条式子各占一种写法的凭据：三条行内（`m:oMath` 直接挂 `w:p`）、三条独立成行（在 `m:oMathPara` 里，只有一条自己写了 `m:jc`=`centerGroup`），结构各样一枚（分数 `m:f`、上标 `m:sSup`、根号 `m:rad` 带 `m:degHide`、括号 `m:d` 带 `m:begChr`/`m:endChr`），还有一条把字**点名成普通字**（`m:rPr/m:nor`）—— 式子里的字一共 13 个码位，见事实 110 |
| `eq-lo.docx` | LibreOffice（`eq.docx` → .docx） | 生产者改了什么都在账上：两条行内式被**升级**成 `m:oMathPara`（3+3 变 2+4）、对齐从 1 条补到 4 条而作者写的 `centerGroup` 变成 `center`、`m:nor` 那一条多一枚 `m:lit` —— 字一个没动（13） |
| `eq.odt` | LibreOffice（`eq.docx` → .odt） | 一条式子一个**部件**：六枚 `draw:frame`（`as-char`、尺寸 `0.314cm` 这种自带单位的串）里 `draw:object` 指 `./Object N`，字在 `Object N/content.xml` 的 MathML 里，另有六枚 `ObjectReplacements/Object N` 的替位图；六条的 `display` 一律写 `block`（行内与独立在这一族分不出来），而 `[n]` 那一条在这里比 OMML 多两个括号字符（17 对 13） |
| `eq-od.docx` | LibreOffice（`eq.odt` → .docx） | MathML 回到 OMML 之后与那次 docx → docx 重写**一格不差** —— 两条路走出来的两副件在这一本上同形，所以不替文件合并任何一格 |
| `eq.doc` | LibreOffice（`eq.docx` → .doc，MS Word 97） | 同一份稿子转成 97 的容器：六条式子变成**六枚内嵌 OLE 对象** —— `ObjectPool` 下 `_2147483647`…`_2147483642` 一物一 storage，各带 `\x01Ole` + `\x01CompObj` + 一条正文流 `Equation Native`（MTEF 二进制，59/71/59/56/58/59 字节，一共 362）；`\x01CompObj` 里那三枚串是 `Microsoft Equation 3.0` / `DS Equation` / `Equation.3`。**式子里的字这一本不读**（MTEF 没有第二个读者），所以整个不交 `text` 键；而 piece 表里的嵌入对象锚正好也是 6 枚 —— 两处各数一次 —— 见事实 113 |
| `eqs.odp` | 手写 odp（`write_equations_odp`；两枚公式部件逐字用 LibreOffice 自己写的 MathML） | 两页、三条 `draw:frame`、两枚 `draw:object`：每页一条式子，各自住在 `Object N/content.xml` 里（`parts_found` 2、里面都有 `<math>` 根 → `math_found` 2），frame 写 `text:anchor-type="as-char"` 与自带单位的 `3.261cm`，第一条另有一枚替位图 `./ObjectReplacements/Object 1`（在包里）—— 外壳只能手写：**LibreOffice 没有 odt → odp 的导出过滤器**（实测 `Error: no export filter`） |
| `eqs-lo.odp` | LibreOffice（`eqs.odp` → .odp） | 式子一条不少、字一字不变（`ab` / `12`、`{a} over {b}` / `sqrt {1 2}`），而三格改了：`text:anchor-type` **整个被丢**（`anchors_written` 2 → 0）、每页补一枚装 `draw:page-thumbnail` 的 frame（`frames_in_notes` 0 → 2、`page_thumbnails` 0 → 2，页上的 frame 数仍是 3 —— 所以两份账必须分开）、样式名从 `fr1` 换成 `gr1`，还给第二枚对象写了 `./ObjectReplacements/Object 2` —— **这个部件既不在包里也不在清单里**（`replacements_missing` 1） |
| `eqs.pptx` | LibreOffice（`eqs.odp` → .pptx） | 反向那一转的真实形状：式子不是 OLE 也不是图框，而是**文本体里的 OMML**（`<a:p><a14:m><m:oMath …>`）套在一枚 `mc:Choice Requires="a14"` 里，同一个 `mc:Fallback` 把**那个形状又写一遍**（`cNvPr` 的 id 与名字一字不差）而改挂一张 `ppt/media/imageN.emf` —— 见事实 112 |
| `eqs-pp.pptx` | python-pptx（`write_equations_pptx`，OMML 原文手挂） | pptx 那一本的第二种写法：`a14:m` **裸挂在 `a:p` 里**（与正文 `a:r` 并列），没有 `AlternateContent`、没有替身图 —— `alternates_total` 0、三格 `fallback_*` 全 null。两条式子的字与结构名与 LO 那份一模一样，每页 `p:sp` 从 2 枚变 1 枚。转 odp 时 LibreOffice **把裸挂的那条整条丢掉**（`draw:object` 0），所以这一族不能拿重写当凭据 |
| `md.odt` | LibreOffice（`md.docx` → .odt） | **同一份稿子的第三副样子**：块数与两份 docx 一样是 18、渲染 35 行里**只有一行不同**（图片地址各按自己文件写的交），而账上换了一整套读法 —— 粗斜要一跳字符样式（`spans_unresolved` 0 才算落到字上）、空格是 `text:s` **记号**（`space_markers` 1，实测写成 `两处空格 <text:s/>之间是一个记号`，后半句挂在这个元素的尾上）、列表是嵌套元素（号一律来自 `text:list-style`，`lists_named` 3）、批注与注**嵌在正文段里面**（跳过的条数各记一格），见事实 109 |
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
| `errors.xlsx` | openpyxl（`write_errors_xlsx`） | 六条公式一个都不算：`<c r="B1"><f>1/0</f><v></v></c>` —— 没有 `t`、`<v>` 是空的，于是「除零长什么样」在这份里根本不存在（`error_cells` 0、`cells_with_written_type` 3）；另有一格布尔常量 `D1`（`t="b"` 写 `1`） |
| `errors-lo.xlsx` | LibreOffice（`errors.xlsx` → .xlsx，同一个格式重算一遍） | 同一批格子第二种写法：`t="e"` 三格（`#DIV/0!`、`#N/A`、`#VALUE!`，显示串就是文件写的）、`t="str"` 一格（`甲乙`，不走共享字符串表）、九格全写 `t`，而那个布尔常量被写成 `<f>TRUE()</f>` 一条公式（6 条公式变 7 条） |
| `errors.ods` | LibreOffice（`errors.xlsx` → .ods） | 第三种摆法：错误格 `office:value-type="string"`（不是 error）加一个空的 `office:string-value`，显示的那串只在 `<text:p>` 里；`calcext:value-type="error"` 那条副本不跟；同一个坏掉的 VLOOKUP 在这里叫 `错误:502` 而不是 `#VALUE!` |
| `epoch.xlsx` | openpyxl（`write_epoch_xlsx`，`wb.epoch` 换成 1904） | 全套断言里第一份写 `date1904` 的件（`date1904="1"`）：`2013-12-23` 在这里的序列号是 **40169**（1900 基准下不是这个数），另有一格序列号正好 **60**（1904 基准下是 1904-03-01，那个闰年 bug 的特例只许在 1900 那一边生效），还有一格像日期的字（`t="inlineStr"`） |
| `epoch-lo.xlsx` | LibreOffice（`epoch.xlsx` → .xlsx，同一个格式重写） | 同一个开关换成 `date1904="true"`、序列数照旧，而带时刻那一格的小数从 15 位截到 10 位（换算出来的秒不变）；那格字换成了 `t="s"` 指共享字符串，日期格式串也换成小写带转义的 `yyyy\-mm\-dd` |
| `rich.xlsx` | openpyxl（`write_rich_xlsx`，`CellRichText` + `InlineFont`） | 一个格子的字分成几段的第一种摆法：**整个文件没有 `sharedStrings.xml`**，富文本全写成行内串（`t="inlineStr"` + `<is><r><rPr><b val="1"/>…</rPr><t>重要</t></r>…`）；A2 两段各有格式，A6 第一段**整个没有 `rPr`** 而第二段有（「没写」与「写了但是空的」），A3 首尾各两个空格、A7 开头一个制表符（这两格的 `t` 带 `xml:space="preserve"`，其余不带），A1 与 A5 是同一条「甲」（被引用两次），B1 的粗体写在**格子上**不在串里 |
| `rich-lo.xlsx` | LibreOffice（`rich.xlsx` → .xlsx，同一个格式重写） | 第二种摆法：八次引用全搬进 `sst`（自报 `count="8"` 配 `uniqueCount="7"`，七条串），每一个 `t` 都补 `xml:space="preserve"`，同一个粗体开关改写成 `val="true"` 并补 `family` / `charset`，A7 那一格还按字体 fallback **切成两段**（`Calibri` 与 `Noto Sans SC`）—— 分段数是生产者的决定，只交不比 |
| `rich.ods` | LibreOffice（`rich.xlsx` → .ods） | 第三种摆法，而且是记号不是字面：`  两头有空格  ` 写作 `<text:s text:c="2"/>…<text:s text:c="2"/>`（`text:c` 说这一个记号顶几个空格），A7 的制表符是 `<text:tab/>`，A4 的两行是两个 `<text:p>`；富文本变成 `<text:span text:style-name="T1">`（那三份字符样式不追，只交 `spans` / `specials` 两本条数） |
| `pipes.xlsx` | openpyxl | markdown 表格的两个装不下的东西：格里的**竖线**与**格内换行**。五个变量分开摆 —— 竖线在中间（A2 `a|b`、B2 `1|2|3`）、竖线在首尾（A4 `|首尾都带|`）、整格只有一个竖线（B4 `|`）、一格同时带竖线与换行（A3）、只带换行（B3）。**做这副的理由是量出来的**：`--markdown` 那两个转义分支在库里没有任何一格带竖线，换行那一支只有 `rich*` 覆盖（`第一行\n第二行`）
| `pipes-lo.xlsx` | LibreOffice（`pipes.xlsx` → .xlsx，同一个格式重写） | 第二个生产者：竖线是普通字符，两家都原样带着走（`sst` 与行内串两条路都过）；这一副铺出来的方格与上一副逐字相同。**但串里的换行写法不同**：openpyxl（Windows，3.1.5）把值里的 `\n` 写成 `\r\n`，LibreOffice 重写时写成 `&#10;` —— 行尾归一是 XML §2.11 要求读者做的，见事实 121（写出这一副的解释器没有 lxml，openpyxl 因此用标准库序列化 —— 与语料其余部分不同，README 前言那条有记）
| `pipes.ods` | LibreOffice（`pipes.xlsx` → .ods） | 第三种存法：换行是三个 `text:p`（不是格里的 `\n` 字符）、竖线仍是串里的普通字 —— 三家到 markdown 这一个出口上铺出同一份文本，这条等式在探针里钉着
| `styled.xlsx` | openpyxl（`write_styled_xlsx`） | 「长相」那一跳的第一种写法：五份字体（默认那份什么都不写、粗体深红换字体、`<i/><u/>` 那份连 `name`/`sz` 都没有、只有 `<color indexed="64"/>` 的、只有 `<color theme="1" tint="0.5"/>` 的）、四条填充（**第 0 条是空的 `<patternFill/>`**、第 1 条 gray125 占位、实心黄底只写 `fgColor`、`lightGrid` 写 `fgColor` + `bgColor`）、两条边界（第 0 条五个空孩子、第 1 条四条 `style="thin"` 各带一个 `color`）、十条 `cellXfs` 而 `cellStyleXfs` 只有一条；只有一格写了 `applyAlignment="1"` 并带 `<alignment horizontal="right" vertical="center" wrapText="1"/>`，`A3` 那一格**连 `s` 都不写** |
| `styled-lo.xlsx` | LibreOffice（`styled.xlsx` → .xlsx，同一个格式重写） | 第二种写法：九份字体（多出来的是 Arial 10 那几份占位）、`cellStyleXfs` 从 1 条变 20 条、每格都写 `s="…"`、粗体开关换成 `val="true"`、`indexed="64"` 那个颜色被换成 `rgb="FF000000"`、空占位改成 `patternType="none"`、`solid` 那一条补出 `bgColor`，而**点状网格底整个换成实心底并改了颜色**（`FF00B050` → `FF90DDB3`）；每一格还补一份写着 `wrapText="false"` 的 `alignment` —— 「没写」与「写了关」在两副件里是两个不同的数 |
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
| `toc.docx` | LibreOffice 的 **docx 导出器**（把目录注进 `notes.docx` 再让它照抄） | **壳是真的、条目是注进去的那一句**：`<w:sdt>` + `<w:docPartGallery w:val="Table of Contents"/>`（这一份没有排出来的条目，`entries` 因此是 0 —— 有排过的看 `toc-full.docx`，见事实 119），级别在域指令文字里 —— LibreOffice 把引号写成 `&quot;`，所以 `TOC \o "1-2" \h` 要还原实体才读得对 |
| `toc.odt` | LibreOffice（从 `toc.docx`） | 同一件东西的另一副面孔：`text:table-of-content`（名字 `目录1`）、级别在 `text:table-of-content-source/@outline-level="2"`，另外**十级条目模板全写出来**（`entry_templates` 报的是文件写了几个，不是用上了几级） |
| `toc-full.docx` | LibreOffice **自己排过一遍**的目录（`md.docx` 注壳 + 一个分页符 + `w:updateFields` → 临时 profile 里的 Basic 宏 `index.update()` → `storeToURL`） | 两条**排出来的**条目：`w:pStyle` 是 `TOC1` / `TOC2`（级别就写在这个号上），地址是 `w:hyperlink/@w:anchor="__RefHeading___Toc52_…"`，页码是 `w:tab` **之后的一段字面**（`1` 与 `2`，一条 `PAGEREF` 域也没有）；`w:sdtContent` 里还多一段「目录」标题（`paras` 3 而 `entries` 2）。段上另写着两条 `w:pPr/w:tabs/w:tab` —— 那是**制表位定义**，不是段里那一下记号 |
| `toc-full.odt` | LibreOffice（同一份宏的 `writer8` 那一次存盘） | 同两条条目的 ODF 写法：容器是 `text:index-body`（标题嵌在 `text:index-title` 里，所以这里 `paras` 2 == `entries` 2），地址在 `text:a/@xlink:href` 上**带 `#`**，页码同样在 `text:tab` 之后（挂在元素的**尾**上，只收 `.text` 就一个字也读不到），而段点的样式是自动样式 `P1` / `P2` —— 那两个号与级别无关，所以级别要顺锚点两跳去看被指那段 `text:h` 写的 `text:outline-level` |
| `toc-full.rtf` | LibreOffice（从 `toc-full.docx` 再转一次） | 第三族的缓存条目：`{\field{\*\fldinst { TOC \\z \\o "1-2" \\u \\h}}{\fldrslt {…结构：一级}{\tab 1}\par …}}` —— 字与页码在**结果群**里，级别在段前的 `\s140` / `\s141`（样式表里那两条的名字是 `toc 1` / `toc 2`）。条目那一本这一族也交了（`scope` 是 `fldrslt`：`paras` 2 == `entries` 2，标题那一行在域外面；`with_anchor` 0 而 `target_marks_written` 2 —— 见事实 123） |
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
| `cell-locks.xlsx` | openpyxl 打底 + `zipfile` 按 ECMA 手写五枚 `protection` | 单元格样式自己那两枚锁定位的形状都摆在一份里：`cellStyleXfs` 一枚**空的** `<protection/>`、`cellXfs` 四枚各一样（`locked="1" hidden="0"` 的另一种拼法、`locked="true" hidden="true"`、`locked=""` 写了名而值是空串、多一枚本层没人写过的 `lockRule="all"`），`dxfs` 那本在场而**一枚都不写** —— 见事实 151 |
| `cell-locks-lo.xlsx` | LibreOffice（`cell-locks.xlsx` → .xlsx） | 同一份重写一遍把这一层**补齐**：`cellXfs` 八枚全写（原来只有四枚）、拼法全换成 `true` / `false`（`1` 与 `0` 两枚穿过转写就不在）、空元素与 `lockRule` 整个不见 —— 见事实 151 |
| `locked-sheet.xls` | LibreOffice（从 `locked-sheet.xlsx`） | BIFF8 的表级保护：`0x0012` + `0x0013` + `0x00DD` 三条，写在**被锁那张表自己的子流**里 |
| `locked-second.xls` | LibreOffice（从 `locked-second.xlsx`） | 对照的另一半：锁挪到第二张，这三条跟着挪窝 —— 「按表记」是这么量出来的，不是按记录名推的 |
| `notes.doc` | LibreOffice（从 `notes.docx`） | MS-CFB 复合文档 + WordDocument 流 + `1Table` 里的 piece 表 |
| `notes-en.doc` | LibreOffice（从纯 ASCII 的 `notes-en.docx`） | 中英一视同仁仍写 16 位 piece —— 记下这个事实，见下 |
| `book.xls` | LibreOffice（从 `book.xlsx`） | BIFF8：BOUNDSHEET（含隐藏表）、SST + CONTINUE、LABELSST / RK / FORMULA |
| `formats.xls` | LibreOffice（从 `formats.xlsx`） | 数字格式那一跳的真件：格子的 ixfe → XF 记录（0x00E0，格式号在正文偏移 2）→ FORMAT 记录（0x041E）；这份里自定义号 165~169 是日期/日期时间/百分比/¥/汉字日期，另有内置号 9 与 41~44 |
| `mulrk.xlsx` | openpyxl | 一行连续的八个数字 + 隔开一行三个 —— 就是为了逼出 MULRK 那种记录 |
| `mulrk.xls` | LibreOffice（从 `mulrk.xlsx`） | BIFF8 的 MULRK(0x00BD)：一行连续格子共用一条记录，每格自己带 `{ixfe(2), rkmac(4)}`；这份件里正好两条（8 格与 3 格） |
| `deck.ppt` | LibreOffice（从 `deck.pptx`） | PowerPoint 97 记录树 + 一个 59 万字节、走 FAT 的属性集流（大流那条分支的样本）；按 `0x03EE` 归出 2 页，与 `deck.pptx` 每页逐张一致 |
| `deck-ph-lo.ppt` | LibreOffice（从 `deck-ph-lo.pptx`） | .ppt 那一族的第二与第三副样本：四页里有占位符页、只有一个自由文本框的页与**整页没有标题块**的那一页（`blocks` 空表是数出来的）；pptx 那头 `a:buChar` 的两段与 `a:buNone` 的两段，转过来段属性一字不差 —— 项目符号这一问在这族没有凭据，见事实 124 |
| `deck-tables-lo.ppt` | LibreOffice（从 `deck-tables-lo.pptx`） | 一页一张 3×3 的表：在 .ppt 里被**摊平成八块文字**（两块自己带着文件写的 \r），所以「这页有几块字」与「这张表有几格」在两种存法里不是同一个数；每块前面那个四字数值在这里只有 0（标题那块）与 4（其余全部）|
| `notes.rtf` | LibreOffice（从 `notes.docx`） | 字体表、颜色表、样式表、`\*\userprops`、域代码与 `\'hh` 回退字节 |
| `hidden.xlsx` | openpyxl 3.1（`write_hidden_xlsx`） | 第 3、4 行隐藏，C/D/E 三列隐藏，**D2/E2 里有字**；一列一条 `<col min="3" max="3" hidden="1">` |
| `hidden-lo.xlsx` | LibreOffice（`hidden.ods` 转回 OOXML） | 同一份账的另一种写法：`<col min="3" max="5" hidden="true">` 一条盖三列，没隐藏的行也写着 `hidden="false"` |
| `hidden.ods` | LibreOffice（从 `hidden.xlsx`） | 隐藏换成 `table:visibility="collapse"`，列那一跳还带 `number-columns-repeated="3"` |
| `notes.pdf` | LibreOffice（从 `notes.docx` 导出） | Writer 那份的 PDF：2 页 letter、5 张子集 TrueType 字体（每张都带 `/ToUnicode`）、2 张 8×8 图、`/Lang (en-US)`、`/MarkInfo /Marked true`、一个 URI 批注、Info 里七个键（`/Title` 是 `<FEFF…>` 的 UTF-16BE 中文） |
| `deck.pdf` | LibreOffice（从 `deck.pptx` 导出） | 同一批字的 Impress 存法：页面 `0 0 720 540`、`/Lang (zh-CN)`、6 张字体、没有批注 —— 与 `notes.pdf` 一起把「不同应用 → 不同页尺寸与语言」钉住 |
| `pdf-comments.pdf` | LibreOffice（从 `notes.docx` 导出，带 `ExportAnnotations=true`） | 同一份 docx 的第二种导出：页上三条注记（13 号链接、11 号 `/Text` 批注、12 号 `/Popup`），批注的 `/T` 把作者与日期拼成一条串 `"liuqi, 09/23/26, "`、`/Contents` 是那句中文、`/M` 是一串全零的 `D:00000000000000Z`；那条 `/Popup` 也在同一个 `/Annots` 数组里并用 `/Parent` 指回批注，`/Rect` 在页面外 |
| `objstm.pdf` | qpdf 12.3.2（经 pikepdf 10.13，从 `notes.pdf` 再存） | **68 个对象里只有 17 个是明写的**：另外 51 个挤在一个 `/Type /ObjStm`（`/N 51 /First 388`）里；文件里**没有 `trailer` 这个词**，`/Root`、`/Info` 只写在 `/Type /XRef` 的流字典里（`/Size 69`）。只扫 `obj` 的读者会报「0 页」，只认 `trailer` 的读者找不到元数据 |
| `locked.pdf` | qpdf（`Encryption(R=6)`，从 `notes.pdf`） | AES-256 真加密，口令 `lbin-test`（owner `lbin-owner`；这是测试件，口令不是秘密）。`pdfinfo` 不给口令直接 `Incorrect password`；`/Encrypt` 指着的字典是 `/Filter /Standard`、`/V 5`、`/R 6`、`/Length 32`、带 `/O` `/U` `/OE` `/UE` `/P` |
| `perms.pdf` | qpdf（pikepdf，从 `notes.pdf`） | **只设 owner 口令**的一份（用户口令为空）：于是 `/P` 那些位真的生效，工具也进得去 —— pdfinfo 读成 `Encrypted: yes (print:no copy:no change:yes addNotes:no algorithm:AES-256)`，与 `lyco_pdf_nav.py` 从 `/P -3384` 算出的位逐条一致 |
| `risk.pdf` | **手搓**（`office_fixtures.py` 的 `write_risk_pdf`，逐对象自数 `<<`/`>>`） | LibreOffice 不肯写的五种形状：`/AcroForm` + 一个 `Tx` 字段、文档级 `/JavaScript`（名字树 + 流）、页 `/AA` 触发的脚本、`/Launch` 动作（打开 `winword.exe`）、`/EmbeddedFiles` 附件 `badge.exe`；页对象**不写** `MediaBox`/`Rotate`，从 `/Pages` 继承。写完用 `pdfinfo` 验：`Form: AcroForm`、`JavaScript: yes`、`Pages: 1`、`Page size: 612 x 792`、`Page rot: 90` —— 五条都被第三方读者认了才算 fixture |
| `forms-hier.pdf` | **pikepdf 挂出来的**（`office_fixtures.py` 的 `write_forms_hier_pdf`，底本 `notes.pdf`）；写完用 pypdf 独立读回一遍 | 表单那一份账要的几种形状，编辑器没一个肯写：`/FT /Tx` 与 `/Ff 4` 只写在祖父 `Person` 上（`Address` 往上跳一跳、`City` 跳两跳才拿到）、`/Kids` 三层、`/Opt` 的两种合法写法各一份（成对 `[[1 一] [2 二]]` 与摊平 `[(甲) (乙) (丙)]`）、`/V` 的三种情形（写空串 / 写成数组 / 整个没写）、两条 Widget 同时挂在页的 `/Annots` 上。**这一份不是编辑器导的** —— 见事实 62 |
| `images.docx` | python-docx（`write_images_docx`，图是 `write_dot_png` 现写的 40×24 PNG） | 文档里那张图的第一种摆法：只有 `wp:inline`（属性一个不写，`xmlns:` 那几条声明不算）、`wp:extent cx="1440000"`（换算 4000）与 `pic:spPr/a:xfrm/a:ext` 同一个数、替代文字只写在外头 `wp:docPr` 上（`descr="一个红点"`）而 `pic:cNvPr` 那里写的是**原文件名** `dot.png` 且根本没有 `descr`、锁只有 `a:graphicFrameLocks noChangeAspect="1"` 一份、`a:blip` 的号是 `rId9` |
| `images-lo.docx` | LibreOffice（`images.odt` → .docx，同一条 LO 写的第二副 OOXML） | 同一张图的第二种摆法：`wp:inline` 补四个 `dist*="0"`、两处尺寸都换成 `1440180`/`864235`（**换算成 4001 与 2401，与 python-docx 那份不是一个数**）、补一条 `wp:effectExtent l/t/r/b`、把名字与那句替代文字**抄进 `pic:cNvPr`**、另补一份 `a:picLocks`（两个开关），而号换成了 `rId2` —— 解出来的部件还是同一个 `word/media/image1.png` |
| `images.odt` / `images.rtf` | LibreOffice（从 `images.docx` 导出） | 另两种存法：ODF 把尺寸写成**自带单位的串**（`svg:width="4.001cm"` → 同一个 4001）、摆法写在**属性** `text:anchor-type="as-char"` 上、替代文字搬到**孩子元素** `svg:desc`、地址是 `draw:image/@xlink:href`（没有关系表这一层，路径是生产者按图片尺寸自己拼出来的那个长名）；RTF 只留 `\picscalex472 / picw40 / pich24 / picwgoal480` 那一串与 `pngblip`，而替代文字搬进了 `{\*\picprop}` 里的 `{\sn wzDescription}` |
| `images-float.docx` | LibreOffice（从 `poke_anchor` 改过锚点的那份 ODT 导出） | 「浮在页上、文字绕着排」那一种，**手上没有一个生产者会自己写出来**（python-docx 只写 inline，LibreOffice 插入默认也是 inline），所以输入是把 `images.odt` 那格的 `text:anchor-type` 改成 `page`、样式换成同一份 `styles.xml` 里带 `style:wrap="dynamic"` 的 `Graphics`；**输出那份 docx 的每个字节都是 LibreOffice 写的**：`wp:anchor` 带十个属性、`wp:simplePos`、`wp:positionH relativeFrom="column"` 里面写 `<wp:align>center`（词在字里）、`wp:positionV relativeFrom="paragraph"` 里面写 `<wp:posOffset>635`（数在字里）、`wp:wrapSquare wrapText="largest"` |
| `images-float.odt` | LibreOffice（从 `images-float.docx` 转回 ODF） | 那一种摆法换到 ODF 里成了**另一个词**：`text:anchor-type="char"`（不是 `as-char`），另补 `svg:y="0.002cm"` 与 `draw:z-index="0"` —— 同一个选择在两家的文件里是两个串，各按各的交；来回一圈之后 OOXML 那一侧的 `wp:anchor` 与绕排也不见了（重写不是无损的，这里正看得见） |
| `deck-pictures.pptx` | python-pptx（`write_pictures_pptx`，图仍是那张 40×24 的 `dot.png`） | 页上那张图的第一种摆法：尺寸只有 `a:xfrm/a:ext` **一处**（`1440000` = 4000）而位置 `a:off` 也在同一层（`360000` = 1cm）、alt 只有 `p:cNvPr/@descr` 一处（第一页写「一个红点」，**第二页什么都没给，python-pptx 把文件名 `dot.png` 填进了那个键**）、第二页不写宽高，于是按 72 DPI 换成 `508000`（=1411），另有 `a:picLocks noChangeAspect="1"` 与 `<a:stretch><a:fillRect/></a:stretch>`；第三页一张图也没有（交空表） |
| `deck-pictures.odp` | LibreOffice（从 `deck-pictures.pptx` 导出） | 换一家：`svg:width="3.999cm"`（同一个 3999）、alt 搬成孩子元素 `svg:desc`、地址是 `draw:image/@xlink:href`，而**图框不写** `text:anchor-type`（odt 那边写 `as-char`）→ 那一族的 `placed` 是 null |
| `deck-pictures-lo.pptx` | LibreOffice（`deck-pictures.odp` → .pptx，一趟来回） | 来回之后不见的三样：`a:picLocks` 整个没了、`<a:stretch/>` 缩成空的（`fillRect` 没了）、尺寸从 `1440000` 换成 `1439640`（4000 → 3999）；留下的：名字与两句 alt 一字未变、`a:off` 分毫未动，而号全被重排（`rId2` → `rId1`、形状 id 2 → 63） |
| `deck-bg.pptx` | python-pptx 1.0.2（`write_background_deck`） | 四页一次只改一个变量的**页底色**：`第1页` 实色 `1A1A2E`、`第2页` **显式**写 `<p:bg><p:bgPr><a:noFill/>`、`第3页` 渐变（两站都点 `schemeClr accent1`，修饰 `tint 100000/50000` + `shade 100000` + `satMod 130000/350000` 全挂在那枚颜色元素的**孩子**上，方向写 `a:lin @scaled="0"`）、`第4页` 整枚 `p:bg` 都不写；每页底色后面跟一枚**空的** `<a:effectLst/>`。模板那份只在**母版**写 `bgRef idx="1001"` + `schemeClr bg1`，11 份版式一枚都不写 |
| `deck-bg-lo.pptx` | LibreOffice（`deck-bg.pptx` → .pptx 重写） | 同四页，三处搬家：第 1 页字面色一字未动；第 3 页两站换成**替文件算完**的字面 `srgbClr 3E7FCC` / `A4C1FF`（修饰整批不写、`@scaled` 改 `@ang="0"`、`rotWithShape` 从 `1` 变 `0`）；**第 2 页那枚 `noFill` 整条丢掉**，于是它与第 4 页交出同一份全 null 的记录。空壳 `effectLst` 一枚都不写；母版那枚 `bgRef bg1` 被**摊到 11 份版式上写成字面 `FFFFFF`** 而母版自己那枚没了（`parts_with_bg` 4 → 13） |
| `deck-bg.odp` | LibreOffice（`deck-bg.pptx` → .odp） | 同一件事换了地方：页只**点名**（dp1 / dp3 / dp4 / **dp3**），第 2、4 页共用 dp3，而那份样式的 `drawing-page-properties` 里 5 句话一条 `draw:fill` 都没有；渐变要两跳才解得开（`draw:fill-gradient-name="msFillGradient_20_1"` → styles.xml 的 `<draw:gradient>`，八格属性含 `angle="90deg"`）；继承那一跳是三跳 `Blank → Mdp1 → draw:fill="solid" #ffffff`，11 份 `style:master-page` 全部点同一份 Mdp1，另两份样式（content.xml 的 dp2、styles.xml 的 Mdp2）**没人点**而四句一字不差 |
| `customxml.docx` | python-docx 打底 + `zipfile` 按真件形状加部件（`add_customxml_parts`） | 包里两枚自定义 XML 存储：`customXml/item1.xml` 是 Word 引用管理器那份 `b:Sources`（三条孩子）加 `item2.xml` 一枚 `s:customData`，各带一份 `itemPropsN.xml`（第一枚有一条 `ds:uri`、第二枚的 `ds:schemaRefs` 在而里面空——真件 25 份里就有一份这样写），item 自己不点名、靠 `Default Extension="xml"` 兜；另在正文合成两条手指（一枚 `w:customXml`、一枚带 `w:dataBinding` 的 `w:sdt`，号指向第一枚）——真件里这两条 0 份写过（本机 33 份真件 docx/docm，排在仓库之外；`w:sdt` 那 4 枚全在页脚且一枚都不带 `w:dataBinding`），所以这里是**合成**的；注意别把自产件的数当真件读——本仓 90 份自产件里那 6 枚带 `w:dataBinding` 的控件是自己写的，见事实 147
| `customxml-lo.docx` | LibreOffice（`customxml.docx` → .docx 重写） | 同一件事三种待遇：三枚 `itemN.xml` 全被清空成 0 字节而 props 与那一跳留着、存储从两份变三份、头两份 props 的 `ds:itemID` 撞成同一个号（正文那一条手指说不清指到哪一份），`w:customXml` 整条丢掉而 `w:dataBinding` 照样留着 |
| `alternate.docx` | python-docx 打底 + `zipfile` 插三块（`write_alternate_docx`） | `mc:AlternateContent` 的三种形状一次摆开：一块配齐（Choice 点 `wps` + Fallback）、一块只有 Choice（点 `w14`，**没有退路**）、一块里两条 Choice 共用一份 Fallback。只有 Choice / 两条 Choice 那两形按 ECMA 的写法**合成**（两个生产者都不这么写，本机真件里 4 块没有一块写两条 Choice）
| `alternate-lo.docx` | LibreOffice（`alternate.docx` → .docx 重写） | 三块**连字一起丢掉**（`blocks` 3 → 0，五句只写在分支里的字一句不剩）—— 这一族最狠的一条生产者差异 |
| `row-height.docx` | python-docx（`write_row_height_docx`） | 这一行多高的三种「没有」一次摆开：`w:trPr/w:trHeight` 的 `@w:val` 与 `@w:hRule` 各说一半（`exact` 1361 / `atLeast` 680 / 第四行写 `0` 而把 `@w:hRule` **摘掉**），第三行故意连 `w:trPr` 都不写 —— 三种「没有」是三格：`rows_without_tr_pr` 1、`rules` 里 `(没有 trHeight)` 与 `(没写)` 各 1；缺 hRule 不补 atLeast |
| `row-height.odt` | LibreOffice（`row-height.docx` → .odt 那一转） | 换一家：行只写 `table:style-name`，`style:row-height` 与 `style:min-row-height` 两个键在那一跳的目的地里（`2.401cm` / `1.199cm` —— docx 那 1361 与 680 twips 经厘米一绕就多了个 1，本仓不换算也不比对）；"什么都不写"那一行的样式里两个键都没有、只剩 `keep-together`（`rows_unwritten` 1 而 `styles_unfound` 0），零那一行是 `min-row-height="0cm"` |
| `row-height-lo.docx` | LibreOffice（`row-height.odt` → .docx，一趟来回） | 零不翼而飞：`@w:val="0"` 被它写成 `@w:val="1" @w:hRule="atLeast"`（它不承认零高，`1` 是它自己挑的数），而那个空行回来时带着一枚**空壳** `w:trPr`（`has_tr_pr` true 而 `tr_pr_children` 空表）—— `rows_without_tr_pr` 1 → 0、`zero_height` 1 → 0，两处都是「壳在」与「壳里写了什么」两个数 |
| `bullets.pptx` | python-pptx 打底 + 按 ECMA 手写 `a:pPr`（`write_bullets_pptx`） | 四页四种答案：一页什么都不写、一页三枚 `buChar`（含 `buSzPct` 与 `spcAft/spcPts`）、一页两枚 `buAutoNum`（`startAt` 只写一条）、一页两枚 `buNone`（其中一条 `marL` 留大） |
| `bullets.odp` | LibreOffice（`bullets.pptx` → .odp 那一转） | 换一家：段上不写，写的是 `text:list/@text:style-name`，样式里十级各一枚 `list-level-style-bullet`/`-number`（第 3 页那两条编号的样式带 `start-value="3"`），而第 4 页整页没有列表 |
| `bullets-lo.pptx` | LibreOffice（`bullets.pptx` → .pptx 同格式重写） | 每段都被补上 `a:pPr`（13 段全有，`silent` 0），`marL` 从 `342900` 变 `343080`，母版的 `p:txBody` 整个丢掉（带 `a:lstStyle` 的件从 12 变 11） |
| `margins.docx` | python-docx 打底 + 按 ECMA 手写 `w:tblCellMar` / `w:tcMar`（`write_margins_docx`） | 四张表四种答案：一张表级写满四条、一张只写左右、一张写了块但一条方向都没有（空壳）、一张连块都没有；12 格里四格自己改过，其中一格只写一条 `w:type="auto"`，另一格四条全零 |
| `margins-lo.docx` | LibreOffice（`margins.docx` → .docx 同格式重写） | 四张表并成一张、方向改名 `start/end`、11 格各补四条、`auto` 换成 `dxa`，表级那四条换成第一个格写过的值（`57` / `170`） |
| `margins.odt` | LibreOffice（`margins.docx` → .odt 那一转） | 同一问换成两跳：12 格各点一份 `family="table-cell"` 的自动样式，11 份写四枚 `style:padding-*`、全零那份被收成短款 `fo:padding="0cm"`；`113` twips 落成 `0.199cm` |
| `margins.pptx` | python-pptx 的 `cell.margin_*`（`write_margins_pptx`） | 六格里两格写满四枚、一格只有 `marL`、三格一枚都没有（第二页那张表整个没人写过）；`marT` 写的是 `36576` EMU |
| `margins-lo.pptx` | LibreOffice（`margins.pptx` → .pptx 同格式重写） | 六格全被补齐四枚，而 `36576` 绕一圈回来是 `36360`（过一遍磅再进 EMU）；两本各按写的交，不换算也不判谁对 |
| `borders.docx` | python-docx 打底 + 按 ECMA 手写 `w:tblBorders` / `w:tcBorders`（`write_borders_docx`） | 三张表三种答案：一张六方向写满、一张连块都没有、一张表级只写 `nil` 与两条实线；格级另有 `double` + 主题指针、`tl2br` 对角线、一条 `none` 与一枚空壳 |
| `borders-lo.docx` | LibreOffice（`borders.docx` → .docx 同格式重写） | 三张表并成一张、表级那份全摊到格上（10 格各有块）、方向改名 `start/end`、`nil` 与 `auto` 与主题指针全换成写实的数 |
| `borders.odt` | LibreOffice（`borders.docx` → .odt 那一转） | 10 格各点一份 `family="table-cell"` 的自动样式，四枚长款 `fo:border-*` 一条不落；`sz="8"` 变成 `1pt solid #000000`、`double` 变成 `2.25pt double`、`nil` 与 `none` 合成同一个 `none`，另有一枚第三种写法 `style:border-line-width-top` |
| `borders.odp` | LibreOffice（`borders.pptx` → .odp 那一转） | 边框住在 `style:paragraph-properties` 上（不是 graphic-properties），短款与长款各 2、虚线 `dashed` 转过来还在，六格里两格连样式名都不点 |
| `borders.pptx` | python-pptx 打底 + 按 ECMA 手写 `a:ln*`（`write_borders_pptx`） | 六格里 1 格写满上下左右、2 格只写一两枚（含一枚 `@w="0"` 与一枚 `noFill`、一枚对角线 `lnTlToBr`）、3 格一枚都没有（第二页那张表整个没人写过）|
| `borders-lo.pptx` | LibreOffice（`borders.pptx` → .pptx 同格式重写） | 六格全被补齐四条，`6350` → `6480`、`12700` 与 `25400` → `12240`，对角线整个丢掉，还有一枚线不写 `@w` |
| `valign.docx` | python-docx 打底 + 按 ECMA 手写 `w:tcPr/w:vAlign` 与 `w:sectPr/w:vAlign`（`write_valign_docx`） | 四态一次摆全（`center` / `top` / `bottom` / `just`）+ 两格连元素都没有；另给这一节写一枚同名的节属性（另一个问）|
| `valign-lo.docx` | LibreOffice（`valign.docx` → .docx 同格式重写） | 只剩 `center` 与 `bottom` 两格还写着，`top` 与 `just` 整枚被丢；节上那一枚留着 |
| `valign.odt` | LibreOffice（`valign.docx` → .odt 那一转） | 词表换成 `middle` / `bottom`，两格被写成**空串** `style:vertical-align=""`，值住在 `style:table-cell-properties` 上 |
| `valign.odp` | LibreOffice（`valign.pptx` → .odp 那一转） | 表与样式都在，而**没有任何一格带这一条** —— 转换整层没写下来，那一本只交 0 |
| `valign.pptx` | python-pptx 打底 + 按 ECMA 手写 `@anchor`（`write_valign_pptx`） | 四态各一枚，其中一枚另带 `@anchorCtr="1"`；两枚都不写的两格与「没这枚属性」同形 |
| `valign-lo.pptx` | LibreOffice（`valign.pptx` → .pptx 同格式重写） | 六格全被补上 `@anchor`（`t` 四、`ctr` 一、`b` 一），`just` 被换成 `t`，而 `@anchorCtr` 一枚不剩 |
| `stats.pptx` | python-pptx 打底 + 按 ECMA **手写** `docProps/app.xml`（`write_stats_pptx`） | 多少字那三份账：两页正文 + 一页备注 + 一张表，而自报写着 `Words=999999`、`Paragraphs=7`（非零而不对）、`Slides=2`（与页部件数对得上）、`Notes=0`（备注部件其实有一枚） |
| `stats-lo.pptx` | LibreOffice（`stats.pptx` → .pptx 同格式重写） | 手写的 `Words`/`Paragraphs` 原样搬过去，而 `Slides`/`Notes`/`HiddenSlides`/`MMClips` 四个名整个丢掉；页上的字符数一个字没动，文字原子从 7 枚被拆成 9 枚 |
| `stats.odp` | LibreOffice（`stats.pptx` → .odp 那一转） | `meta:document-statistic` **只写一条** `object-count="144"`（字数、字符数、段落数、页数一条都没有），而备注在 `draw:page` 里面 |
| `sdt.docx` | python-docx 打底 + 按 ECMA **手写**十一枚 `w:sdt`（`write_sdt_docx`） | 内容控件那一层的形状：11 枚住 1 个部件、类型元素五样各写各的（`text` 2、`richText` 3、`date` 1、`dropDownList` 1、`docPartObj` 1，另 3 枚**没写类型元素**）、`w:sdtPr` 的孩子序就是文件自己写的序（`alias`/`tag`/`id`/`text`/`placeholder`）、一枚套娃（外层 `depth` 0、里层 1）、两枚带 `w:sdtEndPr`、一枚带 `w:dataBinding`（四个属性齐，`storeSchemaID` 是第四个）、下拉那枚两个候选按文件自己写的序、正文那一层的两个口径 11 对 14 而住在表格格子里的那两格也算 |
| `sdt-lo.docx` | LibreOffice（`sdt.docx` → .docx 同格式重写） | 重写动了七处，每一处都是「说了别的话」：11→10（正文空着的那枚**整枚不见**）、`w:sdtEndPr` 那两枚全丢、`richText` 降级成 `text`、`w:alias` 被改写成**空串**（不是没写）而同名的 `w:id` 于是没了、日期那枚把文件写的 `dateFormat`/`calendarType` 两格换成它自己算的 `fullDate` 一格、绑定那枚的孩子**换了序**（`dataBinding` 挪到最后）而它自己的四格属性只剩三格（`storeSchemaID` 被丢掉）、套娃外层失去类型元素；段被摊平成 `w:r`：两个口径都变 3 而 runs 23 |
| `sdt.odt` | LibreOffice（`sdt.docx` → .odt 那一转） | ODF 标准里没有这一层（`<form:` 零枚），而 LibreOffice 把那 10 枚中的 6 枚写进自家扩展命名空间 `loext:content-control`（五个名 `loext:alias` / `id` / `tag` / `lock="sdtContentLocked"` / `plain-text`，**没有类型**），控件里的字与段落都还在（`text:p` 14）—— odt 读者不读 `loext:`，所以这一族对 ODF 交的是「没有这一格」而不是「这一层不存在」 |
| `levels.docx` | python-docx 打底 + 按 ECMA **手写**七枚样式与十二段（`write_levels_docx`） | 那一段是第几级的十一种形状：号写成**数字**（`1`/`2`/`15`/`21`/`31`/`5`/`7`）而 "heading 1"、「标题 #1」写在样式的 `w:name` 上、样式自己也带一枚 `w:pPr/w:outlineLvl`、两处都给且**给得不一样**（名字 1 对样式 3）、名字不像标题而级只在样式上、段自己写 `w:outlineLvl`（一枚 2、一枚 **9**）、点了样式表里根本没有的号 `999`、样式写了 `outlineLvl` 而 `@w:val` 是**空串** |
| `levels-lo.docx` | LibreOffice（`levels.docx` → .docx 同格式重写） | 重写动了六处：号被换成可读 id（`Heading1`/`CustomText`/`emptyoutline`/`TOCHeading`）、每一段都被点上样式（`with_style` 9 → 12）、断链 `999` 被**修成** `Normal`（`style_missing` 1 → 0）、写着 9 的那两枚 `outlineLvl` 被**整个丢掉**（`body_written` 2 → 0）、空串那枚被**补成 `0`**（那一段于是从「写了但没值」变成第 1 级）、而 `TOC Heading` 的级也没了 |
| `levels.odt` | LibreOffice（`levels.docx` → .odt 那一转） | ODF 换一种说法：那 7 段有级的成了 `<text:h text:outline-level="N">`（1 两枚、2/3/4/5 各…），**「是标题」写在元素名上而不是任何属性上**，级才在 `@text:outline-level`；其余 5 段是 `text:p`。这一族的 `outline_levels` 只住 OOXML，odt 那一支不交这一格（级由 `headings` 那一本答） |
| `margins.odp` | LibreOffice（`margins.pptx` → .odp 那一转） | 那一族的格是图形对象：四枚 padding 住在 `style:graphic-properties` 上而不是 `table-cell-properties`，六格里两格**连样式名都不点** —— 那一格只交「这一格没说」 |
| `wrap.docx` | python-docx 打底 + 按 ECMA **合成**两枚 `wp:anchor`（`write_wrap_docx`） | 图是怎么摆的三种形状一份里摆开：`wp:inline`（随字走，结构上**没有**环绕那一支）+ `wp:anchor` 两枚各写一种环绕（`wrapSquare` / `wrapTopAndBottom`）；浮着才写的那几格也在（`@behindDoc` `@locked` `@allowOverlap` `@relativeHeight` 与四格 `@dist*` EMU），位置分横竖两条（`positionH/@relativeFrom="margin"` 加 `wp:align`，另一枚写 `wp:positionOffset` 那个数）—— 真件稀缺：本机 33 份 docx 的正文 161 枚 `w:drawing` 里只有 1 枚是 anchor |
| `wrap.odt` | LibreOffice 版式的 ODF（`write_wrap_odt`，三种锚各一枚） | 换一家：框自己只写 `text:anchor-type`（`as-char` / `paragraph` / `page`），环绕、穿透、四个边距全在它点名的那份 `style:family="graphic"` 样式里（`style:wrap="parallel"` / `"through"`、`style:run-through="front"`、`fo:margin-left="0.21cm"`）；「随字」那一枚的样式里**没有** `style:wrap` 这一格 —— 「文件没说」与「说了不环绕」是两句话 |
| `wrap-lo.docx` | LibreOffice（`wrap.odt` → .docx，一趟来回） | 三份框只剩两张图（按页锚那一张**整张丢掉**，`drawings` 3 → 2），留着的那枚 anchor 把层序号从 `251658240` 换成 `3`（同一意思两种写法，两边都按原样交），环绕方式它自己挑了 `wrapSquare`（ODF 那面写的是 `parallel`），而 `wp:positionV` 的孩子叫 `posOffset` 不是 `positionOffset` —— `offset_written` 只认 ECMA 那个名字，于是 false 而 `children` 仍写着那个名字 |
| `styled-text.docx` | python-docx（`write_runs_docx`） | 一段只点一个字符属性（粗 / 斜 / 下划线 / 删除线 / 上标 / 红 `C00000` / 黄 / 9 磅写成 `sz="18"` 半磅 / 宋体），另有点「明确不粗」（`<w:b w:val="0"/>`）、一串字里两个孩子（`<w:b/><w:i/>`）与**一段里三种字各一串**；没格式那几串**不写 `w:rPr`** |
| `styled-text-lo.docx` | LibreOffice（`styled-text.docx` → .docx） | 同一份件重写一次：每一串字都补一个**空的** `<w:rPr></w:rPr>`（30 串里 16 串是空的），而 `w:val="0"` 换成 `w:val="false"` —— 「有没有这一格」与「这一格说不说不」两家正好一边一种 |
| `styled-text.odt` / `styled-text.rtf` | LibreOffice（从 `styled-text.docx` 导出） | 第三种与第四种存法：ODF 把格式搬到 `text:span/@text:style-name="T1"…T12"`，值在**同一份 content.xml** 的 `style:text-properties` 上（`fo:font-weight="bold"`、`style:text-underline-style="solid"`、`style:text-position="super 58%"`、`fo:color="#c00000"`），「明确不粗」成 `fo:font-weight="normal"`；RTF 只在群头写 `b` / `i` / `strike` / `super` / `cf23` / `highlight7` / `fs18` / `af9`，否定是 `b0`，CJK 的下划线落在 `aul` 那个口袋，而颜色与字体只是**一个号**，要跳文件自己那两张表 |
| `charstyles.docx` | python-docx（`write_styles_docx`） | 三段各点一个**字符样式**（`w:rStyle` 在 `w:rPr` 的第一个孩子位上），定义在 `word/styles.xml`：`Strong` 里写 `<w:b/><w:bCs/>`、`Emphasis` 里写 `<w:i/><w:iCs/>`，第二段还**同时**在段上写 `<w:b/>`（一处一半）；第三段点的号是 `SubtleEmphasis` 而名字写着「Subtle Emphasis」（带空格），定义里除了斜体还有 `w:color val="808080" themeColor="text1" themeTint="7F"` |
| `charstyles-lo.docx` | LibreOffice（`charstyles.docx` → .docx） | 重写留着 `w:rStyle` 与那三个号（`Strong` / `Emphasis` / `SubtleEmphasis` 一字未改），照旧给没格式的串补空 rPr（7 串里 4 串是空的）；样式定义自己那份也没动，只有 `w:rsid` 换了大小写 |
| `charstyles.odt` / `charstyles.rtf` | LibreOffice（从 `charstyles.docx` 导出） | 同一件话的另两种存法：ODF 把 `Strong` 换成 `Strong_20_Emphasis`（住 **styles.xml**，带 `style:display-name="Strong Emphasis"` 与父 `Default_20_Paragraph_20_Font`），而段上自己写的粗体变成 content.xml 里的自动样式 `T1` —— 于是那一句被**套成两层 span**（外 `Emphasis` 内 `T1`）；RTF 在群头写 `\cs34`，而 `{\*\cs34 … Strong;}` 那条定义**同时**被它把自己的 `\b` 抄进群头（号与话都在） |
| `sections.docx` | python-docx（`write_sections_docx`） | 两节：第一节点名页眉与页脚（`rId9` / `rId10`），第二节只补一条指着关系表里**不存在**的号 `rId999` 的偶数页页眉，所以它的页眉与页脚两格都是「沿用第一节」；两节都写 `w:titlePg`，settings 写 `w:evenAndOddHeaders`（不带值）。**顺带量到一条生产者脾气**：python-docx 新加的节默认 `linked_to_previous` —— 给第二节写页眉等于改写第一节那份 `word/header1.xml`，全件仍然只有两份页眉页脚部件 |
| `fields.docx` | python-docx（`write_fields_docx`） | 正文里三条域链（SEQ 编号 / DATE 带 `w:dirty` / PAGE），第四条 PAGE 写在 `word/footer1.xml` 里（不进正文那份账）；两个站内跳转：一个指着真书签 `表锚点`，另一个指着 `没这个书签`；书签是 `bookmarkStart` / `bookmarkEnd` 一对（`w:id="3"` 配对，名字只写在 start 上） |
| `fields-lo.docx` / `fields.odt` / `fields.rtf` | LibreOffice（从 `fields.docx` 导出） | 同一批域在三条来回里各变一次样：docx 重写丢了 `w:dirty`、给指令补一个尾空格、把日期格式里的 `-` 转义成 `\-`，并把缓存值换成它自己算出来的数；ODF 把 SEQ 拆成 `text:sequence`（`text:name="表"` / `text:formula="ooow:表+1"` / `style:num-format="1"`）**并往 `text:sequence-decls` 里补一条 `表`**，页码变成页脚样式里的 `text:page-number`，书签只剩名字不再有号；RTF 写成六条 `\field{\*\fldinst …}{\fldrslt …}`，中文序列名成了 `\u-30616\'3f` 一串码位转义 |
| `fields-mix.docx` | python-docx（`write_fields_mix_docx`） | 域那一份账的原件：十种域各点一次，两种写法都写（2 枚 `w:fldSimple` + 13 条 `w:fldChar` 链 = 15 行），三种「不全」各来一枚 —— 没有 `separate` 的那条、缺 `end` 的那条、指令写成空串的那条，于是 `no_separate` / `unclosed` / `empty_instruction` 各是 1 而 `no_instruction` 是 0；另有 3 处域套域（`nested` 3）与一枚 `w:dirty="true"` |
| `fields-mix-lo.docx` / `fields-mix.odt` / `fields-mix.rtf` | LibreOffice（从 `fields-mix.docx` 导出） | 同一批域的三种改写。docx 重写 15 → 13 行：两枚简单式并成复合式、缺 `end` 那条连字带域一起丢、空指令那枚的 `w:instrText` 整个不写（于是 `no_instruction` 1 而 `empty_instruction` 0），`w:dirty` 归零，而 `STYLEREF` 算不出来源时把「错误: 引用源未找到」这一句字当缓存值写进正文。odt 只剩 11 行，而且种类改写在**元素名**上（`REF` 与 `PAGEREF` 塌成同一枚 `text:bookmark-ref`，只靠 `text:reference-format` 的 `number` / `page` 分开），另多一本只有这家有的 `text:sequence-decl`（6 条序号类型声明，用没用到都写）。rtf 14 群，`control_words` 与行数同为 14，那枚空指令在这里是「群里没有指令」（`no_instruction` 1），而缓存值有 1 条是空串 |
| `cell-links.xlsx` | openpyxl（`write_cell_links_xlsx`） | 一格只改一个变量：站外 http（`A1`）、`mailto:` 且 subject 用百分号写法（`B2`）、只有 `location` 的站内跳转（`C3`，没有第二跳）、`=HYPERLINK()` 公式（`D4`，它不写链接对象）、带悬浮提示的（`E5`，唯一一家写 tooltip）、关系号被删掉的（`F6`，格上留着 `r:id` 而那张关系表里没有它）、没有字的一格挂着一条链接（`G7`）；`数据` 表另给一条回跳 |
| `cell-links-lo.xlsx` | LibreOffice（`cell-links.xlsx` → .xlsx） | 同一批字的第二种写法：`F6` 整个丢了（6 → 5），每条补一个 `display`（`G7` 那个就是地址本身），tooltip 一个也不写，`mailto` 的 subject 从 `%E9%A2%84%E7%AE%97` 解回「预算」，关系号从 `rId1` 起重新编 |
| `cell-links.ods` | LibreOffice（`cell-links.xlsx` → .ods） | 第三种存法：地址挂在段的字上（`text:a/@xlink:href`，一条 `table:hyperlink` 也不写），站内跳转变成 `#'数据'.A1`（点号不是感叹号），`mailto` 的百分号写法又回来了，而 `G7` 那条地址被写成格子的字 |
| `cell-links.xls` | LibreOffice（`cell-links.xlsx` → .xls） | 第四种：同一条 Workbook 流里的一条 0x01B8 记录（显示字、地址、行列范围与两个 GUID 并排写），外部支与站内支的长度字段一个数字节、一个数码元；0x01B7 自报的条数是 **0** 而流里有 6 条 0x01B8 |
| `sheet-pictures.xlsx` | openpyxl 3.1.5（`write_sheet_pictures_xlsx`） | 同一批图一次只改一个变量地摆在四张表上：三种锚元素各写一种摆法（跨格的 `twoCellAnchor` 只写 `from`+`to`、`oneCellAnchor` 写 `from`+`ext`、`absoluteAnchor` 写 `pos`+`ext` 而**连 `from` 都没有**），三条各缺一块；第五条的关系被整条删掉（格上 `r:embed` 那个号还在、图部件 `xl/media/image5.png` 也还在包里，只是那个号指不到任何东西 → 地址与字节都交 null）；每条 `cNvPr` 都带 `descr="Picture"` 那句占位的话，只有一条被改成真名字与真描述（`第二张` / `一个蓝点`）；藏起来的那张表照挂一张，第四张一个不挂 —— 每张表有自己那一份画法账 |
| `sheet-pictures-lo.xlsx` | LibreOffice（`sheet-pictures.xlsx` → .xlsx） | 同样那批图的第二种写法：锚元素**全变成 `twoCellAnchor`**，三种摆法的区别搬到锚块的 `editAs` 上（`twoCell` / `oneCell` / `absolute`，两家一个用元素名、一个用属性名，正好互补），`to` 那四个 EMU 换成另一组数（同一个「跨三格」openpyxl 写 95250、这一家写 95040），每个 `pic` 多出一份 `spPr/xfrm`（`off` + `ext`），blip 上不再写 `cstate`；那条指不到的关系整个删了（5 → 4），而 117 字节那张被两个锚块指着 —— 七份媒体部件并成三份，「几个锚块」与「几个不同的图部件」是两个数（4 / 3） |
| `sheet-pictures.ods` | LibreOffice（`sheet-pictures.xlsx` → .ods） | ODF 只有一跳：`draw:frame` 上直接挂 `draw:image/@xlink:href`，所以 `drawings` 这一格交 null（这一族没有部件那层可数）。摆位是 `draw:x` / `y` / `width` / `height` 四个厘米串，住在格子里的那几条另有 `table:end-cell-address`（`图与格.I9`）与 `table:end-x` / `end-y`；七张源图并成三个 `Pictures/` 部件（同一个 png 被两条 frame 指着），而其中一条 frame 压根没写 `draw:image`（`image_written` 是空表）—— 不是坏掉的地址，是根本没有地址 |
| `sheet-pictures.xls` | LibreOffice（`sheet-pictures.xlsx` → .xls） | 第四族只能两格：形状按表数（0x005D 里偏移 4 的类型 8 = 图片，实测 5 / 1 / 1 / 0），0x00EC **没有图的那张表也写了一条**，而图的字节一条都不按表分 —— 三条 BLIP（OfficeArt 0xF007）全住在整本共用的那条 0x00EB（偏移 1054、正文 1217 字节）里：自报长度 178 / 171 / 722，字签都落在正文第 61 个字节上，从字签到正文末尾正好 117 / 110 / 661，与当初那三张源图的字节数一字不差 |
| `dir-cell.docx` | python-docx（`write_direction_cell_docx`） | 五处各点一次：一行六格里前五格各写一个 `w:textDirection` 枚举（第六格什么都不写）、第二张表的表身写**空的** `w:bidiVisual`、一段写 `w:bidi w:val="1"`、一个 run 写 `w:rtl w:val="1"`，而节上什么都不写 —— 「几格点了」与「哪张表说了」是两本账 |
| `dir-cell-lo.docx` | LibreOffice（`dir-cell.docx` → .docx） | 同一份重写后：两个「正向」枚举整个丢了（`lrTb`、`lrTbV`）、`tbRlV` 降级成 `tbRl`（于是 `tbRl` 那枚数是 2 而不是 1），`w:bidiVisual` 补上 `w:val="true"`，节上多一枚 `w:textDirection w:val="lrTb"`，段上那句从 2 段摊到 11 段（其余各补一句 `w:val="0"`）—— 见事实 128 |
| `dir-sect.docx` | python-docx（`write_direction_sect_docx`） | 只在节上点一次 `w:bidi w:val="1"`：格、表身、段、run 四处一个字不写（`cells_written` 0、`paragraphs_written` 0）—— 「一处说了」不等于「别处也跟着说」 |
| `dir-cell.odt` | LibreOffice（`dir-cell.docx` → .odt） | 唯一一种「不写在身上」的存法：十格全靠 `表格1.B1` 这类地址式自动样式（8 格有值），而 `bt-lr` 那一格走 LibreOffice 扩展词法 `loext:writing-mode` —— 局部名与 `style:writing-mode` 一模一样；枚举多出 OOXML 没有的那一枚 `page` |
| `dir-sect.odt` | LibreOffice（`dir-sect.docx` → .odt） | OOXML 写在**节**上的 `w:bidi` 在这里变成**页面版式**：`Mpm1` 的 `style:page-layout-properties` 写着 `rl-tb`，而它的父元素是 `style:page-layout`（与另外三处的 `style:style` 不同名）|
| `dir-cell.rtf` | LibreOffice（`dir-cell.docx` → .rtf） | 同一件事在 RTF 是六个控制字：`\cltxtbrl` ×2、`\cltxbtlr` ×1、`\rtlrow` ×2（表上那句落到每一行）、`\rtlpar` ×1、`\ltrpar` ×27（默认值被逐段重发）、`\rtlcol` 0。**这一支不读**，留作「缺键不是猜一个数」的凭据 —— 见事实 128 |
| `merges.xlsx` | openpyxl 3.1（`write_merges_xlsx`） | 合并区间的六种形状一次给全：1×4、3×1、3×2、**没有冒号的单格 `ref="A12"`**、两条互相盖住的、同一句 `merge_cells` 调两遍 —— 而**写手自己去重**（件里 5 条、`count` 也是 5）；第二张表一条也没并、第三张表只有块而**锚点格是空的** |
| `merges-lo.xlsx` | LibreOffice（`merges.xlsx` → .xlsx） | 同一份的重写：单格那一条与重叠里较小那一条**一起丢掉**，`count` 跟着改成 3，于是 3 条（`A1:D1` / `A3:C5` / `B7:C9`）、重叠归零 |
| `merges.ods` | LibreOffice（`merges.xlsx` → .ods） |
| `images-dpi.docx` | python-docx（`write_picture_dpi_docx`） | 九张图、五种格式、三种尺寸来历：按原尺寸摆一张、只给宽一张、硬拉成 4cm×1cm 一张，另五张都按 Cm(3) 摆。字节里三种密度来历：PNG 带 pHYs（300 与 72 DPI 各一张）、JPEG 带 JFIF 密度（unit=1 与 unit=0 各一张）、TIFF 用 RATIONAL |
| `images-dpi.odt` | LibreOffice（`images-dpi.docx` → .odt） | 同一份稿子换成第二家：九个框只留六份字节（两张 40×24 的 PNG 按像素数并成一份、32×16 的 GIF 把同尺寸的 TIFF 吸走），尺寸写成 `svg:width` 的 `cm` |
| `images-dpi-lo.docx` | LibreOffice（`images-dpi.odt` → .docx） | 同一条来回的第二副 OOXML：部件后缀 `.jpg` 被改写成 `.jpeg`，被并掉的那两份至今共用一个部件名 —— 于是拉伸的那一张与另一张同名 |
| `images-dpi.rtf` | LibreOffice（`images-dpi.docx` → .rtf） | 第三族：尺寸拆成 `\picwgoal` × `\picscalex` 两半，另写 `\picw` / `\pich` 声明像素；GIF 与 BMP 被重编码成 PNG、两张 TIFF 变成 WMF | 第三种拼法：没有区间串，也没有 count —— 跨度写在格子自己身上（`table:number-columns-spanned` / `number-rows-spanned`），区间从锚点加出来 |

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
      实测那些样式里写着底色与垂直对齐（`ce2` `#d0d8e7` + bottom、`ce4` `#e9ecf3` + top、
      `ce5` `#ffff00` + top —— 那个黄就是 pptx 那面 `a:tcPr` 里的同一个填充）——
      但摆法又是第三种：底色与对齐住在 **`loext:graphic-properties`**（LibreOffice 自己的
      实验命名空间，不是 `style:`），`fo:padding-*` 四条也在那里，而那份样式里的边
      （`fo:border="0.48pt solid #ffffff"`）挂在 **`style:paragraph-properties`** 上 ——
      odt 表格用的 `style:table-cell-properties` 这一族**一个都没有**（9 份 table 家族样式里
      2 份列样式、2 份行样式各带自己的 properties，5 份格子样式各带 graphic + paragraph）。
      五份格子样式**全在 content.xml**，`styles.xml` 里一份 family=table-cell 都没有；
      占位格点的 `standard` 更是 **family=graphic** 的另一个东西（占位格不进账本，数不到它）。
      两份都读不是因为有件需要第二份，而是只读一份就等于替文件定规矩。
      这一跳在文档那一族量过、在演示稿这一族还没量准，所以只交名字、不猜值。

62. **PDF 的表单可以把类型只写在祖父上，而 `/Opt` 按规范只有数组一种写法**（`forms-hier.pdf`）。
    这一份**不是任何编辑器导的**：量过的几个生产者（Word 2013、LibreOffice、手搓的 `risk.pdf`）
    都没在父字段上写过 `/FT` 或 `/Ff`，继承那几条分支因此一直停在「数过了，没有」。
    现在由 pikepdf 把形状挂出来，让分支真的走一遍；第三个读者 pypdf 独立数过。
    * 形状：`/Fields` 上七条根、连子字段十二条、最深第三层
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
    * `/V` 是**四件事**，不是一件：`Ghost` 写 `/V ()`（空串：`value_shape` string、值就是 ""）、
      `Flags` 写成数组（多选列表框，`/Ff` 第 22 位 = 524288 → `value_shape` array、`value` 仍 null
      而那两段在 `value_parts` 里 = `["甲","丙"]`，几段不并成一个串）、
      `Agreed` 写成一个**名字** `/V /Yes`（勾选框与单选全是这么写的 → `value_shape` other、
      `value` null、那一个名字在 `value_name` 里）、`Person` 整个没写（`value_shape` null）。
      三个键 `value_shape` / `value_parts` / `value_name` 就是为了这四件事各有名字，
      挤成一个 `value` 就得替文件选一种说法。
    * **勾选框「打没打上」住三处，一处都不合成布尔**：`Agreed` 三处都写（`/V /Yes` +
      控件 `/AS /Yes` + `/AP` 里 `/N` 的键 `Off`/`Yes`）；`Extra` 故意**不写 `/V`** ——
      文件只说控件现在是 Off，于是 `value_present` false 而 `as_state` "Off"；
      单选 `Pick` 把 `/V /One` 写在父上，两个孩子各是一个控件、各写自己的 `/AS`
      （`One` 与 `Two`），其中 `Two` 与父上的值对不上 —— **那份不一致照交**，不替它挑一个。
      可显示的状态从 `/AP` 的 `/N` 字典的键读来（`Pick` 第二个孩子没带 `/AP` → 空表：
      那是「没说是哪几种」，不是「一种都没有」）。
    * 六条控件（`First`、`City`、两条勾选框、单选的那两个）**同时挂在页的 `/Annots` 上**
      （那一页八个注记：六条 Widget 加两条链接），字段树只从 `/Fields` 走，所以是十二条
      不是十八条 —— 这是那条规则第一次有件可走。
    * 文档级那三个开关也第一次有了非 null 的样本：`/NeedAppearances true`、`/SigFlags 1`、
      `/DA (/Helv 0 Tf 0 g )` —— 有 AcroForm 的另一份（`risk.pdf`）三个都没写，
      其余五份连 AcroForm 都没有。
    * 字符串全带 `FE FF`：没有 BOM 的 `/V (李)` 会被两个读者都按 PDFDocEncoding 读成两个
      拉丁字母 —— 写的时候就得按规范写。

63. **一页上的链接，两家放的地方差一跳**（`deck-links.pptx`、`-lo.pptx`、`.odp`）。
    * OOXML 要跳两跳：那个 run 的 `a:rPr` 里只写 `a:hlinkClick/@r:id` 一个号，
      地址与 `TargetMode="External"` 住在**这一页自己的关系表**里（与图、与表对象同一类链接法）。
      `TargetMode` 没写时交 null，不替它当成站内；号指不到东西时 `target` 与 `external` 都交 null，
      而那个号照交 —— 「写了个指不到东西的号」是文件自己说的话。
    * ODF 一跳就够：地址直接挂在字上（`text:a/@xlink:href` + `xlink:type="simple"`），
      所以 `hop` 是 `inline`，而 `external` 与 `id` 都是 null —— 这一族没有那个开关，
      填 false 就是替文件编一个它没做的判断（`external` 那本合计因此是 0，不是「三条都不站外」）。
    * **号是生产者自己排的**：python-pptx 从 `rId2` 起三条，LibreOffice 重写同一份排成 `rId1/2/3`，
      而三个地址一字未变 —— 与图的轴 id、条件格式的 priority 同一件事，只交出来不拿来比。
    * 踩到的一条：只走 `draw:frame` 会把这三条链全读成 0 —— python-pptx 那个文本框被 Impress
      改写成了 `draw:custom-shape`（那一页 `frames` 只有 2 个，而链有 3 条）。所以现在走
      「页上除 `presentation:notes` 以外的那几块」：备注里的链不算页面上的链，
      而 OOXML 那边备注住在另一个部件里，本来就不会混。
    * `scheme` 只说地址自己写的那一截：`https`、`mailto`；没有冒号、冒号前是空的
      （`#那一页` 这种站内跳法）、或者**只有一个字母**（那是 Windows 的盘符不是 scheme）都交 null。
    * 第二页一条也不链：那一份交 `total: 0`，不是缺这个键。

64. **关系表的成员名中间那一段 `_rels/` 不是可选的**（`deck.pptx`、`deck-links.pptx`）。
    * OPC 把「这一页指着什么」写在 `ppt/slides/_rels/slide1.xml.rels`，而 `slide1.xml` 只是
      宿主部件的名字。`zipread::member` 按名字精确匹配、不会替你猜，所以把成员拼成
      `ppt/slides/slide1.xml.rels` 永远读不到 —— 于是每页的 `relationships` 一直是**空表**：
      键在、形状对、值也自洽，只有跟第二读者逐字段对一遍才露出来（那条链的 `links` 之所以
      对，是因为图与备注各走各的读法，两边都没用到这一处）。
    * 同一次修复里踩到第二条：`a:hlinkClick` 上那个号是 `r:id`，按 `attr("id")` 精确匹配读不到，
      三条链于是全成了「指不到」—— 这与本页另一处 `<p:sldId id="256" r:id="rId2"/>` 是同一个坑，
      区别只在那次两个 `id` 都在、拿到的是放映序号，这次一个都不在、拿到的是空串。
    * 修好后 `deck.pptx` 第一页交三条（版式 / 备注页 / 图，内部的 `Target` 已解成包内全名），
      `deck-links.pptx` 第一页交四条（那三条链本来就是这页关系表里的三条，外部的 `Target`
      不按包内解，照原样）—— 两本的账由 `office_reader.py` 的 `slide_rels()` 各读一遍核对。

65. **一格算出来的结果不是数，三家各摆各的**（`errors.xlsx`、`errors-lo.xlsx`、`errors.ods`）。
    * OOXML 的错误格自己就写着显示那一串：`<c r="B1" t="e"><f>1/0</f><v>#DIV/0!</v></c>`。
      这一支此前两份读者**一起**在它前面加了一句 `#错误 `（`#错误 #DIV/0!`）—— 那串字不在任何文件里。
      两边一起错而谁也没撞上，原因很实在：手上一直没有一个会重算的生产者，`t="e"` 这个分支
      从来没被走到。现在有了（`errors-lo.xlsx` 是 LibreOffice 重算过的那一份），
      与 LibreOffice 自己的 CSV 导出逐格对过：它交的也是 `#DIV/0!`，一字不加。
    * `t="str"` 是第二种「结果不是数」：公式算出来的那句字（`"甲"&"乙"` → `甲乙`），
      LO 不走共享字符串表，那一句直接住在 `<v>` 里。`t="b"` 是第三种：文件写 `1`，显示 `TRUE`。
    * 「文件写了 `t`」与「`t` 没写、按规范默认 n」分两键（`kind_written` / `cells_with_written_type`）：
      openpyxl 六个公式格一个都不写（`<c r="B1"><f>1/0</f><v></v></c>`），LibreOffice 重写同一批格子
      九格全写、连数字格也写 `t="n"`。只交 `kind` 就会把一家的沉默读成另一家的表态。
    * 同一份格子换一家生产者，连「几条公式」都不同：openpyxl 6 条，LO 7 条 ——
      它把那个布尔常量 `D1` 写成了 `<f>TRUE()</f><v>1</v>`，一条常量成了一条公式。
    * ODF 是第三种摆法：错误格写 `office:value-type="string"`（**不是 error**）、
      `office:string-value=""`（空的），显示的那串只在 `<text:p>` 里；LibreOffice 另写了一条
      `calcext:value-type="error"`，这一族不跟（与所有 .ods 里那些 calcext 副本同一个口径）。
    * 最要紧的一条跨家对照：同一个坏掉的 `VLOOKUP`，LO 的 xlsx 导出缓存成 `#VALUE!`，
      它的 ods 导出写成 **`错误:502`**。同一个错误在三副件里是三个名字，各按各的文件交，
      不替它们对上一个 —— 而 `--csv` 交的是文件里缓存的那一串，不是读者重算的结果。

66. **序列数与日期之间没有唯一的换算：`date1904` 那件真来了**（`epoch.xlsx`、`epoch-lo.xlsx`）。
    * 全套断言里从来没有一份件写过 `date1904`，而 `serial_to_iso` 里那一支 `days - 24_107`
      与「60 号那一天的闰年 bug 只属于 1900 基准」两个特例一直在等人验（与上面 `t="e"` 同一件事：
      **没被走到过的分支不算读过**）。openpyxl 把 `wb.epoch` 换成 1904 就写出这个开关。
    * 同一个序列数换一套基准差 1462 天：`40169` 在 1904 基准下是 **2013-12-23**，
      在 1900 基准下它是另一天。第三方见证是 LibreOffice 自己 `--convert-to csv` 的渲染：
      `2013-12-23,1,2020-01-02 03:04:05,1904-03-01,12/23/2013`。
    * `D1` 那一格是这一件存在的另一半理由：它的序列号正是 **60** —— 1900 基准里那个号
      对应 Excel 闰年 bug 造出来的、根本不存在的 1900-02-29（读取器按文件原样报那一串），
      而在 1904 基准里它是好好的一天 **1904-03-01**。那个特例必须只在一边生效，
      两边都数一遍才算验过。
    * 两家写这个开关的拼法又是那两种：openpyxl `date1904="1"`，LibreOffice `date1904="true"`；
      认出来的基准相同，序列数一字未变（`40169` 两边都是 `40169`）。带时刻的那一格
      LO 只留了 10 位小数（`42370.1278356482` 对 `42370.12783564815`），
      而两边换算出来的都是 `2020-01-02T03:04:05` —— 差在第 11 位小数上，不到一秒。
    * 顺手撞见的同族事实：那格长得像日期的**字**（`12/23/2013`）在 openpyxl 手里是
      `t="inlineStr"`（`<is><t>…`），LibreOffice 重写时换成 `t="s"` 指共享字符串 ——
      同一条字两种存法，两边都不许换算成日子。

67. **一条批注在 PDF 里是两条注记**（`pdf-comments.pdf`，另见不带开关的那份 `notes.pdf`）。
    * 生产者这一侧先撞上一条：LibreOffice 的 headless `--convert-to pdf` **把 docx 的批注
      整个丢掉** —— 上面那份 `notes.pdf` 里一条 `/Text` 都没有，只剩一条链接。要显式给
      filter 选项 `pdf:writer_pdf_Export:{"ExportAnnotations":{"type":"boolean","value":"true"}}`
      才带得出来。所以这一族有两份件：带着批注的那一份，和「默认那一转」这一份 ——
      后者让 `notes: 0` 有了一件真文件可指，不是空谈。
    * 一条批注落地是**两条注记**：`/Subtype /Text` 那一条（11 号）自己指着 `/Popup`（12 号），
      而那条 `/Popup` **也在同一页的 `/Annots` 数组里**、反过来用 `/Parent` 指回 11 号，
      它的 `/Rect` 更是摆在页面外（`-14.2, 1612.35`）。于是「几条注记」（3）与
      「几条批注」（1）是两个数，谁也不替谁圆场。
    * 作者与时间在 PDF 里**是一条串**：`/T = "liuqi, 09/23/26, "`（第三个位子空着，
      后面那两句是逗号加空格）—— 与 docx 那边 `w:author` / `w:date` 两个属性是两回事，
      所以这里只交 `/T` 那一串，不替它拆成两个字段。
    * `/M`（修改时间）LibreOffice 写的是 **`D:00000000000000Z`** —— 一串全零。
      读取器把这一串原样交出去：既不替它解成某个日期，也不因为它解不出来就报 null。
      （`D:` 前缀与 `Z` 后缀都留着：那是文件自己写的。）
    * 注记与链接走的是同一条路：`/Annots` 可以是内联数组也可以是间接引用，
      两份读者都两种认（这一件里三条都是内联的）。

68. **一页藏不藏，三家写在三处地方**（`deck-hidden.pptx`、`deck-hidden-lo.pptx`、`.odp`）。
    * OOXML 就一个属性：`<p:sld … show="0">`（PowerPoint 界面里那句「隐藏幻灯片」）。
      python-pptx 没有这个开关，但包是它写的 —— 这里只把 UI 会设的那一个属性设上。
      LibreOffice 把这份转成 odp 再转回 pptx，`show="0"` 一字不动地留着（两副都收进仓库）。
    * ODF 不写在页上：`draw:page` 只点名一份 `family="drawing-page"` 的自动样式，
      那句话在那份样式的 `style:drawing-page-properties/@presentation:visibility="hidden"` 里。
      实测这两页在 `draw:page` 上的属性表**只差 `draw:style-name` 一个值**（dp1 / dp3），
      所以「这一页藏不藏」要跳一跳才知道。
    * 这一件专门备着的坑：同一份 `content.xml` 里 **dp2 与 dp3 两份样式都写着 hidden**，
      而没有任何一页点 dp2 的名 —— 只 grep 全文会把看得见的那页也判成藏的。所以读法是按
      页自己点的名去找（两份件都找，`content.xml` 与 `styles.xml`，不赌自动样式在哪一份），
      找到之后 `visibility_written` 交它自己写的那一串（dp1 那份**根本没有这个属性** → null，
      `hidden` 因此是 false 而不是 null：ODF 的默认就是 visible），
      点名点不到那份样式时 `hidden` 交 null（判不住），`style_found: false` 说清为什么。
    * 三家都有 `hidden` 这一键了：没藏就是 false，不是缺键；藏起来那页的标题、字与
      段落照旧整份交出来（「放映时不演」不等于「这份文件里没有这一页」）。

69. **一个格子的字可以分成几段，而首尾那两个空格是文件写的**（`rich.xlsx`、`-lo.xlsx`、`.ods`）。
    * 两条教训合成一件。**其一**：`<t xml:space="preserve">  两头有空格  </t>` 里那两个空格
      此前被两份读者一起 `trim` 掉了 —— 又一个「两个读者一起错」的例子（Rust 那边 `text().trim()`，
      Python 那边 `.strip()`，而字符串表那一条路 Python **没有** trim，所以两份读者在同一家文件上
      本来就不一致，只是没人把这一格摆进对账）。凭据是生产者自己：LibreOffice 把这份 xlsx 与那份
      .ods 各自导成 CSV，交回来的都是 `'  两头有空格  ,'`（连开头那个制表符也一样在）。
    * **其二**：分段（富文本）这一支从来没有生产者。openpyxl 3.1 起能写 `CellRichText`，
      但它**只写行内串**（`t="inlineStr"` + `<is><r><rPr>…</rPr><t>…</t></r>`，整个文件一条
      `sharedStrings.xml` 都没有），LibreOffice 重写同一份时把八次引用全搬进字符串表，
      并在 `sst` 上自报 `count="8" uniqueCount="7"` —— 那条「甲」被两个格子用了，
      所以两个数都是对的，谁也不替谁圆（`sst_unique_matches` 说的是自报数与条数对不对得上）。
    * 格式住在 `rPr` 的**孩子元素**上（`<b val="1"/>`、`<color rgb="FFC00000"/>`），不在 `rPr`
      自己的属性上（实测两家一个属性都不写）；同一个开关又是两种拼法：openpyxl 写 `val="1"`，
      LibreOffice 重写同一截字写 `val="true"`。还有一处「没写」与「写了但是空的」的分别：
      同一段稿子，openpyxl 的第一段整个没有 `rPr`（`props_written: false`、`format: null`），
      而 LibreOffice 给每一段都补了一份。
    * 分段不改字：`重要` + `普通` 两段的整串仍是 `重要普通`。LibreOffice 还会**按字体 fallback
      切段** —— 那一格 `\ttab 开头` 在 openpyxl 手里是一段，在它手里是两段（`Calibri` 与
      `Noto Sans SC` 各一段），所以 `run_total` 也是生产者自己的决定，只交不比。
    * `.ods` 是第三种写法，而且是**记号不是字面**：`  两头有空格  ` 写作
      `<text:s text:c="2"/>两头有空格<text:s text:c="2"/>`（`text:c` 说那一个记号顶几个空格），
      制表符是 `<text:tab/>`，段内换行是 `<text:line-break/>`。不展开就一个空格也读不出来。
      富文本在 ODF 里是 `<text:span text:style-name="T1">` —— 这一族**不追那份字符样式**，
      只交每一格有几个 span、几个记号（`spans` / `specials`），因为那是另一条没有量过的跳。
      另有一格把粗体写在**格子上**（`s=` → cellXfs → font）而不是串里，两处的账各交各的。
    * 踩到的一条（第二读者自己的）：ElementTree 的 `Element` **没有孩子时是假值**，
      所以 `local_child(r, "t") or r` 会带着空字退回 `r` —— `text` 明明写着「重要」而读出来是 `""`。
      这类 `or` 兜底在 Element/None 之间要用 `is None` 判。

70. **「这格是粗体吗」要再跳一跳，而两家的「没说」不是同一个东西**（`styled.xlsx`、`styled-lo.xlsx`）。
    * 格子上只有一个 `s="7"`：它是 `cellXfs` 的下标，而那条 `xf` 自己只写三个号
      （`fontId` / `fillId` / `borderId`）与一串 `apply*` 旗标，字面住在 `fonts` / `fills` /
      `borders` 三张表里。四张表自报的 `count` 与实际条数一起交（`workbook.styles`）。
    * **第 0 条不是「没有」而是「占位」**：`fills[0]` 在 openpyxl 手里是一个**空的**
      `<patternFill/>`（元素在而没写 `patternType`），在 LibreOffice 手里写明
      `patternType="none"`；`fills[1]` 两家都写 `gray125`。所以「这格有没有底色」交
      `style_filled`，而它为什么是 false 在 `style_fill` 那份原样账里看得见。
    * **颜色还要再下一层**：`D2` 那条底色的字面住在 `fill > patternFill > fgColor` 的第三层，
      而第一版的行账本只走到 `patternFill` 自己身上 —— 于是「这格什么颜色」在文件里明明写着
      而交回 null。这一条最难看的部分是它**藏得住**：`patternType` 在第一层，读得到，
      `style_filled` 也算得对，整份账看上去是通的，只有把颜色串按文件钉死那一条检查会露出来。
      现在一行往下带两层，两份读者同一条规则（实测 `FF00B050` 与重写后的 `FF90DDB3`
      各在自己那一层，`bgColor` 也在）。
    * 三种开关是三种形状，不混：粗体是**孩子元素**（`<b val="1"/>` / `<b val="true"/>`，
      元素在而没写 `val` 按 true 算 —— 那是 OOXML 的写法），换行是**属性**
      （`<alignment wrapText="1"/>`，属性没写就是「文件没说」→ null，**不按默认 false 算**），
      底色是 `@patternType` 的值。三种拼法（`1` / `true` / 缺省）各按各的文件交。
    * 「没写」与「写了关」是有数的：openpyxl 只给那一格写 `applyAlignment="1"`，`A3` 连 `s`
      都不写（`style_written: false` 而 `style_found: true` —— 按默认查第 0 条查得到）；
      LibreOffice 每格都写 `s`、每格都补一份写着 `wrapText="false"` 的 `alignment`，
      于是同一批格子里「说了 false」的格数一个 0 一个 9，而 `cells_wrapped` 反倒都是 1。
    * **重写不是无损的，这一条在长相上看得最清楚**：`indexed="64"` 那个颜色回来成了
      `rgb="FF000000"`；点状网格底 `lightGrid` + `FF00B050` 被换成 `solid` + `FF90DDB3`
      （连 `bgColor` 也换了）；`cellStyleXfs` 从 1 条变 20 条，字体从 5 份变 9 份。
      谁也不替谁圆，两副都收进仓库。
    * `.ods` 的长相在它点名的那份单元格样式里（`ceN` → `style:table-cell-properties`），
      与 xlsx 这三张表没有对应关系；`.xls` 的在 BIFF 的 `XF` 记录里（数字格式那一条已走过）。
      两家都不交这些键 —— 键整个不在，不是 0、也不是 null。

71. **文档里那张图把同一件话说在三处，而三家各处不一样**（`images.docx`、`images-lo.docx`、
    `images-float.docx`、`images.odt`、`images-float.odt`、`images.rtf`）。
    * 尺寸有**两处**：`wp:extent`（画法那层）与 `pic:spPr/a:xfrm/a:ext`（图自己那层）。
      两家恰好都写了两处而**写的不是同一个数**：python-docx 两处都是 `1440000`（4cm → 4000），
      LibreOffice 两处都是 `1440180`（4001）。所以「这张图多大」在这一族不是一个数，
      两处都交、不替它们挑一个（`864235` 那一个是 2401 也是同一件事）。
    * 替代文字有**两处**（`wp:docPr` 与 `pic:cNvPr`）：一家只在外面那处写 `descr`，
      里面那处 `name` 写的还是**原始文件名** `dot.png`；LibreOffice 重写时把名字与那句
      替代文字**一起抄进了里面那处**。无障碍检查问的是「有没有」，所以两处各交一份，
      另给 `descr_written` 把「空的 `descr=""`」与「整个没写」分开。
    * 锁也有**两处**（`a:graphicFrameLocks` 与 `a:picLocks`）：一家只写外面那一份，
      重写那份两份都写。合并成「锁了纵横比」就把生产者的习惯读成了文档的说法。
    * 号是生产者自己排的：同一个部件（`word/media/image1.png`）在一家是 `rId9`、
      在另一家是 `rId2`。所以号照交、解出来的部件也照交，两个一起才说得清这件事。
      顺带一条踩过的坑：号只能在**这份件自己的**关系表里查 —— 同一个包里
      `word/header1.xml.rels` 也敢再来一条 `rId2`，全包查就会串台。
    * **`wp:anchor` 那一种（浮在页上、文字绕着排）手上没有一个生产者会自己写**：
      python-docx 只写 inline，LibreOffice 插入默认也是 inline。所以那份件是把
      `images.odt` 的锚点改成 `page` 之后由 LibreOffice 导出的（输出全是它写的），
      于是那条分支第一次有了真件可走：摆法写在**元素名**上，绕排是
      `wp:wrapSquare wrapText="largest"`（名字与属性一处一半），摆放更怪 ——
      `relativeFrom` 在属性上而值在**孩子的文字里**（`<wp:align>center` 是一个词、
      `<wp:posOffset>635` 是一个 EMU 数），所以 `position_h` / `position_v` 各交三份：
      属性、元素名、那个元素写的字。
    * ODF 换了一套地方：尺寸是**自带单位的串**（`svg:width="4.001cm"`，与那份 OOXML 的
      `1440180` 换算到 0.01mm 都是 4001 —— 两家读者用同一条整数式子），摆法在**属性**
      `text:anchor-type` 上，地址直接在 `draw:image/@xlink:href`（没有关系表这一层），
      而替代文字从属性搬成了**孩子元素** `svg:desc`（所以「写没写这个元素」要另问一句
      `alt_written`，而元素在而里面是空的是另一种情形）。
    * 最出人意料的一条：**同一个选择在 ODF 里有两个词**。`images.odt` 写 `as-char`，
      而 `images-float.odt`（那份 anchor 的 docx 转回 ODF）写 `char`。这两个串不是
      同一个东西的两种拼法，是来回一趟之后 LO 自己改的口径 —— 照文件各交各的，
      折成一个词就是替文件说话。同一趟来回还把 OOXML 那侧的 `wp:anchor` 与绕排整个丢了
      （重写不是无损的，这里正看得见）。
    * RTF 是第三种存法而**逐张也读**：`{\pict …}` 那一格里「多大」一次写在**三种单位**上
      （`\picw40 \pich24` 像素、`\picwgoal480 \pichgoal288` twips、`\picscalex472` 百分比），
      而文件里没有一个字写 DPI —— 所以像素那两个不换算法，只把 twips 那一对照「那张纸」
      同一条整数式子换成 0.01mm（`480` → `847`），把三者乘回去是推算，不交。
      凭此正看得见一趟转换做了什么：同一批字的 docx 写的是 `1440000` EMU（4000），
      而 LibreOffice 的 RTF 导出把它拆成了「目标 847 × 缩放 472%」。
    * RTF 那一族「这是什么格式的图」有**两份凭据**：数据前那个控制字（`\pngblip`）与
      那串十六进制自己带的前八个字节（`89504e470d0a1a0a` → png）。两个都交，
      `sig_agrees` 只在两边都说得出同一个词时才比（`\dibitmap` 没有词干 → null，
      不替它编一个「不一致」）。数据是折行写的，所以读字节那一段跳空白、碰上第一个
      既非十六进制又非空白的字符才停；只看群头 16KB（两个生产者都把形状写在数据之前）。
    * RTF 的替代文字又搬了一次家：住在 `{\*\picprop}` 那张形状属性表里的一对群
      （`{\sn wzDescription}` 给名字、`{\sv …}` 给值）。最要紧的是这一族给了
      **「写了而值是空的」**那一格：`notes.rtf` 与 `toc.rtf` 那枚模板小图两条
      `wzDescription` / `wzName` 都在而值都是空串 —— 于是那里 `props_written` 与
      `alt_written` 都是 true、`alt` 是 `""`，而 `pictures_with_alt_text` 是 0。
      说了，与说了句空话，是两件事；docx 那边对应的键是 `descr_written`（属性在不在），
      三家各处不一样。同一句「一个红点」在三家写在三个地方（`wp:docPr/@descr`、
      `svg:desc` 的字、`wzDescription` 的值），而字一字不差 —— 三处都读，谁也不替谁圆场。
    * 「两种摆法都不是」的那种 `w:drawing` 也不造一条占位记录：这一族另有 `w:pict`，
      手上没有件可量。缺口由 `structure.drawings` 与 `structure.pictures` 的差自己说。

72. **「这页的图有替代文字吗」在 pptx 只有一处可问，而那一处可以写着文件名**（`deck-pictures.pptx`、`deck-pictures.odp`、`deck-pictures-lo.pptx`）。
    * 与文档那一族相反：pptx 一页一张图只写**一处**尺寸（`p:spPr/a:xfrm/a:ext`），alt 也只有一处
      （`p:cNvPr/@descr`）。少了「两处不一样」的余地，也少了一处可以躲的地方。
    * 坑在第二页：调用者**没有**给替代文字，python-pptx 于是把源文件名写进 `@descr`
      （`descr="dot.png"`）。所以「有几张图有 alt」这一问在文件上答案是 1，而那句根本不是描述。
      这本账的三个键各说一件事 —— `descr` 照写、`descr_written` 只说属性在不在、
      `pictures_with_alt_text` 只数非空串；把「是不是人在描述」交给读的人，就是替文件下结论。
    * 不写宽高那一张：python-pptx 用 72 DPI 把 40px 换成 `508000` EMU，**文件里没有一个字写这个假设**。
      所以交数不交解释（`mm_w` 1411 是同一条整数式子的结果，不是读者的猜测）。
    * 来回一趟在图上看得最清楚：`a:picLocks` 整个不见、`<a:stretch>` 还在而里面的 `fillRect` 没了、
      `1440000` 换成 `1439640`（4000 → 3999），而 `a:off` 与两句 alt 一字未动，号则全被重排
      （`rId2` → `rId1`，形状 id 2 → 63）。留下的与不见的都按文件交。
    * odp 走的是文档那一族同一份 frame 账（尺寸自带单位、alt 是孩子元素），但 Impress 给图框
      **不写** `text:anchor-type` —— 那一族的 `placed` 是 null。同一个 ODF 家族里，odt 写了 `as-char`
      而 odp 什么都没写，所以这不是「默认值」问题，是两家的写法本来就不一样。

73. **「这几个字长什么样」与「这一段长什么样」是两本账，而四份件把前一句话写在四个地方**
    （`styled-text.docx`、`styled-text-lo.docx`、`styled-text.odt`、`styled-text.rtf`）。
    * OOXML 把格式写在**段里每一串字自己**的 `w:rPr` 上，而且写成**孩子元素**（`<w:b/>`、
      `<w:color w:val="C00000"/>`），`rPr` 自己一个属性都不写。于是「有没有 rPr 这一格」
      与「这一格里面有没有话」必须是两个数：python-docx 不给没格式的那一串写这一格
      （14 有 / 0 空），LibreOffice 重写同一份件时给**每一串**都补一个空的
      （30 有 / 16 空），而两边「说过话的串」都是 14 条 —— 合成一个布尔就把生产者习惯
      读成了文档里的一句话。
    * 「明确不粗」在这四份件里有四种拼法：`w:val="0"`、重写后的 `w:val="false"`、
      ODF 的 `fo:font-weight="normal"`、RTF 的 `b0`（否定是粘在控制字上的一个数字）。
      只按「`w:b` 这个孩子在不在」判，第一段那种话会被读成**反的**。
    * ODF 既不在段上也不在串上写值：`text:span` 只点一个样式名（`T1`…`T12`），值在一跳之外
      那份 `style:text-properties` 上（这一族的自动字符样式恰好也写在 content.xml，
      所以 `found_in` 交 "content"；命名的字符样式住 styles.xml，两处都找、按文件写的名字交）。
      量到的第三种情况最容易被读丢：**夹在两个 span 中间的那串字，文件根本没给它立元素** ——
      那一条 `element` 是 `#text`，而 `style` 与 `resolved` 都是 null：没有号可查，
      与「有号而查不到」不是一件事。
    * RTF 没有「一串字」这个元素，格式写在**群头**上：`{\cf23 …}`、`{\fs18 …}`、
      `{\loch\hich\dbch\b …}`。所以「有串而没说格式」在这一族判不住，`with_props`
      与 `props_empty` 交 null；`\cf` 与 `\f` 只是**一个号**，要跳文件自己那两张表
      （23 → `C00000`，9 → 字体表里那一条，而那条的名字是非 ASCII 字节，解不动就照旧 null，
      「查到条目」与「读出名字」分开说）。段前缀那一层（所有群之外）说过的控制字
      归属于段、不归属于任何一串字，另记 `words_outside_groups`（这份件里 135 条）。
    * 同一家族的四个口袋：CJK 那串下划线，LibreOffice 写的是 `\aul`（日文下划线）而不是 `\ul`，
      所以 `underline_word` 先把文件点的那个口袋交出来，开关再按四个口袋算一次「要」。
    * 三家对同一句「9 磅」的说法：`w:sz w:val="18"`、RTF `\fs18`（都是半磅，两家同一个数）、
      ODF `fo:font-size="9pt"`（自带单位）。三个都按原样交，不折成一个数。

74. **一句话可以拆在两处说：段上只写一个样式号，另一半住在另一个部件里**
    （`charstyles.docx`、`charstyles-lo.docx`、`charstyles.odt`、`charstyles.rtf`）。
    * OOXML 的 `w:rStyle` 坐在 `w:rPr` 的第一个孩子位上，而它的定义在 `word/styles.xml`：
      第一段只写 `Strong` 这个号、粗体在定义里 —— 所以「段上自己说了什么」（`switches`）
      与「样式说了什么」（`style_switches`）是两栏，`bold_on` 数到 1、`bold_from_style` 数到 1，
      把两个合成一个「三处都粗」就是把两个来处当一个。第二段更直接：样式 `Emphasis` 说斜体、
      段上自己写 `<w:b/>` 说粗体，两处各说一半，`where_both_spoke` 因此是 0 而不是 2。
    * **样式号与样式名不是一回事**：第三段点的号是 `SubtleEmphasis`，名字写着「Subtle Emphasis」
      （中间那个空格是文件写的）；ODF 那一家同一段写的是号 `Strong_20_Emphasis`，
      显示名在 `style:display-name` 里，两栏都交。父样式（`w:basedOn` / `parent-style-name`）
      报出来但**不跟**；主题色 `w:themeColor="text1" w:themeTint="7F"` 按写的交 ——
      解它要开 `theme1.xml`，那一跳这一族不走。
    * ODF 把「样式」这件事做得更彻底，也露出一个以前没人走的分支：段上自己写的格式
      被搬成 content.xml 的自动样式 `T1`，而点命名的字符样式在 **styles.xml**，
      于是同一段话**套成两层 span**（外 `Emphasis`、内 `T1`）。账本因此加了 `depth`，
      并且一条 span 自己的 `text` 只算它直接带的那些字 —— 外层那条交空串，
      里层那条交「又粗又斜」，同一句话绝不报两次（`nested_spans` 就是为这一条而存在的数）。
    * 跨家最容易被对成一个数的地方：ODF 的开关值**全部来自样式**（那一家没有别的地方可写），
      所以它的 `bold_on` 是 2；OOXML 的 `bold_on` 只数段上自己写的，是 1。
      两个数都对，但它们是两问 —— 所以这一族不做等号，只把两栏并排放着。
    * RTF 又一层：字符样式在流里是 `{\*\cs34 … Strong;}` 一群，群头那个 `\*` 的意思正是
      「不认识这个群就整个跳掉」—— 而这一族**认得** cs 定义（段落样式一直是这么读的），
      于是名字以前整个丢掉（`name: null`）。现在解名字之前先把那层 `\*` 剥掉，
      34 → `Strong`、37 → `Subtle Emphasis` 都读得出来；这与脚注那条是同一个教训：
      「不认识才跳」不等于「认识了就允许把里面的名字一起跳掉」。
      同时 LibreOffice 还把样式自己的 `\b` **抄进了群头**（号与话同时在场），
      所以 `words` 里 `cs` 与 `b` 并列，两处都交。

75. **一串字里「有什么」与「说了什么」是两问：域指令不写在页面上，注的号分两本账**
    - `contents` 按文件顺序交出除 `rPr` 以外的每一个孩子，而 `text` 只算 `w:t` 里的那些字：
      `toc.docx` 那条域指令那一串 `text` 是空串，` TOC \o "1-2" \h` 整串交在 `instructions` 上 ——
      把它当成页面上的字读，目录就凭空多出一行谁也看不见的话；图（`drawing`）与分页符
      （`br`，`type` 照写的交）同样是一串字的孩子而一个字都不写，于是 `runs_with_text` 12
      对 `checked` 21 —— 那九个「有这一串而串里没字」是数出来的，不是猜的。
    - 注的引用只有一个号，而**号分两本账**：`notes-end.docx` 的脚注 `2` 排在部件第 0 条、
      尾注 `2` 排在第 2 条，只按号对就会两条都落在第 0 条（第一版就是这么写的，跟第二读者
      逐条对账才逮住）。`note` 因此交种类、号、解到没解到与它排在部件第几条，而
      `notes_in_parts` / `notes_referenced` / `notes_unreferenced` 从部件与正文两头各数一遍
      （这批件里三条全被引用，`notes_unreferenced` 是 0；「点了号而部件里没有」那一条分支
      手上还没有真件，别当它被读过）。
    - `commentReference` 的号照交，但它不跳注那两份部件（批注住在 `comments.xml`，另有一本账），
      所以 `toc.docx` 那一份 `runs_with_ref` 是 1 而 `ref_found` 是 0 —— 那个 0 是「数过了没有」，
      与「没看」不是一回事。
    - ODF 那一条不包起来的字（`element: #text`）四个开关格**各交 null 而不是缺键**：缺键在这份
      报告里只表示「这一族没看」。这一条是重写 `collect_pieces` 时把四格弄丢的，由 16 份 .odt
      的整份对照账一起报出来 —— 一个新形状没有老件当样本，就会这样只活着不走。

76. **「这一串字是被谁包起来的」是第三问：三个壳上的字，串自己一个都没有**
    - `wrapped` 交壳的元素名（`hyperlink` / `ins` / `del`，没壳交 null），`wrapped_written`
      交那份壳自己写着的属性（作者、时间、修订号、`r:id`）—— 这些字都不在串上。
    - **同一次插入，两家写出两种形状**：`revisions.docx` 一条壳（id `11`）包住整句「124000 元」，
      LibreOffice 重写后是两条壳（id `0` 与 `1`，数与单位各一条），并且把全文所有修订号重排
      （`15` → `5`）。所以 `runs_wrapped` 3 对 5、`wrapped_ins` 2 对 3、`wrapped_del` 1 对 2，
      三个数各说各的，而修订账把那两条合成一条逻辑改动 —— 两份账问的不是同一件事。
    - **删掉的字写在 `w:delText` 而不是 `w:t`**：那一串 `text` 是空串，`contents` 照样交出
      `delText`，作者与时间从壳上拿（两家一字不差 `李四` / `2026-03-06T11:45:00Z`）。
      「这一串没有字」与「这一串的字被删掉了」在这里同一个数（空串），差别靠 `contents` 说清。
    - 链接那一句「预算制度」在三副件里是三个号（`rId2` 与 `rId9`）而地址一字未改 ——
      与图的 `r:embed` 是同一条教训：**号是生产者自己排的，只列不比**。

77. **生产者不接受的形状：`w:fldChar` 必须住在串里 —— 挂在段上，一次来回就把域变成死字**
    - 第一版 `add_field` 把 begin / separate / end 三条 `w:fldChar` **直接挂在段上**（与
      `instrText` 那个 run 并列）。这一份 docx 我们两个读者都照读，账上一切正常；
      可 LibreOffice 把它 import 再导出时那三条壳全被丢掉，剩下「指令那一串」和「缓存那一串」
      当成两句普通的字写回 odt/rtf —— 页面上凭空多出 ` SEQ 表 \* ARABIC1` 这样一句死字
      （指令与值粘在一起，中间没有任何分隔）。
    - 也就是说：**「读者读得懂」不等于「文件写对了」**。这一条是拿真生产者量出来的，
      不是从规范里推的；改法是把每一条 `w:fldChar` 包进自己的 `w:r` 里，
      再导出就得到 `text:sequence` / `text:page-number` / `\field` 那些正经形状。
    - 留在这里的教训与「两个读者一起错」是同一族：这次的两个读者一起**放过**了一个坏件，
      只有第三个生产者（LibreOffice 的 import）说了不。

78. **一串字里可以一个字都不写，只说「这里要算」；站内跳转对的是书签的名，不是号**
    - `w:instrText` 那一串 `text` 是空串，`instructions` 交出 ` SEQ 表 \* ARABIC`（反斜杠按写的交）；
      页面上那句 `1` 住在**另一个 run** 里（begin/separate 与 separate/end 之间），那是上一次算出来的
      缓存值。`w:fldChar` 的三种记号各交在 `field_chars`，`field_runs` 数「参与这条链的串有几条」。
    - **`w:dirty` 是「这域脏了，下次要重算」，而只有写的那一份有**：LibreOffice 重导成 docx 时把这一格
      整个丢了，同时把日期与页码的缓存值换成它自己算出来的数（`2026-09-24` → 当天、`2` → `1`）。
      指令本身也变了：多一个尾空格、日期格式里的连字符被反斜杠转义（`\@ "yyyy-MM-dd"` 成
      `\@"yyyy\-MM\-dd"`）—— 三份都按各的文件交，不折成同一个「日期域」。
    - 站内跳转的地址写在 `w:anchor` 上（外部链接写的是 `r:id`），而它对的是 `w:bookmarkStart` 的
      **名字**，不是 `w:id` 那个号：`link_found` 三条一件 —— `true` / `false`（这份件里就有一条指着
      `没这个书签` 的坏跳转）/ `null`（整个没写 anchor）。`anchors_missing` 那条 `false` 分支从此有真件撑着。
    - 页脚里的第四条 PAGE **不在正文这份账里**（`structure.paragraphs` 走 body），所以 `field_runs` 是
      12 而不是 16 —— 「少了哪一条」由部件那本账与 `office-text` 的页眉页脚那一份说清，不在这里偷偷补。

79. **ODF 把链接与域也写成段里的一条元素；同一个 anchor 问题两族各问一次**
    - `text:a` 与 `text:date` / `text:time` / `text:sequence` / `text:page-number` /
      `text:expression` 现在与 span 走同一趟账：各带 `own_written`（元素自己写着的属性，前缀留着）、
      `link_href` / `link_anchor` / `link_found`，也照旧解一次 `text:style-name`。
    - **同一句话，一族解得开、另一族解不开**：LibreOffice 给 ODF 那一条链接点的是
      `ListLabel_20_5`，这一族的样式表里真有这么一份（`resolved: true`），而 OOXML 那份模板里的
      `Hyperlink` 在整个样式表里根本没有（`style_found: false`，事实 74）。同一件事在 `toc.odt`
      里又叫 `ListLabel_20_2` —— 号是生产者自己起的，只列不比。
    - 站内跳转的 href 前缀一个 `#`，对的是 `text:bookmark-start` 的**名字**：这一族**不给书签写号**，
      而 OOXML 是一对（`w:id` 配 `bookmarkStart` / `bookmarkEnd`，名字只在 start 上）。
      两族的坏跳转数一致：`fields.docx` 与 `fields.odt` 都是 1 对 1 错 —— 同一份稿子、两种存法、
      同一个坏名。站外的 href 就是地址本身，没有第二跳 → `link_anchor` 与 `link_found` 交 null。
    - 域是**拆开写**的：OOXML 一句串到底的 ` SEQ 表 \* ARABIC`，在 ODF 里成
      `text:name="表"` + `text:formula="ooow:表+1"` + `style:num-format="1"`；日期另点一份数据样式
      （`style:data-style-name="N10049"`）并带完整时间戳（`text:date-value="2026-09-25T09:31:12…"`），
      而页面上那句缓存值照写的交 —— 正文那一条页码的缓存值写着 `0`，重算不是读者的活。
    - 注、软分页与书签本体仍然整块跳过：`office:annotation` 里那些 `text:date` 是批注的时间，
      有自己的账，在带它的那一段里再算一遍就是把同一件事报两次。

80. **第三族答同一个 anchor 问题：书签群的名字读得到，而它一个字也不进正文**
    - `{\*\bkmkstart 名}` 与 `{\*\bkmkend 名}` 仍然**整群跳过**（与批注、字体表那一路同一个规矩：
      认得一个群不等于要把它当正文），只是跳之前前瞻读一次名字。于是
      `bookmarks` = 解过转义的那几个名，`bookmark_starts` / `bookmark_ends` 两条列表各数一遍 ——
      「有 start 没 end」这种文件自己的失配自己会说。
    - 站内跳转的地址住在**指令**里（`HYPERLINK "#表锚点"`，那是解过转义的那一份），
      而 `links[].target` 交的是文件原样写的那一串（`#\u-30616\'3f\u-27366\'3f\u28857\'3f`）——
      同一个地址两份凭据，读者不替它们对上。`anchors` 是把指令里那些以 `#` 开头的地址去掉 `#`，
      `links_external` 数不带 `#` 的那几个。
    - **三族同一份稿子答同一个数**：`fields.docx` / `fields.odt` / `fields.rtf` 都是
      1 条指得到、1 条指不到。这一条不是两份读者对出来的，是三族对出来的 ——
      同一个坏名在三种存法里都还坏着。
    - 探针里钉着一条**可反证**的：书签的名字没进任何一行正文（那六行一字不多）。
      前瞻读名字这件事如果哪天把字漏进正文，这条就会响。

81. **分节的页眉页脚：没写那一格不是「没有」，是沿用上面那一节**
    - OOXML 的规矩：`w:sectPr` 里没有某种 `headerReference` / `footerReference`，就沿用上
      一处写了它的那一节。所以每一格有三种答案：`own` / `earlier-section` / null，
      而不写这一格与「这一格是空的」是两件事。`sections.docx` 第一节写页眉与页脚，
      第二节一个字没写 → 两格都是 `earlier-section`，指向第一节那两份部件（`rId9` / `rId10`）。
    - 号也可能**根本不存在**：第二节上补的那条 `w:headerReference w:type="even" r:id="rId999"`
      在 `word/_rels/document.xml.rels` 里没有对应的关系。这一格交 `part: null`、
      `part_exists: false`，而 `external` 也交 **null** —— 读者不知道那个号本来要不要站外，
      把它写成 false 就是替文件说话。`refs_unresolved` 数得出这一条。
    - 两个开关按写的交，不答「所以这一格显不显示」：每节一个 `title_pg_written`
      （python-docx 的 `different_first_page_header_footer` 写它），
      `even_and_odd_headers` 交「元素在不在」与「写的值」——
      写了而没给值与整个没有这个元素是两份不同的文件。
    - **量到的一条生产者脾气**（写这份件时撞见的）：python-docx 新加的节默认
      `linked_to_previous` —— 给第二节写页眉，字其实落进第一节那份 `word/header1.xml` 里，
      全件仍然只有页眉页脚各一份部件。所以这份件的页眉写的是「第二节页眉」而节 1 用着它，
      这不是读者的错，也不是 Word 的错，是 python-docx 的默认值。

82. **一个框里的字装不下怎么办：pptx 写在框上，odp 写在框点的那份样式上，而 LO 把两档合成一档**
    - pptx 的答案是 `a:bodyPr` 的**独子元素名**：`a:noAutofit`（什么都不做）、`a:spAutoFit`
      （框随字长）、`a:normAutofit`（字缩进框，且带着算出来的 `fontScale="75000"` 与
      `lnSpcReduction="20000"`）。坑不在元素名，在「一个子元素都没有」：**python-pptx 在
      NONE 那一档什么都不写**（`deck.pptx` 两页三个框全是空的 `bodyPr`），而它给新建文本框
      的默认反倒是 `<a:spAutoFit/>`（`deck-autofit.pptx` 第二页那一个没人设过）。于是
      `says_nothing`（看了那条 `bodyPr`，它没说）与 `by_element["a:noAutofit"]`（文件明说了
      什么都不做）是两本账，不并成一个「不缩放」。
    - odp 要跳一跳：`draw:custom-shape/@draw:style-name` → 那份 family=graphic 样式 →
      `style:graphic-properties` 上的 `style:shrink-to-fit` / `draw:fit-to-size` /
      `fo:wrap-option`。三个键来自三个命名空间，所以**键带着文件自己写的前缀**交出去
      （折成局部名就会互相盖掉，与 `fo:` / `loext:` 那一条同一口径）；跳不通与说了没有
      也分两笔：`style_found` / `style_missing` / `props_written` / `says_nothing`。
    - **量到的一条生产者脾气**：LibreOffice 把 pptx 的「什么都不做」与「框随字长」两档转成
      odp 之后是**一模一样**的一条（`gr1` 与 `gr2` 都是 `shrink-to-fit=false` 加
      `fit-to-size=false`）—— 那一档在这次转换里就是丢了。报告把这两行原样摆出来，
      不替它猜回哪一档。同一次转换里 `wrap="square"` → `wrap`、`wrap="none"` → `no-wrap`：
      一句问题两种字面，两边各交各的，只比那一个各家自己答的 `shrinks_text`。
    - 只有 `draw:custom-shape` 进账（Impress 把普通文本框写成这个），备注块里那些
      `draw:frame` 另数在 `frames`（实测每页 1 个）。框自己的位置尺寸 pptx 交 EMU 加换成
      0.01mm 的那一份（`457200` → `1270`），odp 交自带单位的原样串（`1.27cm`），
      谁都不换算成对方的单位。

83. **这份文档要点哪些字体：一张表、四处点法、主题那一跳，而 ODF 的两种指针不能并成一数**
    - OOXML 的三样东西在三处：声明在 `word/fontTable.xml`（实测 python-docx 那份就是模板的八条，
      每条只写一个 `w:name`），点在每一格 `w:rFonts` 上，而**同一个选择按书写系统写成四个属性**
      （`ascii` / `hAnsi` / `eastAsia` / `cs`）—— python-docx 的 `font.name` 一次写 `ascii` 与
      `hAnsi` 两遍，所以「几条属性点了名字」（`pointed_by_value`）与「几格说过话」
      （`pointer_elements`，实测 fonts.docx 是 78：正文 4 + styles.xml 74）是两个数。
      还有一条路不指字面名而指**主题**：`asciiTheme="minorHAnsi"`，要再跳一跳，到
      `word/theme/theme1.xml` 的 `minorFont/latin@typeface` 才落到 `Cambria`。
    - 第四个属性的拼法与前三个不一致：`asciiTheme` / `hAnsiTheme` / `eastAsiaTheme` 而 **`cstheme`**
      （小写 th）—— 名字是文件写的，不改拼法也不替它统一。
    - **LibreOffice 的 docx 重写在这里最看得出不无损**（`fonts-lo.docx`）：主题那一跳被**就地解开**，
      同一条 `w:rFonts` 既写 `ascii="Cambria"` 又留着 `asciiTheme="minorHAnsi"`（两句话都在，交两份）；
      字体表补进了 `Courier New` 而把两个日文字体从表里**去掉**，于是「点了而表里没有」从 1 个名
      变成 4 个名；`styles.xml` 里 26 条 `cs=""` —— 「写了空话」与「没写这个属性」分两个键；
      而我写在正文那一段的 `eastAsia` 点法整个不见了（正文 `w:rFonts` 从 4 条变 3 条）。
    - ODF 换了两张键：`style:font-face` 声明的是 `style:name`（**表的名字**）与
      `svg:font-family`（**真正的族名**），实测同一张表里两种写法并存 —— `Calibri` 的族名不带引号，
      `Liberation Sans` 的族名写作 `'Liberation Sans'`（多一层单引号）；`Cambria` 与 `Cambria1`
      指着**同一个族名**，只靠 `style:font-charset="x-symbol"` 分开；另有一条 `name="F"` 的
      族名是空串。点它的地方也分**两种指针**：`style:font-name` 对表的名字，
      `style:font-family` 对族名 —— 于是 fonts.odt 上「按名字比全落得地」（`undeclared_names: []`）
      而「按族名比有一条没出现」（`undeclared_families: ["'Courier New'"]`）：
      并成一个数就是把两件不同的事说成一件。
    - 这张表在 content.xml 与 styles.xml 各写一份**一模一样的 11 条**（`faces_duplicated: 11`）——
      两份都走、同名先到的一条算数，与格子样式、列表样式那几条同一条规矩。
    - **没有一个生产者嵌过字体**：`embedded_refs` 交 0（数过了没有），`word/fonts/` 另数一本；
      `w:embedRegular` 那条分支与 ODF 的 `embed="font-file:…"` 都还没有真件可走。
      RTF 不交这个键 —— 那一族的字体在 `{\fonttbl…}` 里，账已经记在 `font_list` 与
      `font_definitions` 那两本上（见事实 41 一类）。

84. **ODF 的页眉页脚在母版页上，而「有没有一节点它的名」是另一本账**
    - 不在正文里，也不在页版式（`style:page-layout`）上：`style:master-page` 自己带子元素，
      一格一个 —— `style:header` / `style:footer` 再各配 `-first`（第一页）与 `-left`（偶数页），
      六格与 docx 那一份账同一个形状。每格只有两种答案：写了（`present` + 那一个元素自己的
      属性 + 里面每一段的字）或整个没有（null，不是空串）。
    - 这一格里面有没有「自己会算」的东西另数：`fields.odt` 那只写了一个 `text:page-number`
      （缓存的字按写的交，`第 1页`），六格里有三个格是这样长出来的（.ods 那几份）。
    - ODF 没有 OOXML 那句「与上一节相同」可交：节只点名一份**版式**
      （`text:section/@style:page-layout-name`），母版页点名它自己的版式，而「正文用哪份母版页」
      在这六份真件里一个字都没写。所以账本只交 `masters_named_by_section` / `masters_unnamed`
      —— 有没有一节点过这份母版页的名，不替 ODF「第一份当默认」那条规范话当文件说过的话。
    - **量到的一条转换损失**（`notes-hf.odt`，出处是同一份两节 docx）：LibreOffice 的 odt 导出
      根本不写 `text:section`（`sections_total: 0`），而是造出第二份母版页 `Converted1`
      把第二节那句不同的页眉搬进去，两份母版页还指着**同一个**版式 `Mpm1` ——
      那一句字还在文件里，可没有任何一节点它的名（`masters_unnamed: 2`）。
      `paper-a4.odt` 是第二份凭据：两份母版页各指各的版式（`Mpm1` / `Mpm2`，就是已经交出去的那一横排），
      而六格一个都没写 → `slots_written: 0`（数过了没有，不是没看）。
    - 顺带量到的一条（已做，见事实 85）：同一份账对 .ods 也读得动 —— `book.ods` 五份母版页 × 六格 = 30 格、
      其中三格里有字段，`office-sheet` 现在把这一份账交在 `page_styles` 上。

85. **表格的页眉页脚只在页版式上：字住在左右两半里，而表连不到页版式**
    - `office-sheet` 的 .ods 分支现在交一份 `page_styles`（与 `office-doc` 那一份 `header_footers`
      同一个函数、同一个形状）。为什么另起一个键而不挂在每张表上：实测这五份 .ods 里
      `table:table` 没有 `table:style-name`，`ta1` / `ta2` / `ta3` 那三份自动样式没有
      `style:page-layout-name`，而 `PageStyle_5f_说明` / `草稿` / `预算表` 三份母版页指着
      **同一份**版式 `Mpm3` —— 全文没有一条写着的属性把某张表连到某份页版式。于是这份账按
      页版式交，归属交 `masters_named_by_section: 0`，不替文件的沉默编一条路出来。`available`
      这一格说的是那两份件（content.xml / styles.xml）**读没读到** —— 成员解压超过上限时交
      `false`，而不是把一份空账当成「这份文件没有母版页」。
    - **段可以不住在格子的直接孩子里**（这一条是改读者的原因）：`Report` 那份页眉的
      `style:header` 下面坐着 `style:region-left` 与 `style:region-right`，一边一段 —— 左半是
      `text:sheet-name` + `text:title`，右半是 `text:date` + `text:time`。只走直接孩子，这一格
      读出来是 0 段、空串，而它明明写着字。所以段落改走后代，每一半再另交一份 `regions`
      （元素名、段数、那一半的字），两半各 1 段、合起来 2 段。
    - 表名与标题在这份文件里缓存的是 `???` 三个问号，日期与时间缓存的是 `0000/00/00, 00:00:00`：
      按原样交，不替它算 —— 那一串占位符是文件自己的字，替它算出一个当天日期就是伪造。
    - 每一格另交 `display_written`：`Default` 与 `Report` 的 `-first` / `-left` 那四格、
      以及 `PageStyle_*` 三份的六格，都写着 `style:display="false"`。「这一格写了而它自己说
      不显示」与「整个没有这一格」（null）是两份不同的文件，而「所以打不打得出来」不归读者判；
      没写这一开关的格交 null，不是 `true`。
    - 数出来的（探针 3v 对 `*.ods` 逐份整块比）：`book.ods` 五份母版页 / 30 格 / 3 格里有字段，
      `chart.ods` 四份 / 24 格，`hidden.ods` / `errors.ods` / `cell-notes.ods` 三份 / 18 格；
      这五份的 `sections_total` 与 `masters_named_by_section` 都是 0，`masters_unnamed` 都等于
      母版页数 —— 每份 .ods 都自带那两份有字的母版页，与这份件里有没有人用过它无关。
    - **一条没走到的分支**（量过才敢说）：想让 docx 的页眉分成左右两半，得写制表位，而
      LibreOffice 导出 odt 时把那种分栏写成 `text:tab`，不写 `style:region-*` 元素 ——
      临时生成的对照件证实了这一点，故未入库。所以「左右两半」这一支目前只有 .ods 走得到。

86. **这张表打出来是哪几行几列：一家写成两条保留名，一家写成表身上的一条属性**
    - OOXML 根本没有「表自己说打哪几列」这个地方：`_xlnm.Print_Area` 与 `_xlnm.Print_Titles`
      写在 `xl/workbook.xml` 的 `definedNames` 里，归属靠 `localSheetId`，而那个数数的是
      `<sheets>` 里的**先后** —— 不是 `sheetId`（这份件是 1..4），也不是 `r:id`（openpyxl 从
      `rId1` 起、LibreOffice 从 `rId3` 起）。三套号各自编，所以账本把「写着的号」「解出来的号」
      「落到哪张表」分三格交：号写了却不是数、或越界，归属交 null 而原号照交 —— 那与「没写号」
      是两份不同的文件。
    - 一条 definedName 可以塞**好几段**（`'两段区域'!$A$1:$B$6,'两段区域'!$C$8:$C$12`），所以
      「原句」与「摊开的几段」两个都交；ODF 的分隔符换成**空白**
      （`两段区域.A1:两段区域.B6 两段区域.C8:两段区域.C12`），地址写法也是第三种（点号，不是 `!$`）。
    - LibreOffice 重写同一份 xlsx：五条、两个名字、五个归属一字不差，**而 sheet 名的引号全没了**
      （带引号的从 5 条变 0 条），条目顺序还整个重排 —— 引号是生产者的写法不是文档的说法，
      所以两边各按各的交，`quoted_entries` 单列一本。
    - ODF 那一族有**两处**，而且不能互相顶替：`table:print-ranges` 坐在 `table:table` 自己身上
      （四张表里三张有），另有一份 LibreOffice 为了与 Excel 来回而写的 `table:named-range` /
      `table:named-expression` 五条，名字一律 `Excel_BuiltIn_*`。四处量出来的要紧事：
      1. **重复行那一半只在来回那一份里** —— `table:print-ranges` 永远不会写 `$1:.$1`，
         只读那条属性就把「每页重复第 1 行」读丢了；
      2. 同一个选择在一种文件里是两种元素：一段范围写 `named-range`，两段那一样写
         `named-expression`（于是 `named_by_element` 是 4 与 1）；
      3. 五样的 `table:base-cell-address` **全是同一个**（`$区域与标题.$A$1`，
         `distinct_base_addresses: 1`）—— 「这是哪张表的」只在地址串里，不在这条指针上；
      4. `table:range-usable-as` 把「重复行」与「重复列」写成同一串（`repeat-column repeat-row`），
         所以它只按写的交，不答「重复的到底是行还是列」。
    - 对照件：`book.xlsx` 有一条命名区域（`总额`）而**零条**保留名 → `print_entries: 0`
      （数过了没有，不是没看），`book.ods` 的 `with_print_ranges: 0`；`.xls` 这一族整个不交这个
      键 —— 那一族的打印开关住在 SETUP(0x00A1) 记录里，这里没有一个读者能核对它的字段位。

87. **页上那个框「我是什么角色」这句话，两家生产者都可以不写 —— 而两份读者以前一边猜一个**
    - OOXML 把角色写在形状的 `p:nvSpPr/p:cNvPr` **旁边**那条 `p:ph` 上（`type` 与 `idx`）。
      python-pptx 给正文占位符写的是 `<p:ph idx="1"/>` —— **`type` 整个不写**；LibreOffice
      重写同一份时写成 `<p:ph/>`，连 `idx` 也丢了。所以「这一格是标题吗」在两份真件里
      一次是「写了 title」、两次是「什么都没说」。
    - 这一格以前**两份读者各猜一个**：Rust 兜成 `"other"`，python 兜成 `"title"`
      （`node.get("type") or "title"`）。规范里这个属性的默认值其实是 `body` —— 也就是说
      两边都不对，而且因为它们对得不一样，`deck.pptx` 第一页那个正文占位符在 Rust 的
      `paragraphs[].placeholder` 上是 `"other"`，在读者账本里却是 `"title"`。
      **这条分歧是写这份件之前一直看不见的**，原因是那条「占位类别」的对照只跑在 `.odp` 上。
      现在两边一律：文件写着 `type` 就交那一句，没写就交 `null`，既不叫 `other` 也不叫 `title`。
    - 于是「没写角色的占位符」与「根本没有 `p:ph` 元素的自制文本框」在这份账里同为 `null` ——
      这不是把两件事说成一件：形状数（`shapes`）与角色账（`placeholder_words`）并排放着，
      第 2 页 3 个形状对 `[title, null, null]`，一眼看得出不平衡在哪。
    - 版式（`ppt/slideLayouts/*.xml`）里也是同一族混写法：同一份模板，`slideLayout2` 写
      `<p:ph idx="1"/>`（无 `type`）、`slideLayout3` 写 `<p:ph type="body" idx="1"/>`、
      `slideLayout9` 写 `type="pic"`、`slideLayout10` 还写 `orient="vert"`；而 LibreOffice
      重写后所有版式的正文都成 `<p:ph type="body"/>`（**idx 全省**），`dt`/`ftr`/`sldNum`
      换成它自己一套连续号（模板 10/11/12 → 这里 1/2/3、4/5/6、…、28/29/30）。
      「这框对应版式里哪一条」那一跳在重写那份里因此是**断的**，所以这一族不假装接得上。
    - ODF 换了一套地方：角色是 `presentation:class`（`title` / `outline` / `page` / `notes`），
      「这是占位符」另有 `presentation:placeholder="true"`，占位符是带 `presentation:style-name`
      的 `draw:frame`，而文本框是**没有**那个属性的 `draw:custom-shape`（实测还有一frame
      连 `draw:name` 都不写）。页点哪份版式也不在页上，在画页样式里
      （`presentation:presentation-page-layout-name`）。
    - 一句口径上的实话（**故意不对齐**）：两份读者对「这页的标题是哪句字」的规则不同 ——
      Rust 只从写了 `title`/`ctrTitle` 的占位符里取，读者在没有占位符时兜回本页第一句字
      （`deck-ph` 第 4 页 Rust 交 `""`，读者交 `只有一个文本框`）。这是读者的规则差，不是文件的
      事实差，所以那条 blanket 只比角色账、不比标题，免得把读者的手法规成文件的说法。

88. **占位符对版式那一跳：一份件走得通，另一份走不通，而这不是读者的错**
    - 页上的 `p:ph` 只留一个号（`idx="1"`），字与位置在那一页点名的版式（`slideLayoutN.xml`）
      里 —— 所以「这一框对应版式里哪一条」是一跳，而要走页自己那张关系表才知道是哪份版式。
      对法只有三种，全按写的比：号写了按号对；号没写按名对；两边都空则对「版式里那条也全空」。
    - `deck-ph.pptx`（python-pptx）：六个有 `p:ph` 的形状**全对上**（三个按号、三个按名，
      `hop_found: 6`、`hop_missing: 0`）。`deck-ph-lo.pptx`（同一份稿子经 LibreOffice 重写）：
      页上那三条正文占位符变成空元素 `<p:ph/>`，而它那份版式给正文写的是 `type="body"` ——
      一句没说话对上一句说了话，于是 **`hop_found: 3` / `hop_missing: 3`**，条数一模一样。
      按规范的默认值（body）替它接一下就能"全对上"，那是替文件说话，所以交「对不上」。
    - 第三份凭据 `deck-lo.pptx`：它的版式里那条也什么都没写（`(None, None)`），页上那条同样
      空 —— 两边写得一样，这一跳就算通（`hop_found: 3`、`hop_missing: 0`）。可见"断"不是
      丢信息，而是**两边写的对不上了**。
    - 三种情形分得开：`by_idx` / `by_type` / `no_ph` —— 最后一种是自制文本框，那一跳
      根本无从走起，与"走了但没对上"不是一回事；`no_idx_written` 单数一本（写了 `p:ph` 而没写号）。
    - ODF 那一族没这一跳的对应物可交：角色写在 `presentation:class` 上（本身就是答案），
      页点哪份版式在画页样式上（`presentation:presentation-page-layout-name`），
      所以 `.odp` 与 `.ppt` 都不带 `placeholder_hops` 这个键。

89. **这张表的哪几行每页重复：一家写在行上（一枚无值的元素），一家写在表身上（两个数）**
    - `table-header.docx`（python-docx 走 oxml 层）：四张表里 3 张标了、一共 4 行，其中第四张把标记写在**第二行**上（`non_leading: 1`、那一张的 `contiguous_from_first: false`）。`w:tblHeader` 按规范没有值，在场就是重复，所以「哪几行」只能逐行交一张布尔表 —— 只交一个总数就把这件事抹平了。
    - `table-header-lo.docx`（同一份稿子经 LibreOffice 重写）：那一枚「不是从第一行起」的标记整个不见了（4 行 → 3 行、`non_leading` 1 → 0），同时它给**每一行**都补了一枚**空的** `w:trPr`（1/2/0/1 → 3/3/3/3）。于是「有几枚 trPr」与「有几行标了表头」必须是两本账：并成一个数就会把「这一行被写过」当成「这一行是表头」。
    - `table-header.odt`（同一份稿子转 ODF）：表身上那四个属性（`table:header-rows` /`header-rows-repeated` / `header-column` / `header-columns-repeated`）**一个都没写**，四张表全交 null。这里所有 `.odt` 一件都没写过这件事（每一份的 `with_header_rows` 都是 0），所以 ODF那一支只证得到「按写的交、不替文件兜」—— null 是「这份文件没说」，0 才是「它说了不重复」。
    - 两族的形状不折算：能对齐的只有「几张表」这一问（同一份稿子两份都是 4 张）。有表却一条没标的件（`paper-a4.docx`）交 `tables_total: 0` 与空数组，不是缺键；RTF 那一族有它自己的第三种写法（`\trhdr`），这一支还没读，所以 `notes.rtf` 根本不带 `table_headers` 这个键 —— 缺键就是「这一族没看」。

90. **这一段上有哪几个制表位：一家写在段上，一家一跳在段点的样式里，而位置是两个不同的串**
    - `tabs.docx`（python-docx，四条段各改一个变量）：`w:pPr/w:tabs/w:tab` 三个属性 ——
      `w:pos` 是整数 twip（3cm 落成 `1701`、9cm 落成 `5102`，落不下 1700.79），`w:val` 八条全写了，而 `w:leader` 有 **2 条整个属性不落**（生产者那一档叫 `SPACES`）—— 所以那两格交 null，交 `"none"` 就是替文件说话。
    - `tabs.odt`（同一条稿子转 ODF）：**四个数一模一样**（段 5 / 有定义的段 4 / 定义 8 / 制表字符 8），可这一族的制表位不在段上 —— 段只写一个样式名（`P1`…`P4`，父名 `Standard`），定义在那份样式的 `style:paragraph-properties/style:tab-stops/style:tab-stop` 上；位置换成带单位的串，而 **9cm 成了 `8.999cm`**（谁换算的谁负责，读者不替它平回来）；对齐有 5 条没写（左对齐这一家压根不写），「引导符」是 `style:leader-style` 与 `style:leader-text` **两个**属性合起来的，小数点那一样整个换成 `type="char"` 配 `style:char="."`。
    - 同一条稿子在 RTF 里是第三种：位置又回到 twip（`\tx1701` 4 条、`\tx5102` 4 条），而对齐与引导符是**只管紧跟的那一个** `\tx` 的前缀（`\tldot` `\tqr` `\tlul` `\tqdec`…）——规则量准了记在 `tab_stops.rs` 模块头，这一支读者读了它，只是形状不同：那份件的 `structure.tab_stops` 逐条交流上数到的 `\tx`（8 条）与 `\tab`（8 个），与前两家同一个数（见事实 91）。
    - 两本账同名要分开：定义是 `w:tabs/w:tab`，字符是 run 里的 `w:tab`（ODF 是 `text:tab`）——全局数一遍 `tab` 就会把 8 条定义数成 16 个字符。这里每一份的 `tab_chars_total` 与 `stops_total` 都是 8，那是这份稿子恰好相等，不是同一条账。
    - 「写了没人点」在这一问里又出现一次：`tabs.odt` 44 份具名段落样式里 7 份写了制表位，其中 3 份（`Header` / `Footer` / `macro`）**没有任何段点它**；`style:default-style` 没有名字可点而照样落到每一段上，所以另交一份 `default_style_stops`（这几份件里都是空的）。
    - 有段而一条没定义的件（`paper-a4.docx` 五段）交 `with_stops: 0`、`stops_total: 0` 与空数组，不是缺键 —— 0 是数过了没有。

91. **第三家：RTF 的制表位是一条扁平流，前缀只管紧跟的那一个位置**
    - `tabs.rtf`（同一条稿子经 LibreOffice 转 RTF）：正文 walk 数到 8 个 `\tx` 与 8 个 `\tab` —— 与前两家的「定义 8 条 / 字符 8 个」是**同一个数**，而单位回到 OOXML 那种 twip（`1701` / `5102`），ODF 那个 `8.999cm` 是另一家的写法。
    - 前缀的账只能逐条交：`\tqr` / `\tqc` / `\tqdec` 各 1 条（align_words 3），引导前缀 6 条（`\tldot` 1、`\tlth` 1、`\tlul` 4），而「左对齐」这一家**不写词** —— 所以 align_words 与 positions 是两个数，谁也不顶替谁；第 3 条位置带 `tqr` + `tldot`，第 4 条只带 `tlul`。
    - 样式表那一群里另有 4 条位置（`header` / `footer` 两份样式各写 `\tqc\tx4680` 与 `\tqr\tx9360`），而两家读者的正文 walk 都按这一族的规矩**跳过** `\stylesheet`，所以那 4 条不进账本。段点了哪份样式、样式里又有位置 —— 这一族没有段边界可认，于是**不冒充归属**，只交流上数得清的（这就是它与另两族形状不同之处：那边逐段交，这边逐条交）。
    - 「有制表字符而一个位置都没定义」在这族是真有的：`lists.rtf` 交 `chars: 5`、`positions: 0`（那 5 个是列表标签里的 `\tab`）；`tables.rtf` 交 `positions: 0` —— 0 是数过了没有，与另两族交空数组同一个意思。

92. **这几条批注是谁写的、锚在哪一段：OOXML 分两处按号配，ODF 合在一段里**
    - `doc-comments.docx`（python-docx 1.2 的 `add_comment`）：内容与锚点分家 —— `word/comments.xml` 里三条各带 `w:id` / `w:author` / `w:initials` / `w:date`（时间带 Z），正文里三种锚点（`commentRangeStart` / `commentRangeEnd` / `w:r/w:commentReference`）只带号。两个方向都要数：有内容没锚（orphan）与有锚没内容（dangling）是两回事，这里都是 0。
    - 同一段可以锚两条：这份件第 0 段带 `ids: ["1", "0"]` —— 号在正文里的先后与部件里的先后不是一套。
    - `doc-comments-lo.docx`（LibreOffice 重写同一份）：条数 3、六个锚点数、`hosts` 全部一字不差，而**部件里那三条排成 1,0,2**（原来 0,1,2），`distinct_authors` 因此也换了顺序 —— 所以「第几条批注」不说清算哪个就是两个答案，两份都交、不挑一个。
    - `doc-comments.odt`：`text:annotation` 就坐在所属那一段里面，作者与时间是孩子元素（`dc:creator` / `dc:date`），而那个时间**没有 Z**；「全文几段」在这一族是两个数：6 个 `text:p` = 正文 3 段 + 批注里 3 段（只交一个就会把批注的字当正文的字数进去）。
    - 没写过批注的件（`paper-a4.docx`）交 `part_written: false` 与一串 0，不是缺键；RTF 那一族不交这个键（它的批注早另有 `annotations` 那一本账：作者、日期、字、号都交）。
    - 还没做的：批注**回复线程**。python-docx 的 `Comment` 没有回复 API，Word 那一条是 `word/commentsExtended.xml` 里的 `w15:paraIdParent`，本机没有任何生产者写过它 —— 所以这一格不在账上（不猜）。

93. **这一段与下一页的关系：四个开关在 OOXML 坐在段上，在 ODF 一跳在样式里，而且不是一个开关**
    - `keep.docx`（python-docx 的四条真 API）：`w:keepNext` / `w:keepLines` / `w:pageBreakBefore` 写出来是**空元素**（在场就是开着，文件没给值），而关掉孤行控制写出来是 `w:widowControl w:val="0"`。三种状态分开交：`present`（元素在不在）、`val`（文件写的值，没写交 null）、再按给的词算 `on_written` / `off_written` —— 把「在场」当「开着」就会把 `w:val="0"` 数成开着。
    - `keep-lo.docx`（LibreOffice 重写同一份）：五段每段都被补了一个 `w:pPr`（4 枚 → 5 枚），值换成 `w:val="true"` / `w:val="false"` 这一种拼法，而**带 `w:pageBreakBefore` 的那一段整个不再有这一格**（交着开关的段从 4 段掉到 3 段，`paragraphs_indexed` 1,2,3,4 → 1,2,4）—— 同一份稿子的「段前分页」在这一转里丢了，读者只按看到的交。
    - `keep.odt`（同一份稿子转 ODF）：段身上一个字都没写，四个开关一跳在段点的样式上（`P1` `fo:keep-with-next="always"`、`P2` `fo:keep-together="always"`、`P3` `fo:break-before="page"`、`P4` `fo:widows="0"` + `fo:orphans="0"`）。关键形状差：**孤行控制在这一族是两个数，不是一枚开关**；而基线那段点的 `Standard` 自己写着 `widows=2` `orphans=2` —— 所以「五段全都有人写了数」（`paragraphs_with_any` 5）比 OOXML 那份的 4 还多，两族这两个数不能互相对账。
    - RTF 那一族不交这个键（缺键 = 这一族没看）：它写 `\keepn` / `\pagebb` / `\nowidctlpar`，可实测同一份件里 11 条 `\keepn` 中只有一条落在正文段上、其余在样式表里，而 `\widctlpar` 8 条也几乎都是样式表自带的默认 —— 归属判不住，规则先记在这里而不是硬算一个数。

94. **这张表套的是哪个样式：一家是两本账（样式 id + 那枚 look 的位与缓存），一家只剩一个名字**
    - `table-style.docx`（python-docx）：样式住在 `w:tblPr/w:tblStyle/@w:val`，而那是个**样式id**（`LightGrid-Accent1`）不是给人看的名字；另有一枚 `w:tblLook`，六个 `w:firstRow`… 的位**加一个十六进制缓存**（`w:val="04A0"`）。四张表里两张点了样式、四张都带 look。
    - 两本账可以互相不一致，而这是文件自己的账：第三张把 `w:firstRow` 改成 0，缓存还是 `04A0`（python-docx 不重算它）。读者两个都按写的交，不拿位去修缓存、也不拿缓存去修位。
    - `table-style-lo.docx`（LibreOffice 重写同一份）：样式与六个位一字没改，而它**把缓存重算了**（第三张变 `0480`），另外几张那个值顺手换成小写（`04a0`）—— 大小写与算没算都是写法差别，两条都在账上并排看得见。
    - `table-style.odt`：这一族只有一个名字 —— 四张表各点一份 family=table 的自动样式（`表格1`…`表格4`，都能找到、都没有父样式），而 OOXML 那个样式 id 与那枚 look **整个不见了**：样式那一路的信息在这一转里丢了，交看到的、不替它认回来（`look_written` 这个键在 ODF 根本没有）。
    - 没套样式的件（`tables.docx`）交 `with_style_written: 0` 与空数组而不是缺键；RTF 那一族不交这个键（缺键 = 这一族没看：它用 `	rowd` 那一套行属性，样式是另一回事）。

95. **这一段的行距是多少：一家那个数的单位由紧跟的另一枚属性决定，一家把单位写在串上**
    - `line.docx`（python-docx）：`w:pPr/w:spacing` 上两枚属性管一件事 —— `w:line` 是那个数，`w:lineRule` 说它是**什么单位**：`auto` 时是 1/240 倍（`360` 就是 1.5 倍、`480` 就是 2 倍），`exact` / `atLeast` 时是 twip（22 磅 = `440`、18 磅 = `360`）。于是「1.5 倍」与「至少 18 磅」在文件里是**同一个数**，只有 `lineRule` 分得开。读者两枚分开各交（`line_written` / `rule_written`），不合成一个「行距」字段、不换算、也不拿规范里的默认值替那一格没写的段接上。
    - `line-lo.docx`（LibreOffice 重写同一份）：四个数与其单位一个都没改口（`rules_written` 还是 `auto` 2 条、`exact` 1 条、`atLeast` 1 条），而段零被补了一份 `w:pPr` —— 里面**没有** `w:spacing`。补壳子与补内容是两件事，所以 `has_pPr` 与 `has_spacing` 各记各的。
    - `line.odt`：一跳在段点的那份样式里，`fo:line-height` 是**带单位的串** —— 1.5 倍 → `150%`、2 倍 → `200%`、22 磅 → `0.776cm`（读者只按串尾分类交出去：`unit_forms` = `%` 三条、`cm` 一条，不换算也不约分）。一丢一多都在账上：`atLeast` 那一段四个相关属性**一个都没写**（那格 null，不是 0），而 docx 里什么都不写的段零这一族点的 `Standard` 样式里写着 `115%` —— 同一份稿子两个答案，谁也不替谁圆。
    - RTF 那一族不交这个键（缺键 = 这一族没看）：`\sl` 与 `\slmult` 在样式表里就成批出现（`{\s0\snext0\sl276\slmult1…}` 是默认段样式），段自己没写时它是继承来的，归属判不住 —— 与制表位、段落缩进那两条同一个坑。

96. **这一段自己有没有说画个框、铺个底：一家的壳与里面的边是两件事，一家一条 shorthand 顶四条边**
    - `pborder.docx`（python-docx，段边框没有公开属性所以走 `OxmlElement`）：`w:pPr` 下面摆两枚元素 —— `w:pBdr` 是**装边的壳**，里面 `w:top` / `w:left` / `w:bottom` / `w:right` 各带自己的四个属性（`val` 是线型、`sz` 是 **1/8 磅**、`space` 是「边离字多远」的磅、`color` 可以写 `auto`）；`w:shd` 是底纹（`val` / `color` / `fill`，还能点一枚 `themeFill` 主题色）。这份件里三枚壳、五条边、两枚底纹，其中**一枚壳整个是空的**（`border_element: true` 而 `edge_count: 0`）—— 那是文件说过的话，不能读成「这一段没边框」，所以在场与有内容分两个数。
    - `pborder-lo.docx`（LibreOffice 重写同一份）：每段都被补了一份 `w:pPr`（4 → 5 枚），而那个空壳**整个不见了**（带壳的段从 3 掉到 2、`border_element_empty` 归 0），另外把 `w:color="auto"` 折成一个具体色 `000000`。三条边与两枚底纹的值本身一字未改 —— 「丢了一格」与「换了一种说法」都在账上，不去替它圆。
    - `pborder.odt`：两样都搬到段点的那份样式上（一跳），而形状整个换了：四边单线合成**一条 shorthand**（`fo:border="0.74pt solid #ff0000"` —— 值里塞着宽度、线型、颜色三段，`sz=6` 那枚 1/8 磅在这里写成 `0.74pt`），只有上面一条双线那一段则**四条各写一遍**，其中三条明写着 `fo:border-left="none"`（这一族说「这边没有」是写出来的，与 OOXML 那不写这一条边不是一回事，所以 `sides_written` 4 与 `sides_none` 3 分开数），另多一份逐根的 `style:border-line-width-top="0.079cm 0.079cm 0.079cm"`；docx 那个 `w:space="1"` 在这里搬成 `fo:padding="0.035cm"`。
    - 底纹那一枚最要紧：**同一个 `w:fill` 在两种 `w:val` 下不是同一个角色** —— `clear` + `fill="FFFF00"` 那一段转过去是 `#ffff00`，而 `solid` + `fill="00B050"`（另点着 `themeFill="accent6"`）那一段转过去成了 `#ffffff`。两份读者都把串原样交出来，不猜哪个才对，也不拿规范里的默认值替它接。
    - RTF 那一族不交这个键（缺键 = 这一族没看）：`\brdrb` 这一族段边框住在样式表里而非段自己身上，与制表位、行距那两条同一个坑 —— 归属判不住。

97. **文档里有几个文本框、框里写了什么：一个框可以在一份件里存两份，而框里的段不是正文的段**
    - 为什么值得单独一本账：框里的字页面上只出现一次，但在 OOXML 的件里可以**存两份** —— LibreOffice 的 docx 导出把 `tbox.odt` 那一个框写成 `w:drawing`（DrawingML，尺寸在 `wp:extent` 的 EMU 上：`cx="1800225" cy="864235"`）与 `w:pict`（VML，那一份的 `v:shape` 干脆没写 `style`）两个分支，各带一份 `w:txbxContent`，两份里的字一字不差。所以 `text_boxes` 这一族把「几份格子」（`boxes_total` 2）与「几句话」（`distinct_text_count` 1）分两个数，合成一个就把同一句话读成两个框、或把两个框读成一份。
    - **框自己带段**：正文的直接孩子 3 段，整棵树 7 段，差的那 4 段就是两份副本各带两段（`paragraphs_in_boxes_direct` 4）。这与批注、脚注、`text:note` 同一族教训 —— 「这份文档有几段」本来就有两个诚实的答案，只交一个就说不清别的账本的 `index` 是从哪份清单数的。
    - `tbox.odt` 是这一族的源头件（zipfile 写的最小 ODF：`mimetype` 第一成员 + manifest 三条）：框是 `draw:frame`，名字、锚、尺寸与坐标都写在框自己身上，尺寸是**自带单位的串**（`5cm` / `2.4cm`），与 OOXML 那两处的 EMU 与 `style` 串都不是同一种东西 —— 按写的交、不换算也不互证。
    - `tbox-lo.odt`（LibreOffice 重写同一份 odt）：挂上帧样式 `Frame`，而 `svg:x` / `svg:y` / `draw:z-index` **三格整个不见**（null，不是 0），尺寸从 `5cm` / `2.4cm` 换成 `5.001cm` / `2.401cm`；那一句话一字未改，也还是只有一份。「丢了哪几格」与「换了写法」都在账上，不替它接回去。
    - **有帧不等于有框**：`notes.odt` 那一张图的 `draw:frame` 里没有任何 `draw:text-box`，于是 `frames_total` 是 1 而 `frames_with_boxes` 与 `text_box_elements` 都是 0 —— 数帧当框就会把一张图读成一个文本框。没有框的件交一串 0 与空表而不是缺键。
    - RTF 那一族不交这个键（缺键 = 这一族没看）：LibreOffice 的 RTF 导出里 `SHAPPIE` 0 次、`\pict` 0 次 —— 框这个形状在那条流里根本不存在，字直接落进正文段落，判不出「这一段在框里」（与制表位、行距、段边框同一族）。

98. **这些书签是怎么配对的：止只写号不写名字，断的两个方向各一本账，重名的看生产者怎么办**
    - `bkmks.docx`（python-docx，书签没有公开属性，走 `OxmlElement`）八段各造一种情形：完整一对、跨段一对、只有起、只有止、Word 自己的 `_GoBack`、与第一段**重名**的第二条、以及一条站内跳转 `w:anchor="跨段"`。量到的头一条是形状差：`w:bookmarkStart` 带 `w:id` **和** `w:name`，而 `w:bookmarkEnd` **只带 `w:id`** —— 五条止的 `name_written` 全是 null。所以「这条书签闭没闭」只能按号配，不能按名字配。
    - 断的两个方向各记一本账：这份件里 5 起 5 止、闭 4 对，`starts_without_end` 1（`断了`）与 `ends_without_start` 1（号 `9`）—— 合成了一个数就说不清是哪种断法。名字以下划线开头的是 Word 自己塞的光标记号（`_GoBack`），`hidden_starts` 另数一笔：把它算进「这份文档有几个书签」就是替 Word 说话。重名不合并（`distinct_names` 4 个、`duplicate_names` 是 `["口径"]`、`names_total` 仍数 5 次）。
    - `bkmks-lo.docx`（LibreOffice 重写同一份）：两个**断的整个被删掉**（5 起 5 止 → 4 起 4 止、两本孤账都归 0）、号从 1..5 整批重排成 0..3、第二条重名的它不报错而是**改名** `口径_副本_1`，而站内跳转的 `w:anchor="跨段"` 一字未改 —— 删、排、改都是文件自己的事，读者只交现在这份件写的。
    - `bkmks.odt`：记号在这一族有**三种** —— `text:bookmark` 是一枚点，`text:bookmark-start` / `-end` 才是跨段的一对（两头都写 `text:name`，所以按**名字**配，没有号可查）。最要紧的一条：同段起止的那一对在这里变成**一枚点**，于是这一件成了「3 枚点 + 1 对跨段」，而 docx 那面是「5 起 5 止」—— 两个数不是同一个问，谁也不换算成谁。那个改名的副本在这里写作带空格的「口径 副本 1」，与 docx 那面的下划线是两个不同的串。
    - 没有书签的件交一串 0 与空表而不是缺键；RTF 不交这一份配对账（缺键 = 这一支不再交一次）：那一族的 `\bkmkstart` / `\bkmkend` 条数早就在 `structure.bookmarks` 那本账上，两份数不互相顶替。

99. **这一页放映时怎么换：一页可以写两条 `p:transition`，而三个属性各说一件事**
    - `deck-tr.pptx`（python-pptx，切换没有公开属性，走 `parse_xml`）三页各改一个变量：第一页把 `spd`（多快）、`advClick`（点一下换不换）、`advTm`（几毫秒自动换）三个都写满，效果是**孩子元素** `p:fade`；第二页只写 `spd="fast"` 而方向在孩子自己身上（`p:wipe dir="l"`）；第三页一个字都不写（`elements: 0`）。这三句是可以互相独立的：写了速度不等于说了要不要点。
    - `deck-tr-lo.pptx`（LibreOffice 重写同一份）：第一页的 `advClick` **没了**（只剩 `spd` 与 `advTm`）、第二页连 `spd` 也没了（属性表整个是空的，而孩子的 `dir=l` 留着），第三页**原本什么都没写，它补了两条** —— `{spd:slow, dur:2000}` 与 `{spd:slow}`，两条都没有效果孩子。于是一篇里「4 条元素」而「只有 3 页有切换」：**一页两条是真会发生的**，这两个数不能互推，所以每页交 `elements`（几条）+ 每条自己的 `written`（写了哪些属性）+ 孩子清单。
    - `deck-tr.odp`：切转换了地方也换了词表 —— 页面上一个 `p:transition` 都不剩，属性去了 `style:drawing-page-properties`（`presentation:transition-type="automatic"`、`transition-speed="fast"`、`duration="PT5S"`，且 dp1 写了 dp2 没写），效果本身去了一棵 SMIL 动画树（`anim:transitionFilter` 的 `smil:type="fade"` + `smil:subtype="crossfade"`，第二页是 `barWipe` + `leftToRight` + `smil:dur="0.5s"`）。本条账只读 OOXML 那一种，所以 odp 的页**不交这个键**（缺键 = 这一族没看，不是 0）—— 那一族的账是另一问、另一次测量。
    - 两支读者比这一问时**不比页序**：一支按 `presentation.xml` 的放映序列页、一支按部件名，所以探针按内容排序的多重集比（外加两条求和：全篇几条、每页几条之和），位置留给放映序那一本账去说。

100. **同一页的切换在 ODF 写在两处：页点名的 drawing-page 样式里一份，页体内那棵动画树又一份**
    - `deck-tr.odp` 三页正好凑成三种情形。第一页点 `dp1`，那份 `style:drawing-page-properties` 上写满了一句半：`presentation:transition-type="automatic"`、`transition-speed="fast"`、`duration="PT5S"`，紧挨着还写效果自己的 `type="fade"`、`subtype="crossfade"`、`fadeColor="#000000"`；而页体内那棵 `anim:par node-type="timing-root"` 树里，`anim:transitionFilter` **又把效果写了一遍**（`smil:dur="0.75s"` + fade/crossfade）。两处都交、不互证，也不挑一个当准 —— 与 pptx 那两张尺寸（`wp:extent` 与 `a:ext`）同一族先例。
    - 第二页点 `dp3`，只写半句：有 `transition-speed="fast"` 与 `type="barWipe"` / `subtype="leftToRight"` / `direction="reverse"`，而**没有 `transition-type`、没有 `duration`**。这一族没有「默认就是 fast」这回事，没写就交没有（不拿规范或别的页的写法替它接）。
    - 第三页点 `dp4`：那份样式**找得到**（`style_found: true`，在 content.xml），可一个切换属性都不写 —— `written` 是空表而不是缺键，`effects` 是空表、`timing_roots` 是 0。与 pptx 那一面正好反过来：同一份稿子的第三页在 LibreOffice 的 **pptx** 重写里被**补了两条** `p:transition`，而它的 **odp** 导出对同一页一个字都不写 —— 同一个生产者的两个导出方向相反，两份件各自说自己的话。
    - `deck.odp` 两页都点 `dp1` 而那份样式什么都没说：两页 `written` 都是空表、`style_found` 都是 true —— 「跳到了那份样式而它没说」与「跳不到那份样式」是两件事（后者 `style_found` 才是 false）。
    - 两支读者的键按族分开：pptx 每页带 `transition_detail`、odp 每页带 `odp_transition`，另一族那个键整个不在（不是空表）；比这一问不比页序，按内容多重集与效果条数求和比。

101. **这一节的页码怎么写：OOXML 一节三条属性、三种待遇，而「从几开始」跨族两头各丢一次**
    - 形状先分开：OOXML 把这一问放在**每一节**的 `w:sectPr/w:pgNumType` 上，一枚元素三条可以各自缺的属性
      （`w:fmt` 用什么数、`w:start` 从几起、`w:chpNum` 跟不跟章号）；ODF 放在**页版式**的
      `style:page-layout-properties` 上（`style:num-format` 与 `style:page-number`）。一节一条与一版式一条
      不是一套计数，两边各交总数与找得到的条数，不做等号。
    - 元素在场与属性写了什么是两件事：`notes.docx`（python-docx 的原件）那一节**根本没有**
      `w:pgNumType`（`with_element` 0、`element_present` false、`written` 空表），而 LibreOffice 转出的
      那几份（`keep-lo.docx` / `line-lo.docx` …）每一份都带这一格、写着 `fmt="decimal"` ——
      同一个问题的两份件，这一格在不在完全看生产者。缺键、false、空表与 null 各说各的话，
      这里一个都不合并。
    - 三条属性不是同一个待遇：`restart.docx` 把三条全写（`start="7"` / `fmt="upperRoman"` /
      `chpNum="none"`），LibreOffice 重写同一份（`restart-lo.docx`）之后 `start` 与 `fmt` 一字未动，
      而 `chpNum` **整格没了**。所以「写了哪几条」按现在这份件交，不替上一版接回来。
    - 跨族走一趟，「从几开始」两头各丢一次：`restart.docx` → `restart.odt` 只剩页版式上
      `style:num-format="I"`（这一族把大写罗马写成一个字母 `I`，词汇表与 `w:fmt` 不是一套，
      两边各按写的交、不折算），`style:page-number` 一个字没落（`with_page_number` 0）；反方向
      `pnum.odt` 段落上明写 `style:page-number="7"` + `style:use-page-numbering="true"`，转成 docx 后
      那一节只带 `fmt="decimal"`，`w:start` 是 null —— 不是 0、也不是 7，而是这一格没写。
      两头都不替它猜「那大概就是从头开始」。
    - 一处不赌它住在哪：这一族的页版式**通常在 styles.xml**（实测这几份都在），但两份件都走，
      每条交自己的 `part`；`layouts_total` 与 `masters_total` 分两个数（`pnum.odt` 是 1 与 2 ——
      两份母版页共用一份版式）。LibreOffice 转出的那份 odt 还另写一个 `style:default-page-layout`，
      它的 `page-layout-properties` 只带网格设置 —— 与「那张纸」同一规矩：那条既不报也不编号。
    - RTF 与遗留 .doc **不交这个键**（缺键 = 这一支没看）：RTF 的页码写在节属性那一格里
      （`\pgndec` 是十进制、`\pgnstart` 才是起点），实测这批件里 LibreOffice 只写 `\pgndec`，
      两份件的 `notes-hf.rtf` / `paper-a4.rtf` 各两次（正好一节一次），而 `\pgnstart` 一个都没有；
      这一支读者的 `sections` 本来就是 null，节归属判不住（同「那张纸」只交文档级的先例）。
      `.doc` 的节属性住在 table stream 里，这一族读者不走那里。

102. **这份文档写了哪种语言：OOXML 一枚元素说三路文字，ODF 只有一格还要拆成两段**
    - 形状先分开：OOXML 的 `w:lang` 有三个可以各自缺的属性 —— `w:val`（拉丁那一路）、
      `w:eastAsia`（中日韩那一路）、`w:bidi`（复杂脚本从右往左那一路），一条元素可以同时说三路；
      它可以坐在四层上（`word/styles.xml` 的 `w:docDefaults`、样式定义、段自己的 `w:pPr/w:rPr`、
      每串字的 `w:rPr`），四层各交一份、不合并。ODF 只有一格语言位：字符属性
      `style:text-properties` 上的 `fo:language` + `fo:country`（外加 `fo:script`），
      值是**拆开的两段**（`en` + `US` 对 `en-US`）。两边各按写的交，不拼也不折。
    - **模板的说法不是作者的说法**：`notes.docx` 全文只有 styles.xml 里那一条 `w:lang`，
      它在 `docDefaults` 上同时写 `val="en-US"` / `eastAsia="en-US"` / `bidi="ar-SA"` —— 一份全中文
      稿子在文件级默认上声明「复杂脚本是阿拉伯语」。这条账把它原样交出来（`doc_defaults`），
      同时 `in_document` 是 0：正文一个字都没说过，两件事分开写。
    - 段层与 run 层以前没有生产者：全语料 51 份 docx 的 `w:lang` 一条都不在正文里。
      `lang.docx` 用 python-docx 自己的 XML 层写出来（段一条 + run 三条），量到三个属性确实
      各自独立：`distinct_vals` = `es-ES / fr-FR / de-DE / en-US`、`distinct_east_asia` =
      `ja-JP / zh-CN / en-US`、`distinct_bidi` = `ar-SA` 是三个清单，不并成一个「几种语言」。
    - LibreOffice 重写同一份（`lang-lo.docx`）：正文四条一字未动，另外给 `Normal` / `NoSpacing` /
      `MacroText` 各补了一条（值都是 en-US / en-US / ar-SA）—— 5 条变 8 条、`levels_seen` 多一层。
      **它补的不算这份稿子说过的话**，所以按四层分开交，不合成「这份文档的语言」。
    - 跨族那一趟丢得最狠（`lang.odt`）：ODF 只有一格语言位，于是**只写 `eastAsia="ja-JP"` 那一串字
      整个没落**（`distinct_languages` 里没有 `ja`），三路全写那串只剩 `de` + `DE`（`zh` 与 `ar`
      都不见）—— 交回来的 `distinct_languages` 是 `de / en / es / fr`。
    - 「说了没有」与「一个字不说」是两件事：`tbox-lo.odt` 的一条 `style:text-properties` 写
      **`fo:language="none"`**（`none_written` 1，它的 `country` 也写着 `none`）；`tbox.odt` 与
      `pnum.odt`（zipfile 写的最小件）整族零条 —— `elements_total` 0、`entries` 空表、
      `parts_seen` 空表，不替它补 `en`。
    - 宿主走法不用父指针：ODF 只认 `style` / `default-style` 的**直接孩子** `text-properties`，
      另交一份「整棵树里带这三个属性的元素」条数与 `not_under_style`，两边能互相对账；
      两支读者的序都是**两趟**（先所有 `style`、再所有 `default-style`），否则整份 list 没法比。
    - RTF 与遗留 .doc **不交这个键**（缺键 = 这一支没看）：RTF 写 `\lang` 加一个 LCID 数字
      （另有 `\langfe` 那一路），整名比对与归属判据还没量完 —— 实测这批件里 `\lang` 族控制字
      每份都出现 5–18 次，但「哪一段说的」没判据；而「这份文档是哪国语言」那个**属性级**问句
      早就在 `office-meta` 的 `dc:language` 那一份账上，两份数不互相顶替。

103. **脚注与尾注怎么编号：OOXML 把同一句话写在两处，两处说的不一样；ODF 一类注一份，两类不对称**
    - 形状：OOXML 的 `w:footnotePr` / `w:endnotePr` **自己不写属性**，值在孩子身上
      （`<w:numStart w:val="5"/>`、`<w:numRestart w:val="eachPage"/>`、`<w:numFmt w:val="decimal"/>`、
      `<w:pos w:val="sectEnd"/>`），另有两个特殊孩子 `w:footnote` / `w:endnote` 带 `w:id` ——
      那是分隔符与延续分隔符的引用（注部件里两条空正文的占位）。它可以出现在**两处**：
      `word/settings.xml` 一份、每一条 `w:sectPr` 又一份。
    - 头条是「两处不一样」：`nset.docx` 的 settings 那份说了 `numStart=5` + `numRestart=eachPage`，
      节里那一份只有 `pos` 与 `numFmt`，也没有分隔符引用 —— 所以 `attrs_only_in_settings` 是
      `["numRestart", "numStart"]`、`attrs_in_both` 是 `["numFmt", "pos"]`，两份各交一份、不合成。
      （另一条同类先例是图的尺寸与替代文字：两处都写就是两处都报。）
    - LibreOffice 重写同一份（`nset-lo.docx`）：**两处的那两格都没了**，`attrs_only_in_settings`
      因此变空 —— 「谁丢了起点」在账上看得见；编号格式与分隔符引用一字未动。
    - ODF 是一类注一份 `text:notes-configuration`（实测两份都在 **styles.xml**），两类注**不对称**：
      footnote 那份带 `style:num-format` + `text:start-value` + `text:footnotes-position` +
      `text:start-numbering-at`，endnote 那份只有前两个 —— 没写的交 false / null，
      不拿另一类的写法替它接。词汇也与 OOXML 不是一套（`1` / `i` 对 `decimal` / `lowerRoman`，
      `text:start-value` 对 `w:numStart`，`text:start-numbering-at` 对 `w:numRestart`），两边各按写的交。
    - 跨族那趟照旧丢：带 `numStart=5` 的 docx 转成 odt，LO 写的是自己的默认 `start-value="0"`、
      `start-numbering-at="document"`（不是 eachPage）—— 与页码起点那条同一族事实。
    - 反面凭据：`notes.docx`（python-docx 原件，**有脚注**）两处都没写过这一格 →
      `footnote_written` / `endnote_written` 与两条 `sections_with_*_pr` 全是 false / 0，
      `footnote` 是 null 而不是空表 —— 「这份件有注」与「这份件说了注怎么编号」是两件事。
    - 造件的两条坑（记下来免得再试）：元素名是 `w:footnotePr`，写成 `w:footPr` 会被整条丢掉，
      于是量出来的「LO 不读」是假的；而**稿子里一条注都没有时 LO 两处也都不写**，
      那测的是「没有注所以没设置」，不是「LO 不读设置」—— 所以测量件建在真有注的 `notes-end.docx` 上，
      并且补完之后把孩子按 schema 顺序重排一遍（LO 自己写的顺序是 `pos, numFmt, 引用×2`）。
    - RTF 与遗留 .doc 不交这个键（缺键 = 这一族没看）：RTF 的注编号在 `\ftrprops` 那一路控制字上、
      没有节级对应物；.doc 的注设置在 table stream 里。

104. **这一页有哪些形状、哪个是组合、按什么顺序叠着：树自己不算形状，组合自己那两套坐标，而「字装在哪一层」两族各有岔路**
    - 形状一条清单一个形状，按文档序 —— pptx 里 `p:spTree` 的**孩子顺序就是叠放顺序**，
      ODF 里 `draw:page` 的孩子是同一句话的另一种写法。每行交 `kind` / `name` / `id` /
      `depth` / `parent`（父条在本清单里的序号）/ `xfrm` / `size_written` / `text_carrier` /
      `paragraphs_direct` / `children`。`spTree` 自己那枚 `cNvPr id="1" name=""` 不是形状：
      `deck-gr.pptx` 第 1 页第一条就是那个散框，`unnamed` 因此是 0 而不是 1。
    - 头条是「组合是一个条，孩子指回它」：`deck-gr.pptx` 第 1 页 5 条 = 顶层 2 + 组合里 3，
      `groups` 1、`max_depth` 1；组合那一条自己那枚 `a:xfrm` **四份都在**，而 `off` 是 `0,0`、
      `ext` 与 `chExt` 一模一样（`2900000×900000`）—— 外面那份是页坐标、`ch*` 那份是孩子自己的
      坐标系，同单位不同语义，按写的交、不替它「修正」成页上的位置。第 2 页整份清单是空的：
      `shapes_total` 0 而不是缺键（那页的 `spTree` 只剩一个 `grpSpPr`）。
    - LibreOffice 重写同一份（`deck-gr-lo.pptx`）：形状、组合、两套坐标、五个名字一字未动，
      而 `id` 从 2..6 整批重排成 **61..65**（1 让给树自己），坐标走那条老换算
      （`100000`→`100080`、`2900000`→`2899800`）。它还给 `spTree` 自己的那份 `grpSpPr` 补了一个
      **全 0 的 `a:xfrm`** —— 树不是形状，那一份不进清单；这条也正是「找 xfrm 只能看两层」的理由：
      一份自己的 `xfrm` 都没有的组合，往深里找会把孩子的坐标当成自己的。
    - 转成 odp（`deck-gr.odp`）：同一页还是 5 条、层级与五个名字全对得上，但三件事全换了 ——
      分组是 `svg:g`（**`draw:group` 这份件里一次都没出现**，按名字找分组会找空）、
      `id` 这一族根本没有（一律 null，不是空串）、尺寸是 `5.555cm` / `0.278cm` 这种自带单位的串。
      组合那一层更彻底：`size_written` 是**空表而不是 0** —— ODF 的分组不写自己的框。
      `nested` 两族也不同义：pptx 里深度 >0 必在组合里，ODF 里 `draw:frame` 套 `draw:image`
      也算一层（`deck.odp` 第 1 页 `nested` 1 而 `groups` 0），两个数不互相解释。
    - **字装在哪一层**（`text_carrier`）是这一条新学的事：pptx 这边一律 `p:txBody`（一张
      `pic` 干脆没有 —— `deck-pictures.pptx` 前两页 `carriers_seen` 是空表）；ODF 一族的
      `draw:frame` 装在 `draw:text-box` 里，而 `draw:custom-shape` 把 `text:p` **直接挂在形状自己身上**
      —— 所以 `deck.odp` 第 1 页 `carriers_seen` 是 `["text-box", "self"]`，两种并存。
      只按「有没有 text-box」数段，这一页从 4 段掉到 3 段，`deck-gr.odp` 那页 4 段全没。
      段只算自己那一层的：组合里那些记在孩子身上，否则一层报一次、整页翻倍。
    - 反面凭据与界：备注那棵树不进这份账（pptx 只走这一页的 `spTree`，ODF 的形状白名单里没有
      `notes`，走不进去 —— 与页上链接那一条同一规矩）；遗留 .ppt **不交这个键**（缺键 = 这一族
      没看）—— 它的记录树里没有「形状树」这一层，按 0x03EE 容器归页的那本账另在 `records`。

105. **哪条批注已解决、谁回复谁：OOXML 这句话在另外两份部件里，靠段号连，两跳各自都会断；ODF 只有半句**
    - 形状：`word/comments.xml` 里那条 `w:comment` 自己只有 `w:id` / `w:author` / `w:date`，
      **回复与已解决都不在它身上** —— 第二份 `word/commentsExtended.xml` 的 `w15:commentEx` 用
      `@w15:paraId` 指回「批注体内那一段的 `w14:paraId`」（不是 `w:id`！），带 `@w15:done` 与
      `@w15:paraIdParent`；第三份 `word/commentsIds.xml` 又给同一个段号配一枚 `@w16cid:durableId`。
      一问四份数据、两跳才连得上，所以每一跳都单独数「连上了几条 / 剩几条孤儿」。
    - 头条是**两跳都可以断**：`crep.docx` 三条 `commentEx` 只连得上两条（`ext_orphans` 1），
      `commentsIds.xml` 三条里也有一条孤儿（`ids_orphans` 1）。把三份并成一个「几条批注」，
      文件里断着的线就被读成没断。回复也是同一枚号的事：`threads[1].parent_para_id` 是
      `11111111` 而 `replies_to` 是本清单里的第 0 条 —— 号对不对得上，两本账分开交。
    - **「没写」与「写了 0」是两件事**（这一条有生产者凭据）：`crep-r.docx` 里 2 条注只有 1 条
      `commentEx`，另一条是 `ex_found: false`；而 `crep.docx` 那条明确写 `done="0"`。
      三档各数各的：`done_true` / `done_false` / `ex_without_done`（后者是「有记录而没写 done」）。
    - **反向证明词表不是我编的**：把 `crep.odt` 第一格改成 `loext:resolved="true"`（`crep-r.odt`）
      再让 LibreOffice 导成 docx，它**自己写出** `word/commentsExtended.xml` —— 只给已解决那条写
      记录、`w14:paraId` 是它新排的 `01000000`、而 `commentsIds.xml` 整个不写。
      同一条路反过来走（docx→odt）它对 `w15:done` 看都不看：`crep.odt` 两条都写
      `loext:resolved="false"`，源件那条 `done="1"` 没落过来。**同一个生产者的两个方向，一个写一个不认**。
    - 生产者会丢整份：`crep-lo.docx`（LibreOffice 重写 `crep.docx`）里两份部件**整个不见**，
      连批注体内那个段号也没了（`paras_with_para_id` 2 → 0）—— 于是这一问两头都读不出来，
      账上交一串 0 与 `null` 而不是缺键。反面凭据还有 `comments.docx`：python-docx 那份
      **有两条批注而这一格一个字都没写**（`ext_part_written` false）——「有批注」与
      「说过它解没解决」是两件事。
    - ODF 这一族只有半句：`loext:resolved` 直接压在 `office:annotation` 身上（两份件都走，
      `parts_seen` 说清在哪份解到），而**回复没有任何对应物** —— 那一格里既没有 parent 也没有
      thread，所以这一族**不交回复那几格**（不是 0）。另有一条只在别的文档族量到的分野：
      `cell-notes.ods` 三条注**一条都没写** `loext:resolved`（`without_resolved` 3）——
      同一个生产者的 Writer 出口写满、Calc 出口一个字不写，所以「没写这个属性」必须是独立一档
      （那一份是 .ods，`office-doc` 不走它，这一格在探针里没有凭据，只在此记下出处）。
    - RTF 与遗留 .doc 不交这个键（缺键 = 这一族没看）：那一族的注没有「谁回复谁」与「结没结」的位置。

106. **公式那枚 `<f>` 自己写了什么：共享组的跟随格在文件里没有公式正文，三个生产者三种写法**
    - 形状：`shared.xlsx` 一列八格是**一个**共享组 —— 主格写 `<f t="shared" ref="B1:B8" si="0">A1*2</f>`，
      跟随的七格写 `<f t="shared" si="0"/>`，**正文是空的**，要按 `si` 找回主格再按行平移才知道它是什么。
      于是这一页有三个数：`formula_elems` 16（几格有公式）、`text_written` 9（几格写了正文）、
      `empty_text` 7 —— 合成一个数就把「文件没写这条公式」读成了「这格没公式」。
    - `attrs_seen` 交的是**文件里属性写的顺序**（这里是 `["t","ref","si"]`），不是字典序：
      这条清单是这条 lane 的凭据本体，`attr_values` 再给每个属性值的分布（`t` 全是 `shared`、
      `si` 全是 `0`、`ref` 只有一个 `B1:B8`）。
    - **`<v>` 在不在与有没有缓存值是两回事**：这 16 枚 `<f>` 所在格子都带一枚 `<v>`，
      可那标签里没有字（openpyxl 从没算过）—— 所以交 `cached_written`（有那枚元素）与
      `cached`（按写的串，这里是空串）两格，不替它把「有标签」说成「有值」。
    - 三个生产者三种写法：openpyxl 的 `<f>` **一个属性都不写**（`book.xlsx` 1 枚带 0 属性）；
      LibreOffice 重写同一份时**不用共享组**（16 枚各写正文，`shared_elems` 0），
      但给每一枚都写了 `aca="false"`（实测 16/16）—— 「谁丢了共享」与「谁换了写法」都在账上。
    - 跨格式那一路顺手量到一件别人替我们算过的事：同一份转成 `shared.ods`，公式变成
      格子身上的 `table:formula`，16 条全带正文**且逐行平移**（`of:=[.A2]*2`、`of:=[.A3]*2`…）——
      也就是说 LibreOffice 读共享组读对了，跟着 Excel 的语义把每格该是什么写成了什么。
      这一族没有 `si` / `ref` / 共享这些位置，所以那些键**整个不交**（不是 0）。
    - 那一族的形状还纠正了一次（两份读者原本一起把样式名当成了表名）：带公式的是**格子自己**，
      所以 `attrs` 交那一格写着的属性全表，`sheet` 交它所在那张 `table:table` 写的名字
      （实测 `表一` / `预算表` / `错误`），样式名单独占一格 `style`，而 `cell` 逐条 `null` ——
      这一族不写格子地址（列可以整个不写、行可以用 `number-rows-repeated` 顶好几行）。
      前缀的分布挪到 `formula_prefixes` 那一格（xlsx 那一族没有「前缀」这回事，那个键在那边
      整个不出现），`attr_values` 两家同一个意思：每个属性值的分布。
    - 清单按**文档序**：一行里先 B 后 C，所以「第几条」不是「第几行」（第 0 条 B1 主格、
      第 1 条 C1 普通公式、第 2 条才是跟随格 B2）—— 钉住这三条的顺序就是为了别拿索引当行号。
    - 界：这一本只看 `<f>` 元素自己，**不展开共享组、不重放公式语义**（那不在 T0 只读的范围里）；
      老键 `formula` 继续按文件写的正文交（跟随格就是空串），这一本负责说明那个空是怎么来的。
      遗留 `.xls` 不交这个键（那一族的公式在 BIFF 记录树里，是另一问）。

107. **这张字体字典自己说了什么：子集前缀、`/FontDescriptor` 在不在、里面有没有 `/FontFile*`**
    - 两个问句分开交。`fonts` 那一本问「这份 PDF 用了哪些字体」（名字、`/Subtype`、`/Encoding`、
      有没有 `/ToUnicode`、是不是藏在对象流里）；`font_embedding` 这一本只问
      「**这一层有没有说它带了字面数据**」：逐张交 `subset_prefix` / `descriptor` /
      `font_file`，外加 `with_descriptor` / `descriptor_missing` / `with_font_file` /
      `subsets` / `file_kinds` / `subtypes`。键互不重叠，谁也不顶替谁。
    - 正例是 LibreOffice 那七份：每张字体都自己写 `/FontDescriptor`，descriptor 里带
      `/FontFile2`，`/BaseFont` 前面还有一截生产者自己截的**子集前缀**
      （`EAAAAA+Calibri` → `subset_prefix` `EAAAAA`）—— 六张/五张全部如此，
      `pdffonts` 那三列 emb/sub/uni 一律 yes，对象号与这里逐个对得上。
      没有 `+` 的名字交 `null`（不是空串）：生产者没写就不算子集，不拿规范的默认说法替它补。
    - 反面凭据是 `risk.pdf`：一张 `Helvetica`（Type1）—— descriptor 与 font_file 都**没有**
      （`descriptor_missing` 1、`with_font_file` 0），`subsets` 0、`with_to_unicode` 0。
      标准 14 字体本来就从不嵌入，所以这里读不到「嵌入失败」这种故事，只读到「这一层什么都没说」。
    - 两条边界都是量出来的：① 字体字典可以整个住在**对象流**里 —— `objstm.pdf` 明文只有 17 个对象，
      五张字体全在 `/Type /ObjStm` 里，不拆那一层就会报「这张 PDF 一张字体也没有」；
      ② 加密那份（`locked.pdf`）`pdffonts` 一个字都不列，而字体字典是明文对象，
      这里数得出 5 张全带 `/FontFile2` —— 第三方闭嘴与我们能读是两件事，两份答案都留着。
    - 界（这一本**不**做的那一件事）：Type0（CID）字体的 descriptor 不在字体字典上，
      而在 `/DescendantFonts` 第一个孩子身上 —— 本地八份 PDF **一张 Type0 都没有**，
      没有凭据就不写那一跳（`pdf.rs` 里 CID 的 `/W` 宽度同样是早就划出去的界）。
      所以对 CID 件这一本会报 `descriptor: null`，那是「这一层没写」，
      **不是**「这份文件没嵌字体」—— 这一句写进 `font_embedding_of` 的注释里，免得下一轮误读。

108. **这一节是从哪儿开始的：「另起一页」在一份件里根本没写，而重写那一份替它写了**
    - 形状：一节一条 `{section, element_present, type_written, written}`，外加
      `sections_total` / `with_element` / `type_missing` / `distinct_types`。值就写在
      `w:sectPr/w:type/@w:val` 上，那一枚元素的属性表整份交在 `written` 里（键名按写的交），
      不折成布尔、不归一化。
    - 正例是 `sstart.docx`（python-docx 写三节：另起一页 / 连续 / 偶数页）：3 节里只有 **2 节**
      写了 `w:type`（`with_element` 2、`type_missing` 1），因为 `nextPage` 正是 Word 的默认，
      生产者一个字都不写；`distinct_types` 于是只剩 `["continuous","evenPage"]` ——
      不是「第二节是 continuous」这一句少了，而是第一节那句话**从未落在纸上**。
    - LibreOffice 重写同一份（`sstart-lo.docx`）：3 节全写，第一节多出一枚
      `<w:type w:val="nextPage"/>`，另两节的值一字未变（`with_element` 2 → 3）。
      所以「这一节另起一页」在源件里是「没说」、在重写件里是「说了」—— 两份各交各的，
      既不拿规范的默认替前者补上，也不因后者多写就改前者的账。
    - 反面凭据是 `restart.docx` / `notes.docx`：`sections_total` 1、`with_element` 0、
      `distinct_types` 空。单节文档多半什么都不写，这一本对它们交的就是「这一层什么都没说」，
      而 `0` 是数过了没有（与「这一族整个没看」的缺键是两件事）。
    - ODF 那一族**不交这个键**（缺键 = 这一族没看，不是 0），理由是量过的：LibreOffice 把
      `sstart.docx` 转成 odt 之后，全文只有**一枚** `text:section`（`text:name="TextSection"`，
      就是「连续」那一节），另两节既没有 `text:section` 也没有任何写着起始类型的地方，
      分页改由段落属性点母版页承担（`style:master-page-name="Converted2"`，而 styles.xml 里
      确实排着 `Standard` / `Converted1` / `Converted2` 三份）—— 同一句问话在这族里拆成两处、
      还有一半没落纸，所以不硬凑一个键；页版式与母版页那两处的账另在 `page_numbering`
      与 `header_footers` 里。

## 这些数字从哪来
109. **把结构搬进 markdown：两家生产者的渲染一字不差，而「列表号写在样式上还是段上」在账上分得开**
    - 形状：`office-text --markdown` 才交这一本（没开就整个键都不给，不交一份空串装作渲染过）——
      `{family, available, text, chars, cut, blocks, paragraphs, headings, list_items,
      bullet_items, ordered_items, list_from_style, unresolved_fmt, tables, table_rows,
      empty_dropped}`：`text` 是渲染结果，其余那些数说的是「这一本凭什么这么长」。
      ODF 那一面同形状再加 10 格：`lists_named` / `lists_unnamed` / `spans_unresolved` /
      `annotations_dropped` / `notes_dropped` / `space_markers` / `links` / `images` /
      `covered_cells` / `repeated_spans`。
    - 正例是 `md.docx`（python-docx 写，每段只管一件事）：18 块、10 段、2 标题、5 个列表项
      （3 圆点 + 2 编号）、1 张表 3 行、丢掉 2 个空段，`chars` 359。
    - LibreOffice 重写同一份（`md-lo.docx`）：**渲染一字不差**（359 个码位一个不缺），而
      `list_from_style` 从 5 变成 0 —— 号在 python-docx 那份里写在**样式**上
      （`List Bullet` → `numId`，段上自己不写），重写时抄到**段上**。同一个选择在两家文件里
      落在两个地方，搬进 markdown 之后看不出来，因为渲染只问「这一段是不是列表项」；
      两个数都留着，才知道是谁改的手。
    - 层级只按文件写着的 `ilvl` 走：`md.docx` 那条「缩进一层的那条」在这一族是**换了一个 numId**
      表达的（样式里连 `ilvl` 都不写），所以它不缩进 —— 搬不过去的那一层由 `list_items` 与
      `bullet_items` 这两个数说清，不拿样式名字尾数的数字当层级。反面凭据是 `lists.docx`：
      那里「直接挂在段上的第二级」真写了 `ilvl="1"`，缩进两格；另有两条**解不到编号格式**
      （一条点了不存在的号、一条是 LibreOffice 重排出的 `numId="0"`），渲染挑最保守的 `- `，
      而 `unresolved_fmt` 2 把这件事说在账上，不藏进字符串里。
    - 「有 `w:numPr`」不等于「是列表项」：本机 28 份真件的模板样式 `Subtitle` 里带一枚
      **没有 `numId` 的** `w:numPr`，按「看见 numPr 就算列表」去读，那 28 份的副标题全变成列表项。
    - 转义只在该转的地方转：表外的竖线照字交（`竖线 |`）、表里的补一个反斜杠（`尾格 \| 带竖线`）；
      一段普通正文的行首长得像结构记号时补一个反斜杠（`\# 这不是标题` / `\1. 这不是编号`），
      不然文件里写着的字会被读成标题与编号。图与链接的 target **按关系表写的原样交**
      （`media/image1.png` 是相对 `word/` 的），这一本不把图搬出来；实测 14 条链接的地址里
      没有一个含空格、括号或竖线，所以不转义 target 不会把链接截断。
    - **ODF 那一面另量一遍，读法整个换**（`md.odt` = LibreOffice 把 `md.docx` 转成 odt）：
      块数与两份 docx 一样是 18、段落与表格数一样，`chars` 是 388 —— 35 行渲染里**只有一行不同**，
      就是图片地址那一行（docx 的关系表写 `media/image1.png`，LibreOffice 在 ODF 里按内容哈希命名成
      `Pictures/1000000100000008000000088E4DF5D4.png`），两族都**按自己文件写的原样交**；
      粗斜不在 run 上而在 `text:span` 点的那份 `style:family="text"` 字符样式里，而那份样式
      可能在 content.xml 也可能在 styles.xml（先到先得，与编号那一条同口径），解不到就记
      `spans_unresolved` 而不猜形状（`md.odt` 三条 span 全解到，`styled-text.odt` 那 15 段就是这一跳的凭据）；
      空格、制表、换行是**元素**不是字（`text:s` 的 `text:c` 说几个），这一族没有
      `xml:space="preserve"`，所以句中两个空格被拆成「一个字面空格 + 一枚记号」，而后半句挂在
      **这个元素的尾**上 —— 第一版把尾当叶子跳掉了，那句只剩「两处空格 」；
      层级来自 `text:list` 的**嵌套深度**（docx 那边是 `ilvl` 那个数），列表样式名一律在 styles.xml
      的 `text:list-style` 上（这份语料 300 条、content.xml 里 0 条），所以这边记
      `lists_named` / `lists_unnamed`；批注（LibreOffice 写作 `office:annotation`）与注
      （`text:note`，`notes-end.odt` 里脚注两枚 + 尾注一枚）**就嵌在正文段里面**，它们的字整块跳过并
      各记一条数（`notes.odt` 那句「这里要补上不含税口径」在渲染里一个都不剩）；
      表格里 `number-columns-repeated` 是文件自己说了「顶几列」，照数展开（一条最多 64 列）。
    - 界：这一本走两族（OOXML 的 word 与 ODF 的 odt）。odp / ods / rtf / .doc / .pdf **不交这个键**
      （缺键 = 这一族还没搬，不是空文档，`notes` 里说一句）：演示稿的正文按页分、表格的「段」是格子、
      RTF 与 .doc 的层级根本不在同一套记号里，每一样都要另量一遍。OOXML 表格里被合并的格子
      （`gridSpan` / `vMerge`）也不展开、不补空格 —— markdown 的表格表达不了那个。


110. **文档里的公式：OMML 挂在段上、MathML 住在另一个部件，而「式子占不占一行」是生产者会改的**
    - OOXML 那一份每条交 `{index, paragraph, placement, host, align_written, structures, runs,
      nor_runs, lit_runs, text}`：`placement` 只按文件写着的挂法判（`m:oMath` 直接坐在 `w:p` 里
      是行内，坐在 `m:oMathPara` 里是独立成行），`structures` 按文档顺序交那一条用了哪些元素名
      （壳 `oMath`/`r`/`t` 不算结构），`text` 只拼 `m:t`，`align_written` 没写就是 null。
    - **LibreOffice 的 docx 重写把两条行内式升级成独立成行**（3 行内 + 3 独立 → 2 + 4），
      给四条都补上 `m:jc`（`align_written_total` 1 → 4），并且**把作者写的 `centerGroup` 换成
      `center`**；`m:nor` 那一条被补了一枚 `m:lit` —— 于是 `nor_runs` 与 `lit_runs` 两个数分开交，
      不并成一个「普通字」。而式子里的字一个都没动（`text_chars` 13、`math_runs` 12）。
    - ODF 那一面这条问话要**跳进另一个部件**：`draw:frame`（`text:anchor-type="as-char"`，
      `svg:width` 写成 `0.314cm` 这种自带单位的串）里 `draw:object xlink:href="./Object N"`，
      字在 `Object N/content.xml` 的 MathML 里，清单把 `Object N/` 声明成
      `application/vnd.oasis.opendocument.formula`；同一枚 frame 里还有一枚 `draw:image` 指向
      `ObjectReplacements/Object N` 的替位图 —— 两处地址都按写的交（少交一处就有一处没人认）。
    - **是不是公式要凭部件自己说**：`math_found` 数「那个部件里真读到 `<math>` 根」的条数，
      读不到根的单记在 `objects_without_math`（图表那类嵌入对象走的是同一扇 `draw:object` 门，
      不靠地址形状猜）。
    - **行内与独立在 ODF 这一族分不出来**：六条的 `<math>` 一律写 `display="block"`
      （`inline_written` 0、`block_written` 6），所以只交「按写的几个 block」。
    - **同一句话两族的「字」不一样长**：`[n]` 那一条在 OMML 里括号是 `m:d` 的属性
      （`m:begChr`/`m:endChr`），`<m:t>` 只有 `n`；到 MathML 里括号成了 `mo` 元素，于是是 `[n]` ——
      两份件 `text_chars` 因此 13 与 17，差的正是第 3、5 条。
    - **线性式另有存放**：那个部件的 `<semantics>` 里还带一枚 `<annotation encoding="StarMath 5.0">`
      写着 `{a} over {b}` 这种线性源，按原样交在 `annotation_source`，不算进式子里的字。
    - 从这个 odt 再回转成 docx（`eq-od.docx`）与那次 docx → docx 重写**账格不差**。
    - 反面凭据：`md.docx` 这一份一个式子都没有，交的是 `equations_total` 0（数过了没有）而不是缺键；
      `images.odt` 有 `draw:frame` 却没有 `draw:object`，于是 `frames_seen` 1 而 `objects_total` 0
      —— 页面上那张图那一本与公式这一本各数各的门。
    - 界：只数正文段（`w:p` / `text:p|h`）**直接孩子**里的式子，表格与注部件里的不数（两支同口径）；
      不做线性化（不生成 LaTeX）；rtf / `.doc` / `.ppt` 不交这个键 —— 那几族把式子内嵌成字段或
      对象是另一套记号，本机没有能写出这些件的生产者，量不到就不写那一支。

111. **放映里的公式：一条式子一个部件，而「页上有几枚 frame」与「有几条公式」是两个数**
    - ODF 那一族的式子内嵌成对象：`draw:frame` → `draw:object xlink:href="./Object N"` →
      `Object N/content.xml`（MathML），同 frame 里另有一枚 `draw:image` 指向
      `ObjectReplacements/Object N` 的替位图。两处地址**都按写的交**，替位图那一格还带
      `replacement_found`（这串地址指的部件在不在包里）。
    - **`deck.odp` 是这条 lane 的反面凭据**：它一条公式都没有（`objects_total` 0、`math_found` 0），
      可页上仍有 5 枚 `draw:frame`、注块里 3 枚、页缩略图 2 枚 —— 拿「frame 数」当「公式数」
      就会把一份没有公式的放映报成有五条。所以 `frames_seen` / `frames_in_notes` /
      `page_thumbnails` / `objects_total` / `math_found` 五格各数各的。
    - **是不是公式凭部件自己说**：`Object N/` 这一族既装公式也装图表，判据是那个部件里解析得出
      `<math>` 根（`math_found` 对 `objects_without_math`），不靠地址形状猜。
    - LibreOffice 重写同一份 odp 改的三格：`text:anchor-type` 全丢（2 → 0）、每页补一枚
      `draw:frame` 装 `draw:page-thumbnail`（挂在 `presentation:notes` 里）、frame 样式名
      `fr1` → `gr1`；式子的字与 `<annotation encoding="StarMath 5.0">` 的线性源一字未变。
      它还给第二枚对象写了 `./ObjectReplacements/Object 2`，而**这个部件既不在包里、清单里也没有**
      （`replacements_written` 1 → 2、`replacements_missing` 1）—— 引用与内容不匹配是文件的事实，
      报出来而不是替它补圆。
    - 生产者做不到的那半条也记着：**LibreOffice 没有 odt → odp 的导出过滤器**
      （`Error: no export filter`），所以 `eqs.odp` 只能手写外壳 —— 里面的两枚公式部件是
      LibreOffice 自己在 `eq.odt` 里写的 MathML 逐字抄的（外壳是我写的、部件是生产者写的）。
      第一次试的时候 `--convert-to pptx` 只留下 `.~lock…#` 与一个 0 字节 tmp，我差点记成
      「本机做不出」—— 真实原因是手写的第二枚 MathML 少了一个 `</msqrt>` 闭合标签；补上就稳了。
      **转换器不出件，先怀疑自己的件。**
    - 反向那一转（`eqs.pptx`）露出 pptx 的装法：既不是 `p:oleObj` 也不是 `p:graphicFrame`，
      而是**文本体里的 OMML**（`<a:p><a14:m><m:oMath …>`）外加一张 EMF。我第一版只 grep 了
      前者就下了「odp → pptx 把公式丢了」的结论 —— 那是「没找到」不是「没有」。
      pptx 那一族另起一本（下面事实 112）：它的式子既不是 `p:oleObj` 也不是 `p:graphicFrame`，
      而是**文本体里的 OMML** —— 我第一版只 grep 了前者就下了「odp → pptx 把公式丢了」的结论，
      那是「没找到」不是「没有」。
    - 这一族的 `objects_without_math` 有真实凭据了：`deck-chart.odp` 两枚 `draw:object` 的部件
      都在包里、都解析得开，可根不是 `<math>`（是图表），于是 `math_found` 0。
      **第二读者第一版把「解析得开」当成了「是公式」**，在这份件上报出 2 条公式，
      与 Rust（一直按 `<math>` 根判）在 CI 上对不上才暴露 —— 两份读者各自的盲点只有撞上
      有反例的件才现形，这也是这条 lane 要把图表件一起过一遍的原因。

112. **放映里的公式（pptx 那一支）：一条式子把同一个形状写两遍，一遍有字、一遍有图**
    - LibreOffice 的 pptx 把式子写成 `mc:AlternateContent` → `mc:Choice Requires="a14"` →
      `p:sp` → `p:txBody` → `a:p` → `a14:m` → `m:oMath`（字在 `m:t` 里）；同一个
      `AlternateContent` 的 `mc:Fallback` 里**那枚 `p:sp` 又写一遍**，`cNvPr` 的 id 与名字
      一字不差（`eqs.pptx` 两页各一枚：9/对象1、10/对象2 → `duplicated_shapes` 2），
      只是那一遍没有 `txBody`，改挂 `a:blipFill → a:blip r:embed="rId1"` 指向
      `ppt/media/imageN.emf`（`image/x-emf` 在 `[Content_Types].xml` 里有 Override）。
      三格 `fallback_blip` / `fallback_target` / `fallback_found` 就把「文件写的号、解出来的
      部件、部件在不在包里」各交一份。
    - **页级三格因此必须分开**：`eqs.pptx` 第一页 `shapes_total` 2、`paragraphs_total` 1、
      `formulas` 1 —— 拿形状数当式子数就会在 LO 那份上翻一倍。`alternates_total` 数的是
      页里**所有** `mc:AlternateContent`（连 `p:transition` 那枚也算，所以是 4），
      与「式子那一枚有没有 Fallback」（`fallbacks_written` 2）不是同一个 population。
    - 第二种写法由 python-pptx 手挂：`a14:m` 直接坐在 `a:p` 里、与正文 `a:r` 并列，
      没有外壳也没有替身图 —— `alternates_total` 0，三格 `fallback_*` 是 **null（这一族
      没写这一格）** 而不是 false；两条式子的字（`ab` / `12`）、结构名
      （`f`/`num`/`den`、`rad`/`radPr`/`degHide`/`deg`/`e`）与 `math_runs` 4 与 LO 那份
      完全一致：**差的是外壳，不是内容**。
    - 一条生产者边界（本机量的）：那份裸挂的 `eqs-pp.pptx` 转 odp 时 LibreOffice
      **把整条式子丢掉**（转出的件里 `draw:object` 0、没有任何公式部件），再转回 pptx 也只剩
      正文那一段字。所以这一支的第二个生产者只能停在「按写的交」，不能拿重写当凭据。
    - OMML 那一段的读法与 docx 那一本共用同一条排除表（`r` 与 `t` 是壳与字、不算结构，
      `m:nor` / `m:lit` 各数各的），两家都在 `equations.rs` 里同一个函数，不抄两遍。

113. **遗留 .doc 里的公式对象：`ObjectPool` 一物一 storage，正文流自己叫什么就交什么**
    - 97 的 .doc 是 CFB 容器：`eq.docx` 经 LibreOffice 那一转，六条式子变成 `ObjectPool` 下
      **六枚内嵌对象**（storage 名 `_2147483647` 起往下发号），每枚带 `\x01Ole`、
      `\x01CompObj` 与一条**正文流**。这一族的门不只通公式（图表、别的编辑器对象走同一扇门），
      所以判据用**正文流自己写着的名字**（`payload_stream` = `Equation Native`），
      并且 `objects_total` 与 `equations_total` 两格各数各的 —— 拿前者当后者就是猜。
    - `\x01CompObj` 后半是三枚「u32 长度 + ANSI 串」：`Microsoft Equation 3.0` /
      `DS Equation` / `Equation.3`。**这个含义不是猜的**：同一份件里 Word 自己的根写
      `Microsoft Word-Dokument` / `MSWordDoc` / `Word.Document.8` —— 两份件的三格形状一致，
      只是各自的名字不同。（第一版把头部读成「u8 类型 + 16 字节 CLSID」= 29 字节，
      三枚串全成 null；真实头部是 12 字节加 16 字节 CLSID = **28**。）
    - **字整个不在这一本里**：式子的字在 MTEF 二进制里，本机没有认得它的第二个读者，
      所以那一族**不交 `text` 键**（缺键而不是空串）—— 与 docx 那一边形成对照：
      同一份稿子 `eq.docx` 的 `equations_total` 也是 6，只是那一边的字在 `m:t` 里读得到。
    - 一处自证：**piece 表里正文的嵌入对象锚（U+0001）6 枚，容器目录里 `ObjectPool` 的孩子
      也 6 枚**。两格都交（`structure.object_marks` 与 `equations.objects_total`），
      读者自己去看它们合不合，账本不拿一格去圆另一格。
    - 没有 `ObjectPool` 的件交 `pool_found` false 与 0（`notes.doc` / `notes-en.doc` 都是）：
      数过了没有，与「这一族没看」是两件事 —— 后者是**缺键**。

114. **`.ppt` 这一族到不了公式那一步：转换把它变成一张位图，什么标记都不留**
    - 这一条是**量出来**的，不是「那族大概没有」。把已经有公式的 `eqs.odp` 用 LibreOffice 转成
      MS PowerPoint 97（`--convert-to ppt`），拿第二读者的 CFB 解析看容器：目录项一共 8 条 ——
      `Root Entry` / `\x01CompObj` / `\x01Ole` / `Current User` / `Pictures` /
      `PowerPoint Document` / 两份属性集 —— **没有 `ObjectPool`**（`.doc` 那一条路在这里整个不存在）。
    - 全文再扫一遍记号：`Equation` 0 次、MTEF 的头（`1c 00 00 00 02 00 c6 c1`）0 次、
      `Equation.3` 0 次、连 EMF 的签名都 0 次；`Pictures` 流只剩 916 字节的一笔位图数据。
      也就是说式子既没变成 OMML、也没变成内嵌对象，只剩渲染结果。
    - 因此 `office-slide` 的 `.ppt` 分支**不交 `equations` 这个键**：缺键 = 这一族没读，
      不是 0（`.ppt` 分支交的是记录树那一份账 —— 记录 / 容器 / 文字原子条数，按 0x03EE 归页）。
      这条缺键在 probe 里有断言盯着（3aw 的最后一条），哪天有生产者真的把式子写进 `.ppt`，
      断言会先红，再按七步配方补那一支。
    - 那份 462KB 的 `eqs.ppt` **没有进仓库**：里面 442,604 字节是 LibreOffice 写的那份
      `\x05SummaryInformation`（它自己把属性集撑大的），为一个负面结论押这么大一份件不值。
      量法与数字都记在这里，要复现只需一条 `--convert-to ppt`。

115. **`office-doc --csv`：一行就是文件自己写着的几格，而「这一行几格」是存储的数、不是页面上的数**（`tables.docx` / `tables-merged.docx` / `tables-merged.odt` / `md.docx`）
    - 表格铺成 CSV 是办公文件最常见的一个出口（把文档里的表搬进别的工具），这一本**不补方格**：
      文件在一行里写了几格就交几个字段。于是同一张视觉上 2×3 的表，两家给出的不一样长 ——
      `tables-merged.docx` 第一张 `[2, 3]`（`ragged` true、`covered_cells` 0，首行 `跨两列,第三列`），
      `tables-merged.odt` 第一张 `[3, 3]`（`ragged` false、`covered_cells` 1 而 `empty_cells` 1，
      首行 `跨两列,,第三列`）。差别不在谁读错了，在 OOXML 把横向合掉那一格**整个不写**、
      ODF 照样写一枚空的 `table:covered-table-cell`（事实 42 记的是那两句话本身，这里记它的 CSV 后果）。
    - **同一个输出串可以来自两条不同的话**：第二张（纵向合并那一张）两家的 CSV 串**一字不差**
      （`跨两行,右上\n,右下\n`），而 `covered_cells` 是 docx 0 / odt 1 —— OOXML 留着那一格、在它身上写
      `w:vMerge`（没写值就是 continue），ODF 把被盖住的那格写成占位元素。所以 `covered_cells`（文件写了占位格）
      与 `empty_cells`（这格没有字）各数各的，两个都不替另一个圆场。
    - 一格里几个段就用换行连着（`md.docx` 那格「这一格有 / 两段字」），进了 CSV 整格加引号、里面的引号翻倍；
      引法与 `office-sheet --csv` **共用同一个 `csv_field`**（一处规矩，两族同判据）。竖线不是引用触发符，
      所以 `尾格 | 带竖线` 是裸字段 —— 那些字符是数据，不是分隔符。
    - `--table` 只要从 0 起的序号、按文档顺序，不给就是第一张。两条 error 各说各的事：不是数的那句把收到的串
      原样回显（`--table 要的是从 0 起的序号，收到「没这个号」`），越界那句连「一共几张」一起给
      （`这份文件里没有第 9 张表（一共 2 张）`）。`columns` 是**最宽那一行**的格数，不是文件声明的列数 ——
      「这张表多宽」在 `table_layouts` 那三本账里（事实 56），这一本不替它答；`line_end` 说行尾只有 LF。
    - RTF 与遗留 `.doc` **不交这个键**：RTF 数得清 `\row` 与 `\cell` 却归不到某一张表（量过，事实 100），
      `.doc` 只有 piece 表里的格子标记（事实 113 那条边界）—— 两处都做不出这张 CSV，缺键而不是空串。
    - 第二读者是 `scripts/acceptance/lyco_doc_csv.py`（`docx_grids` / `odf_grids` / `doc_csv`，只用标准库）：
      表按 `descendants` 数、行与格按**直接孩子**走、嵌在格子里的那张表的段不算这一格，三条判据各写一遍。
      「段」那一层也照 Rust 的判据走（`.scratch/probe_csv_trim.py` 在 30 份有表的件上逐格量过一致）：
      每段**各自** trim、ODF 跳过 `text:annotation` 子树，而 `text:s` 与 `text:line-break` 在网格这一本
      **不展开**（展开是 `office-text --markdown` 那一族的事）—— 写在这里是为了将来真出现带记号的格子时，
      两边先在这里分道，而不是各自猜一个页面上的样子。
      probe 的 3ay 一条 lane 把**每一份 .docx 与 .odt** 的第一张表整份对账，再把每张表按号各取一遍
      （`tables_total` 与 `table` 两格），最后钉上面那四条实测串与两条 error 文案。
116. **`office-slide --csv`：一页一张表一份账，三种合并写法在这里两两分开**（`deck-tables.pptx` / `deck-tables-lo.pptx` / `deck-tables.odp` / `deck.pptx` / `deck.odp`）
    - 挑页有三层，失败的话也分三层各说各的：`--page` 收**放映顺序里从 0 起的序号**、部件名（pptx 那一族）、
      页名（odp 那一族），不给就是第一页。页挑不到说的是「这份放映里没有第 9 页（一共 2 页）」；
      页挑到了而那一页没有那张表，说的是「这一页（ppt/slides/slide1.xml）里没有第 1 张表（一共 1 张）」——
      两句话不互相顶。账里带 `page` 与 `page_index`，**错误那条也带 `page`**，不然不知道是哪一页说的。
    - 合并的**第三种写法**（事实 60 那一条的 CSV 后果）：pptx 把被盖住那一格照样留在文件里、在它身上写
      `a:hMerge` / `a:vMerge`（字是空的），ODF 另写一枚 `table:covered-table-cell`。于是同一张 3×3 的表在
      这三份件里都是 `columns_per_row` `[3, 3, 3]`、`covered_cells` 2、`empty_cells` 2，而**铺出来的串一字不差**：
      `"科目\n金额",,备注\n服务器,124000,含税\n"网络\n设备",8000,\n`。这与文档那一族正相反 —— 那边
      OOXML 把那一格整个不写，同一张表交回 `[2, 3]` 且 `ragged` true（见事实 115）。
    - `covered` 只问「这一格身上有没有那两条之一」，不问字空不空：`merge_written` 把文件写了哪一条交出来
      （hMerge 与 vMerge 不折成一个词），没有字的格子由 `empty_cells` 另数。
    - 表不住在第一页的那份件最能看出挑页要分层：`deck.pptx` 第一页（`ppt/slides/slide1.xml`）没有表，
      不给号挑到的就是它 → 交的是那句话而不是空串；`--page 1` 才拿到那张 2×2（`科目,金额\n服务器,124000\n`）。
      odp 那一支同一页的身份证是页名「第二页：数字」——这一族没有部件路径可指。
    - `tables_total` 数的是**这一页**几张表（实测这几份都是 1），不是整份放映几张；`cut` 与文档那一本同一条
      判据（被 `--limit` 截过的行/格不在网格里，这件事由它自己说）。
    - **「这一族没读」与「读了而没被要求」是两件事**：遗留 `.ppt` 那一族**整个不交这个键**（与它不交 `equations` 是同一个边界，事实 114），而 pptx 与 odp 读了这个开关 —— 不给 `--csv` 时那一格**在场而值是 null**。这条是 CI 量出来的：把它写成 `is_none()` 在那两支会红（文档那一族的 RTF / .doc 才是缺席，见事实 115 最后一条）。
    - 第二读者在 `lyco_doc_csv.py`：`pptx_grid` / `pptx_page_grids`（放映那一族自己走 `a:tr` → `a:tc`，
      一格的字仍按段拼再 trim）、`odp_page_grids`（与 .odt / .ods 共用同一个 `odf_grid`，合并那枚占位格同一口径）、
      `page_csv`（一页每张表一份账，按文档顺序）。probe 的 3az 一条 lane 把**每一份 .pptx 与 .odp 的每一页**
      按号与按名字各挑一遍（页序两读者先对齐才比），再逐张表整份对账，最后钉上面那三条实测串与两层失败。
117. **`office-text --markdown` 有了第三族：放映的大纲，条目标不标是文件自己说的**（`deck.pptx` / `deck-lo.pptx` / `deck-tables.pptx` / `-lo` / `deck-tr.pptx` / `deck.odp`）
    - 一页一个 `#`，标题取自那一族自己写的那句话：pptx 是形状的 `p:ph/@type=title|ctrTitle`。
      `deck-tr.pptx` 三页一个标题形状都没有 → `titles` 0、`titles_missing` 3，整篇**没有一行 `# `**
      （不替页编一个标题）。
    - **条目这一件事三家写得都不一样**，所以三本账分开：`deck.pptx`（python-pptx）连 `a:pPr` 都不写
      （84 码位、`bullets_written` 0、`bullets_silent` 2）；LibreOffice 重写同一份稿子时给两条写了
      `a:buChar`（87 码位、`bullets_written` 2、`bullets_silent` 0，`blocks` 都是 5）。
      两处相差 3 个码位 = 两个 `- ` 的标记 + 列表项之间不再空一行。
      `a:buNone` 是第四种情形（明说这不是条目 → `bullets_denied`），沉默的那一段不替它补标记。
    - 这一族的粗与斜是 `a:rPr` **身上的属性**（`b="1"` / `i="1"`，不是 docx 那种孩子元素），
      `a:br` 与 `a:tab` 是**段的直接孩子**（不在 run 里也要还原，不然一个字都读不出来），
      链接在 `a:rPr/a:hlinkClick/@r:id` 而地址在这一页自己的关系表里（两跳，各家 id 各编各的号）。
    - 页序有两本：正文那一份 `paragraphs` 按部件名序，markdown 这一本按**放映顺序**
      （`presentation.xml` 的 `sldId` 清单）—— 两处页序本来可以不一样，各按各的交，不折成一个。
      量的时候踩过一条：这一处关系表的 `Target` 是**相对 `ppt/`** 写的（`slides/slideN.xml`），
      只把 `../` 那种接上前缀，解出来的名字指不到任何部件，于是整份放映一页也读不到（两份读者各踩各的）。
    - 一张 `a:tbl` 走与 docx 同一条铺法（一格两段的 `<br>`、格子里的竖线才转义）：
      `deck-tables.pptx` 与 LibreOffice 那份的整本账**一字不差**（95 码位）。
    - 备注不进 markdown（那不是页面上给观众看的字），只交 `notes_pages` 数有几页带 notesSlide 部件；
      图也不进（`pictures` 数在那儿）。**odp 现在也走这一本**（见事实 118）：没搬的是 `.ods`
      / rtf / 遗留 .doc / .pdf —— 那一族这个键整个不在（缺键 = 没读，不是空文档）。
    - 第二读者是 `scripts/acceptance/lyco_deck_markdown.py`（`pptx_deck_markdown`：借 `lyco_markdown.py`
      的 `render` / `esc` / `rels_of`，段读者另写一份，因为那一族的记号是属性不是元素）；
      probe 的 3b0 一条 lane 把**每一份 .pptx** 的整本账与读者对，再钉上面这几条实测。
118. **同一本大纲的第四族：odp 把「这是一条」写在元素上，而两族的渲染可以一字不差**（`deck.odp` / `deck-tables.odp` / `deck-tr.odp` / `eqs.odp` / `deck-pictures.odp`）
    - 标题在**框自己身上**：`draw:frame`（或 `custom-shape`）的 `presentation:class=title`，取那一个框里
      头一段非空字。`deck.odp` 两页两标题；`deck-tr.odp` 三页一个 `class=title` 都没有 → `titles` 0、
      `titles_missing` 3、整篇**没有一行 `# `**（与 pptx 那一族的 `p:ph/@type` 是同一条判据的另一种拼法，
      不是一处代码）。
    - **条目是元素**：段住在 `text:list` > `text:list-item` 里就是条目，嵌套层数就是级别 —— 这一族没有
      `a:pPr/@lvl` 那样的层级属性，所以 `levels_written` 在这里量出来全是 0（那一格交的是「文件写了几个
      层级属性」，两家各按各的写法数）。
    - **两族同一份渲染**：LibreOffice 出的 `deck-lo.pptx` 与 `deck.odp` 同为 87 码位、5 块、整串相等；
      `deck-tables` 那张表两族也是同一份 markdown（95 码位、3 行）。可两本的其余账目各是一份，不互相补齐：
      odp 每页都写一块备注（`notes_pages` 2 对 pptx 的 1）、页上的图也多数一枚（2 对 1），
      而 `covered_cells` 那一格只有 odp 这本交（`deck-tables.odp` 2 —— pptx 那一族把合并写在 `a:hMerge` 上，
      这一本不数它，`--csv` 那本才数）。
    - **备注块不是第二张 `draw:page`**：`presentation:notes` 的孩子是 `draw:page-thumbnail` 加两个
      `draw:frame`（量过），所以按局部名数 `page` 只数到真页 —— `deck.odp` `pages` 2 / `notes_pages` 2；
      而 `eqs.odp` 两页**一个备注块都没有**（`notes_pages` 0：那一份的外壳是手写的，LibreOffice 没有
      odt → odp 的导出过滤器，见本表 `eqs.odp` 那一行），LibreOffice 把它重写成 `eqs-lo.odp` 之后
      `notes_pages` 变成 2 —— 同一份字，两家对「一页该不该有备注块」的答案不同，两本账因此分开。
      这一条判不住的话，同一问就有 2 与 4 两个答案。
    - `text:h` + `text:outline-level` 是这一族的标题形状，而**这批 12 份 odp 里一个都没有**
      （`headings` 与 `levels_written` 全 0）：0 是数过了没有，不是没看 —— 那一条分支在本仓库里没有生产者样本。
    - 表、嵌入对象、图与控件里的那几段**不再当页上的段交第二遍**（`table` / `object` / `image` / `control`
      整个子树不走进去）：同一句排两次是镜像先犯、两边一起对出来的。粗与斜那一跳、空格与制表记号的展开、
      批注跳过后单独计数，全部与 .odt 那一本共用同一条读法，`covered_cells` / `repeated_spans` / `links`
      三格也从同一个 `OdfRun` 里取 —— 一条 `text:a` 在这里数得到的就是那里数得到的那一条（`deck-links.odp` 3）。
    - 第二读者是同一份 `lyco_deck_markdown.py` 的 `odp_deck_markdown`；probe 的 3b1 把**每一份 .odp**
      的整本账与读者对，再钉上面这几条实测。
119. **目录里那几条「排出来的」条目：先前那条「生产者做不出」是一条假阴性，宏这条路走得通**（`toc-full.docx` / `toc-full.odt` / `toc-full.rtf`）
    - 为什么先前判错：`write_toc_seed` 那一份注壳用的种子**既没有真标题也没要求更新域**，
      于是 LibreOffice 转换后目录里只剩我们注进去的那一句占位 —— 那只能证明「LO 保留域指令」，
      证明不了「LO 会不会自己排」。补上正控制（`md.docx` 有 Heading 1 / Heading 2、
      `settings.xml` 里写 `w:updateFields val="true"`）再转一次：`--convert-to` 仍然只照抄，
      **PAGEREF 一条也不写**。也就是说转换器不重排索引，这句是量出来的。
    - 走得通的那条路：临时 profile（`-env:UserInstallation`，不碰用户自己那份配置）里放一个
      Basic 宏 —— 装载 → `getTextFields().refresh()` → 每条 `getDocumentIndexes().update()` →
      `storeToURL` 两次（docx 与 odt）。顺序有一条坑：**先让 profile 自己建起来再写宏文件**，
      反了会被 LibreOffice 退出时盖回它那份 `script.xlc`，于是宏静默什么也不做（第一次就踩了）。
    - 排出来之后两家把同一件事写在三处：条目文字与页码之间都是**一枚制表记号**（不是域），
      页码是 `1` / `2` 那样的**字面串**；OOXML 的地址在 `w:hyperlink/@w:anchor`（只有名字），
      ODF 在 `text:a/@xlink:href`（带 `#`，照写）；级别 OOXML 写在段自己点的样式名上
      （`TOC1` / `TOC2`），ODF 段上只有 `P1` / `P2` 这种自动样式名 —— **那两个号不是级别**，
      所以 ODF 的级别顺锚点两跳去读被指那段 `text:h` 自己写的 `text:outline-level`，
      每条都带 `level_from` 说清这一级是从哪来的（`paragraph-style` / `target-outline-level`）。
    - 容器也不同：`w:sdtContent` 把「目录」那一行标题当**直接孩子**写（`paras` 3 而 `entries` 2），
      ODF 把标题嵌在 `text:index-title` 里面（`paras` 2 == `entries` 2）—— 同一问两个答案，
      所以两本账一起交，不折成一个数，也不替谁判「这一段算不算条目」。
    - 两处读法坑，都是量的时候撞出来的：`w:pPr/w:tabs/w:tab` 是**制表位定义**（到 8639 右对齐、
      点线引导），把它当段里的记号就会在第一个字之前先撞到一个，整条条目被误判成「制表符之后」；
      ODF 的页码挂在 `text:tab` **之后的一段裸文字**上（ElementTree 里就是那个元素的尾），
      只收元素的直接文字就一个字也读不到 —— 与批注那一条是同一个教训。
    - 第三个生产者差别留在那儿不合并：同一个二级标题，ODF 那边它自己点的样式叫 `P3`
      （而一级那条叫 `Heading_20_1`）—— 照文件交，不替它改成看起来该叫的名字。
      RTF 这一族这轮**不交 `entries` 这个键**（缺键 = 这一族没读）：它的条目在结果群里、
      级别在段前那个 `\sNNN` 上，两件事都另有读法，量到的形状已写进上面那一行表。
    - 第二读者是 `scripts/acceptance/lyco_toc_entries.py`（两份 `toc_entries`，
      挂在 `office_reader.py` 的 `docx_contents` / `odf_contents` 上）；probe 的 3b2 把
      **每一份 .docx 与 .odt** 的整本条目账与读者对，再钉上面这几条实测。

120. **一张方格两个出口：`--markdown` 与 `--csv` 用的是同一份铺平，转义按 markdown 的规矩**（`pipes.xlsx` / `pipes-lo.xlsx` / `pipes.ods`）
    - 为什么另做一副件：`--markdown` 有两个分支（竖线躲成 `\|`、格内换行铺成 `<br>`），
      量过 212 份存量 fixture 之后确认**整库里没有一格带竖线**（`office_reader.py` 铺平再用
      `csv` 模块解回来数过），换行那一支只有 `rich*` 三副有。没件可测的分支不能说它成立。
    - 两本账同源：`square()` 只算一次方格，`render_csv` 与 `render_markdown` 各自转义，
      所以 `rows` / `columns` 在两边是同一个数（探针 3f1 对每一份都 assert 这条等式，
      并且与第二读者的 `rows` / `columns` 三方对）。
    - `separator_after_row` 说的是一句格式事实：`| --- |` 加在第 0 行之后，因为 markdown 的
      表格**必须有表头**；文件没说第一行是表头（`book.ods` 的「草稿」只有一行字，那一行照样
      被当成表头），所以这一格叫「分隔线在第几行之后」而不叫「表头」。
    - 三家生产者到这一个出口上合上：`pipes.xlsx`（openpyxl，行内/串表两条路）、
      `pipes-lo.xlsx`（LibreOffice 重写 xlsx）、`pipes.ods`（ODF 把换行写成三个 `text:p`）
      铺出来的 markdown 全文逐字相同 —— 这是这一批里唯一一条「三族同文」的等式，
      别的账都是各交各的。
    - 第二读者是 `office_reader.py` 的 `square()` / `md_render()`（与 `csv_render()` 同一份方格）；
      probe 的 3f1 把 14 份表格件的整本 markdown 账与它对，再钉那三副 pipes 的转义与「躲过的竖线不被当分列符」这一条（把 `\|` 收回占位再切列，那一行仍是两格）。

152. **图的 chartSpace 那一层：图例在不在、空值怎么办、标题删没删，八张图四件对照**
    （`chart.xlsx` 与 `chart-lo.xlsx`、`deck-chart.pptx` 与 `deck-chart-lo.pptx`；这四个部件是两族共用的
    同一套 `c:` 词汇，所以两家同一个形状交）
    - `c:legend` 只在表格那一路在场：两份 xlsx 各两张图都写图例且都写 `c:legendPos val="r"`，
      而两份 pptx 的四张图**一张都不写**（python-pptx 不写，LibreOffice 的 pptx 导出也不写）。
      「图例没有」与「这一族不写图例」在账里是同一格 `legend_found=false`，
      靠它旁边的 `groups[].written` 与系列数才分得开。
    - LibreOffice 存 xlsx 时多写两枚：`c:overlay val="0"` 与 `c:autoTitleDeleted val="0"`；
      openpyxl / python-pptx 那一路 `overlay` 不写 —— 于是「图例叠不叠」有 `0` 与「没写」两种形状，
      而 ECMA 的默认（不叠）是规范的话，不替文件填。
    - `c:autoTitleDeleted` 是**逐图**的而不是逐件的：LibreOffice 重写的那份 pptx 里，
      同一页那两张图一张写 `1`、另一张写 `0`（`deck-chart-lo.pptx` 第一页）—— 所以这一格按每张图
      各交一值，绝不折成「这份稿子删过标题没有」。
    - `c:dispBlanksAs` 八张全写 `gap`，本库没有一件写 `zero` 或 `span`；交的是文件写的那个词，
      不换算成「空值当洞/当零/连线」三种说法。
    - 两枚读错过位置才找到的：`c:varyColors` 坐在**绘制组里**而不是 chartSpace 层（第一版按
      chartSpace 读，八张全交 null —— 那是读错层交出来的零，已由 `groups[].written` 交过，
      这一层就干脆不放这一格）；`c:delete` 那四枚在**坐标轴**上，图例里一枚都不写，
      所以 `legend_delete` 八张全 null 是实测而不是没读。
    - 本机做不出、因此这一本里根本没有那一格的：`c:multiLvlStrRef` / `c:multiLvlStrCache`
      （两级类别）。真件普查里它出现在 `ppt/charts/chartN`，而 python-pptx、openpyxl、
      LibreOffice 三家都不写它 —— 没有可复现生产者就不开那一格，也不拿单级缓存冒充两级。

151. **单元格样式自己那两枚锁定位：三个容器、两种拼法，而「表锁了」与「格式设了锁」是两件事**
    （`cell-locks.xlsx` 手写、`cell-locks-lo.xlsx` 是 LibreOffice 重写同一份；普查 = 45 份自产 .xlsx 的聚合，
    外加真件里已有的 `locked-sheet*.xlsx` 那一对）
    - `protection` 在 `xl/styles.xml` 里不是容器的孩子，而是 `xf` / `dxf` 的孩子：所以账按
      `cellStyleXfs` / `cellXfs` / `dxfs` 三本各记一笔（在场与枚数分交），另交「第几个格式带着它」的下标。
      第一版把它当容器直子读，四十一份全交零枚 —— 结构要量，不能照记忆写。
    - 整库 45 份 .xlsx 里 21 份写这一层、共 82 枚：真件那 43 份中 19 份写、共 66 枚，全出自 LibreOffice 那一路，
      openpyxl 那 24 份一枚都不写（另 5 枚与 11 枚来自本仓那两份手写件与它的重写）；写着的每一枚都同时交
      `locked` 与 `hidden`，真件里值只有
      `locked="true"` / `hidden="false"` 这一对（LibreOffice 把 ECMA 的默认也逐条写出来）。
    - 拼法**按层量**：同一族在 `xl/worksheets` 的 `sheetProtection` 写 `1` / `0`，在这一层只写
      `true` / `false`。合成件因此专写 `locked="1" hidden="0"` 与 `locked=""` 与一枚空元素，
      让「1/0」「写了名而值空串」「在场而无属性」三形各有凭据；LibreOffice 重写这一份时把
      `1` 与 `0` 换成 `false`、把空元素整枚丢掉、把那枚认不出的 `lockRule` 也不留。
    - `dxfs`（条件格式那本）在 6 份件里在场而**一枚都不写** —— 「有这本而零枚」是实测形状，
      与「这一族没这一层」分别是两句话；后者由反面凭据交：`.ods` / `.xls` / docx / pptx 四家
      的整份输出里根本没有 `cell_locks` 这个键（ODF 的锁只在 `table:table` 那一层，
      `.xls` 的位在 BIFF 的 `XF` 记录里，那是 `protection` 那一本按表交的 PROTECT）。
    - 与 `protection` 那一本（文档级 / 表级锁）分家：`locked-sheet.xlsx` 的表是锁着的而这一层
      一枚都不写；LibreOffice 重写同一份时补成四枚。表没锁时这些位不生效，所以「这份表能不能改」
      要两本一起看，任何一本单独交出去都只说对一半。

150. **中文排版那几枚段开关的三处住处，与「裸写 / 空串 / 枚举值」的三种说法**（`cjk-switches.docx` 手写、
    `cjk-switches-lo.docx` 与 `cjk-switches.odt` 是 LibreOffice 重写的同一份、`cjk-odf.odt` 手写 ODF 那一头、
    `cjk-odf-lo.docx` 是它转回 docx；普查 = 本机真件聚合，只留数、不外带文件名与正文）
    - 九枚写在 `w:pPr` 底下：`kinsoku`、`wordWrap`、`overflowPunct`、`autoSpaceDE`、`autoSpaceDN`、
      `adjustRightInd`、`snapToGrid`、`contextualSpacing`、`textAlignment`。同一枚也可能只写在段点的那份样式里，
      或写在 `docDefaults` 那一处，所以三处都按原样交，不折成「这份文档开没开中文紧凑」那一个数。
    - 真件普查（129 份真件 docx/docm）：`contextualSpacing` 108 份写、正文 2868 + 样式 1473 枚，而 4341 枚
      **全是裸写**（没写 `@w:val` —— 规范里那算真，不是「没值」）；`snapToGrid` 12 份 788 枚里值 `0` 占 597；
      `autoSpaceDE` 19 份（裸 229 / `0` 43 / `true` 22）；`wordWrap` 360 枚正文里有一枚拼成 **`off`**；
      `textAlignment` **不是布尔**，只出现 `auto` 与 `baseline`；`noProof` 148 枚**全部**住 `w:rPr`（所以它在
      `run_no_proof` 那一本而不是段上九枚里）；**`docDefaults` 一处都不写**这九枚 —— 这一枚只能合成，所以
      真件上 `defaults_written` 恒全 0 是一句实测话，而不是一条规范话。
    - ODF 那一头只有四枚近亲（64 份真件 .odt）：`contextual-spacing` 1841 枚（false 1085 / true 756）、
      `line-break` 恒 `strict` 123 枚、`punctuation-wrap` `hanging` 69 / `simple` 3，而 `snap-to-layout-grid`
      与 auto-space、adjust-right-indent、text-align-last **一条都没有**。跨族只核对名字不折算语义
      （`kinsoku`↔`line-break`、`overflowPunct`↔`punctuation-wrap` 是近亲不是同义）。
    - LibreOffice 同格式重写动得最狠：`kinsoku` / `wordWrap`（含那枚 `off`）/ `autoSpaceDE` / `autoSpaceDN` /
      `adjustRightInd` / 字侧 `noProof` **六处整个不再写**，`docDefaults` 那枚也不留；`overflowPunct` 两枚都
      改写成 `false`；`snapToGrid` 的「裸 / 0 / 1」换成 `true`、`false`、`true`；`contextualSpacing` 显式关掉的
      那一枚不见了（裸写的两枚留着）；只有 `textAlignment` 的三个枚举值**一字不动**穿过。
    - 反方向只搬得动四件事（手写 .odt 转回 docx）：`kinsoku=true`、`overflowPunct`（true 与 false 各一枚）、
      `snapToGrid=false`、裸写的 `contextualSpacing`；其余五枚全 0。docx → ODF 那一转把 `wordWrap="off"` 换成
      `fo:wrap-option="no-wrap"`（**这一族不读那一格**，它归排版兼容那本），并把 `contextual-spacing` 逐段补满
      （20 段里 15 段有，false 11 / true 4），`line-break` 那两枚写在没人点用的样式上，所以按段解出来是 **0 枚**。
    - 自产件的语料级一句话：段上写着九枚之一的只有 3 份（全是本仓手写的），而**样式那一跳**有 13 份能拿到
      （python-docx 那份模板的样式自己写满 `kinsoku`/`overflowPunct`/`autoSpaceDE` 各 44 段）——
      「段上没写」与「这一族不写」是两件事，与事实 149 那条同一个讲法。

149. **这一本工作簿自己的设置：谁存的、算不算、打开停在哪一张 —— 而这三问两家的答案几乎不重合**。
    `xl/workbook.xml` 顶上那一排元素不在任何一张表上，所以也不在任何一份按表的账里；
    `workbook_settings` 对每一枚同时交「元素在不在场」与「它写了哪些属性、值原样是什么」。
    - **空壳是一句说过的话**：openpyxl 写一枚 `<workbookPr/>`（在场而一个属性都没有），
      语料 41 份 xlsx 里这样的空壳有 **21 份**；LibreOffice 重写时把它填成三格 —— 于是
      `empty_elements` 与 `attrs_written` 各记一本，合并就把「生产者没说话」说成「生产者是 false」。
    - **同一份件里两种布尔拼法**：手写那份 `backupFile="1"` 与 `autoFilterDateGrouping="false"`
      并存，`boolean_spellings` 就是数这个的（语料里 `date1904` 出现三种拼法：`false` / `true` / `1`，
      19 份写了这一格）。
    - **「最后一版是谁存的」在两处**：`fileVersion` 的 `appName` 与那三个数字。语料 41 份里
      **19 份有 `fileVersion`、枚数之和 20**（真 Excel 会补一枚 `GenuineMicrosoftOffice`），
      而 `lastEdited` / `lowestEdited` / `rupBuild` 只有 9 份真件写 —— openpyxl 与 LibreOffice
      都不写；同一份 `workbook-settings.xlsx` 手写两枚，LibreOffice 重写后**合成一枚**、
      `appName` 换成 `"Calc"`、两个数字丢一个。
    - **`calcPr` 两家零重合，而且同一格两个意思**：openpyxl 那 22 份只写 `calcId` 与
      `fullCalcOnLoad`，LibreOffice 那 19 份反过来写 `iterate` / `iterateCount` / `iterateDelta` /
      `refMode`；`refMode` 在 MS-XLSX 里是迭代引用的行 / 交叉模式（`row` / `crossSheet`），
      LibreOffice 在同一格写的是它自己的公式语言记号 `A1` —— 语料里三种值（没写、`A1`、`row`）
      都在，所以两边按原样交而**不换算也不判**。`iterate` 写了 19 份而值等于 `true` 的只有 2 份：
      「迭代计算开没开」这一问，多数件是「文件说了关」。
    - **「打开停在哪一张」是这层里少数两家人都认的**：`workbookView/@activeTab` 与
      `firstSheet` / `tabRatio` 从手写件原样穿过 LibreOffice 的重写，而窗口四格的值被改、
      `visibility` / `minimized` / `autoFilterDateGrouping` 整枚不见。
    - **ODF 与 .xls 不交这一格**：`.ods` 没有这三枚元素，同类问题在 `settings.xml` 的
      `ooo:configuration-settings` 那一组（15 份 `.ods` 全写 `AutoCalculate` 与 `SyntaxStringRef`，
      而迭代计算那三格**一份都不写** —— 转格式丢掉的事。同一趟还量到两件「改掉」的：`AutoCalculate` 14 份写 true 而这本转出的那一份写 false（openpyxl 那本写了 `calcMode` manual，转换认它），而 `workbook-settings.ods` 比另外 14 份**多一整格** `CodeName`（值 `ThisWorkbook`，从 xlsx 的 `workbookPr/@codeName` 搬来），那一组因此是 40 条而不是 39 条 —— 「恒 39 条」这句在这份件之后不再成立，排版兼容那本（事实 133）与探针里各有一句同一说法也跟着改，那一组的形状与条数由排版兼容
      那条账（事实 107）交代；`.xls` 把计算模式记在 BIFF 的 `DBSTAT` / `CALCCOUNT` 里，
      本机没有第二个读者能核对那些字段偏移，所以不读。缺键 = 这一族没这一层。

148. **这一段是第几级：级可能写在三处，而真件最常写的那一处是样式的名字，不是段上那串号**。
    Word 不说「这是标题」，它给段点一个样式号（`w:pStyle/@w:val`），而「第几级」有三个住处：
    段自己的 `w:pPr/w:outlineLvl`、被点名的那份样式的 `w:name`（`heading 3` / 「标题 #1」）、
    那份样式自己的 `w:pPr/w:outlineLvl`。`outline_levels` 把三处都交，`level` 只按一条写死的优先序
    取一个（**段 > 样式名 > 样式自己的那个数**），两处打架的那些段由 `conflict` 说。
    - **这一本存在的理由是现成那一本的漏**：`headings` 只拿段上那串**号**比 `heading`/`标题` 前缀，
      而真件把号写成 `1`/`2`/`3`/`21`/`31`/`15` 这样的数字，级在样式名上 —— 本机 33 份真件
      （排掉仓库自产件）12525 段里 **701 段有级**，旧那一本只认到 **290，漏 411 段（59%）**；
      701 段的级**全部**来自样式名，真件里**没有一段**自己写 `w:outlineLvl`。
      同一份 `levels.docx` 上两本账各说一个数：`headings` 0 条，而这一本 `resolved` 7 ——
      旧那一本不是「这份没有标题」，是「这一处它没去看」。
    - **`w:outlineLvl w:val="9"` 不是第 10 级**：ECMA 那一格 0..8 才是九级大纲，9 是「正文本身」。
      所以写着 9 的那一行交 `level: null` 而 `level_from` 写「那是正文」—— 本仓唯一一处**不照着数加一**
      的地方；自产件两份各测一枚（段上一枚、样式上一枚，`body_written` 2）。真件那一趟只量到「级从样式名来」
      这一种来源（701 段全是），段自己写与样式自己写 `outlineLvl` 这两形本机真件**一段都没有** ——
      那两形的凭据是自产件，别把它说成真件分布；
    - **断链就在自产件里**：普通件 `notes.docx` 的 3 段里有 1 段点着样式表根本没有的号（整库摊开那份账：
      92 份自产件里 `style_missing` 合计 10）。所以 `style_found` 与 `style_missing` 各交一笔，
      那一行的 `level_from` 是 `"(没说)"`，而不是把它默认成「普通段」——LibreOffice 重写时会替它补一个
      `Normal`（`style_missing` 1 → 0），那是**生产者替文件做主**，不是文件自己写的。
    - LibreOffice 重写那一份动六处：号换成可读 id（`Heading1`/`CustomText`/`emptyoutline`/`TOCHeading`）、
      每段都点上样式（`with_style` 9 → 12）、断链修成 `Normal`、写着 9 的那两枚 `outlineLvl` **整个丢掉**
      （`body_written` 2 → 0 —— 它认为正文就是不写）、样式里 `@w:val=""` 那枚**替文件补成 `0`**
      （那一段于是从「写了但没值」变成第 1 级）、而 `TOC Heading` 那份样式的级也没了（两处都不说）。
      号换成可读 id 之后，旧那一本反倒认到 4 条 —— 同一份文档，认得多少取决于生产者怎么写号。
    - 转成 .odt 换一种说法：那 7 段有级的成了 `<text:h text:outline-level="N">`（1 两枚、2/3/4/5 各一枚），
      **「是标题」写在元素名上而不是任何属性上**，级才在 `@text:outline-level`；其余 5 段是 `text:p`。
      这一族的 `outline_levels` 只住 OOXML：odt / rtf / 遗留 .doc 不交这一格，级由 `headings` 那一本答。
    - 空串与「没写」是两句话：`@w:val=""` 在场而不算数，那一行交 `style_written: ""` +
      `level_from`「样式写了但没值」；整枚元素不在场才是 `null` +「(没说)」。数字也只认 **ASCII** 的，
      全角数字与混着别的字符都算「写了但没说数」（两份读者同一口径）。
    - 截行不截账：`--limit` 只砍 `rows`（`listed`/`cut` 说这一格），十一本合计与那两张分布表
      （`levels`/`froms`）恒按全部段数算。整库摊开（自产 92 份 docx/docm）：段数之和 527、
      算得出级的 75、名字像标题的 70、段自己写级的 5、两处不一致的 4、写着 9 的 4、断链的 10。
    - 第二读者是 `office_reader.py` 的 `docx_outline_levels` / `ascii_int`（92 份 docx/docm 逐份影子跑过，
      与 33 份真件的普查各自独立）；probe 的 **3db** 段逐件对整本账，再钉 `levels.docx` 的十一种形状、
      LibreOffice 那六处、`--limit 3` 那一格、两本账在同一份件上的 0 与 7、整库摊开那十本与反面凭据那 8 格；
      Rust 那侧是 `office_doc.rs` 的 `the_level_of_a_paragraph_can_come_from_three_places`，
      模块是 `lilyco-binfmt/src/outline_levels.rs`。

147. **这一层可以填：一枚 `w:sdt` 说自己是什么，而「是什么」写在它 `w:sdtPr` 的孩子里，不在它自己身上**（三份 `sdt*` + 本机 33 份真件 docx/docm 与自产 90 份各算一本，真件只量数、不入库）
    - 形状：每枚一行 35 格 `{part, index, depth, pr_present, pr_children, type_seen, type_count, alias,
      alias_present, tag, tag_present, id, id_present, lock, lock_present, placeholder, placeholder_present,
      showing_plc_hdr, data_binding, date, list_kind, list_items, list_values, doc_part_obj, gallery,
      endpr_present, content_present, content_children, paras_direct, paras_total, tables_direct, tables_total,
      cells, runs, chars}`；整本 26 格 ＝ 16 本合计 + `parts_with_controls` + 四张分布表（`children`/`types`/
      `galleries`/`locks`）+ `family`/`available` + `rows`/`listed`/`cut`。三个名各交两格（`alias` 是值、
      `alias_present` 是在场）——「没写」「写了空串」「写了值」是三句话，只交一个 `alias: ""` 就把前两句混成一句。
    - **真件普查（本机 33 份真件 docx/docm，排在仓库之外）：只有 4 份写了这一层、各一枚，共 4 枚** ——
      而这四枚的形状与自产件完全不同：**四枚全住 `word/footer1.xml`（正文里一枚都没有）**、全部没写类型元素、
      全部带 `w:sdtEndPr`、全部写了 `w:id`，而 `w:alias` / `w:tag` / `w:dataBinding` / `w:lock` / `w:placeholder`
      **真件零枚写过** —— 「Word 自己导出的控件长这样」与「模板里那块目录长这样」是两种形状，所以
      `type_none` 与 `endpr_present` 分列，而不是合成一句「完整吗」。
    - **自产件是另一本账（90 份 docx/docm，6 份写了、共 25 枚，全在 `word/document.xml`）**：
      类型分布 `text` 9、`richText` 3、`docPartObj` 4、`dropDownList` 2、`date` 2、没写类型元素 5，
      `alias` 19 枚（其中 2 枚是空串）、`id` 写了值 20 枚、`lock` 2 枚都写 `sdtContentLocked`、
      `dataBinding` 4 枚（`prefixMappings`/`xpath`/`storeItemID` 各 4、`storeSchemaID` 2）——
      这些数**只属于自产件**，别拿它们当真件分布说话。
    - **住址这一条是真件给的，不是自产件给的**：真件那 4 枚全在页脚部件里，只顺着正文那一路走会**一枚都不剩**，
      所以这一族扫 `word/*.xml` 全部部件并按件名排序。（这一趟先前写成「本机 162 份、14 份带控件、共 42 枚」，
      那是**把仓库里的 fixture 也算进了真件分母**——`D:\Code` 之下本来就含 `tests/fixtures/office`；
      按「件是否住在那一目录之下」分开重扫才是上面这两个数。教训与「扫不到要先怀疑判据」同一条：
      **分母混了自产件，比例与分布就都成了自产件的画像**。）
    - LibreOffice 同格式重写那一份动了七处，每一处都是「说了别的话」而不是「没说」：11→10（正文空着的那枚**整枚不见**，
      于是「这一层有几枚」也变了）、`w:sdtEndPr` 那两枚全丢（`endpr_present` 2 → 0）、`richText` 降级成 `text`、
      `w:alias` 被改写成**空串**（不是没写，而 `alias_present` 仍然 true）、同一枚的 `w:id` 于是没了、日期那枚把
      文件自己写的两格（`dateFormat="yyyy年MM月"`、`calendarType="chineseLunar"`）**换成它自己算的一格**
      `fullDate="2026-09-27T00:00:00Z"`（那三格本来就互斥，谁写交谁）、绑定那枚的孩子**换了序**（`dataBinding` 从第四
      挪到最后，而它自己的四格属性只剩三格 —— `storeSchemaID` 被丢掉）、套娃外层**失去类型元素**。另外把段摊平成 `w:r`：
      正文两个口径都变 3 而 runs 23。还有一处只在看两本账对账时才看得见：那张表**还在整件里**（`structure.tables` 仍是 1、
      部件里 `w:tbl` 一枚没少）却**已经不在这枚控件里**了（控件那一层 `tables_total` 1 → 0、`cells` 2 → 0）——
      控件被摊平时住在里面的表跳回了正文，两本账各答各的问，拿其中一本当另一本会得出「这份没有表了」那种错话。
    - **ODF 那一转要说准**：标准那一层确实没有这个名字（`<form:` 零枚），但 LibreOffice 把那 10 枚中的 6 枚写进了自家
      扩展命名空间 `loext:content-control`（五个名 `loext:alias`/`id`/`tag`/`lock="sdtContentLocked"`/`plain-text`，
      **一个类型都不写**），控件里的字与段落都还在（`text:p` 14）。本仓的 odt 读者不读 `loext:`，所以这一族对 ODF
      交出的是「没有这一格」，而**不是**「这一层不存在」—— 那两句的区别由 3ca 最后一条钉住。
    - 模板里那块目录本身就是一枚控件（`w:sdt` + `w:sdtPr/w:docPartObj/w:docPartGallery val="Table of Contents"`，
      `toc.docx` 交 1 枚），而「这份有没有目录、排出来几条」是 `structure.contents` 那本账 —— 两问分开交：
      拿这一枚当那一条会得出「有目录但没有条目」这种看着矛盾其实都对的答案（`toc.docx` 正是：控件 1、条目 0）。
    - **一条更正的历史（两次都值得记）**：`custom_xml` 那条事实（138）与 `customxml.docx` 那格原本记着
      「正文那两条手指（`w:customXml` 与 `w:sdt`/`w:dataBinding`）本机真件 0 份写过」。本趟普查先把这句当成假负
      改掉了，理由是「逐份重扫量出 6 份写了」——**那句“改掉”才是错的**：那 6 份全在仓库的 fixture 里
      （`sdt.docx` / `sdt-lo.docx` / `customxml*.docx` 那一类），排掉 fixture 之后**真件 33 份里 `w:dataBinding`
      与 `w:customXml` 确实一枚都没有**，所以那两处已按分开的口径改回来，并把「真件 0 份 / 自产 6 枚」两件事
      分开写。教训是**同一句话的两次改写都错在同一步：没先问「这批件是谁写的」**——真件与自产件混在一个分母里，
      既会把自产件的形状当成真件的分布，也会把「扫到」当成「真件写过」。
    - 截行不截账：`--limit` 只砍 `rows`（`listed` 与 `cut` 说这一格），16 本合计与四张分布表恒按全部枚数算
      （`--limit 3` 那一条钉的就是这个：列出 3、`cut` true，而 `controls` 仍是 11）。
    - 两条边界都在那三份件上量过（真件里 `w:date` 一枚都没有，所以这一句只能说「两个生产者各写一种」）：
      **`w:date` 那三格两种写法都真** —— 手写那枚写成属性（`dateFormat="yyyy年MM月"` + `calendarType="chineseLunar"`），
      LibreOffice 那枚既写属性 `fullDate` 又写孩子（`dateFormat` / `calendar` / `lid` / `storeMappedDataAs`），
      本仓只交属性那一本，所以后一种写法的 `date` 看着只剩一个数；`w:listItem` 的 `@w:value` 与 `@w:displayText`
      是两格（`list_values` 只交值，而 LibreOffice 重写改的正是 `displayText`：`甲选项` → `甲`，值一字未动）。
    - 第二读者是 `office_reader.py` 的 `docx_content_controls` / `sdt_row`（90 份自产 docx/docm 逐份影子跑过，与那 33 份真件
      的普查各自独立）；probe 的 **3ca** 段逐件对整本账，再钉三份件的整本、`--limit 3` 那一格、模板那枚目录控件、
      一份没有控件的件（`available` true 而 16 本全零 —— 「没有」与「没读」两件事）、整库摊开那 11 本与反面凭据那 8 格；
      Rust 那侧是 `office_doc.rs` 的 `a_content_control_says_which_kind_it_is_and_how_much_text_it_holds`，
      模块是 `lilyco-binfmt/src/content_controls.rs`。

146. **这份稿子有多少字：`docProps/app.xml` 自报的七个名与正文实算的三个口径并排，谁也不盖谁**。
    「实算」交三份：只数 `ppt/slides/slideN.xml`、页 + 备注、包里所有带 `a:t` 的部件（版式与母版里也有字）——
    生产者到底数了哪些部件，文件里没写，所以三个口径都交、三个各自打等号，不替它挑一个。
    - **真件普查（本机 104 份 pptx，102 份带 `docProps/app.xml`）：数件数的都对、数字数的都不对** ——
      `Slides` 与页部件数对 101/102、`Notes` 与备注部件数对 102/102，而 `Words` 与三个口径的实算切词
      **96 份可比 0 份对**、`Paragraphs` 与 `a:p` 条数 **98 份可比 0 份对**；另有 78 份 `Words` 写着 `0`
      而正文有字（103/104 份的页部件有字）——「自报 0」是「没数过」，不是「这份没有字」；
      2 份整件没有 app.xml（那种件的这一格是 null），6 份带了 app.xml 却没写 `Words`。
    - **版式与母版里有没有字是生产者差异，不是文档差异**：真件只有 3/104 份有，而 python-pptx 那份模板
      给每份自产件写了 12 个部件、1369 个字符，于是「按整包实算」在自产件上是正文的二十倍（1440 对 61）。
    - LibreOffice 重写同一份 pptx 时把**手写的** `Words`（999999）与 `Paragraphs`（7）原样搬过去，
      却把 `Slides` / `Notes` / `HiddenSlides` / `MMClips` 四个名整个丢掉 —— 那两个本来对得上的数于是变成
      `null` 而不是 `false`：**「没写」与「写了而不对」是两句话**。同一趟它把 7 枚 `a:t` 拆成 9 枚，
      而字符数 61 一个字没动。
    - odp 那一族的「自报」只有 `object-count` 一条（17 份自产 odp 全是这一个名，值 28~150，另有一份连
      `meta.xml` 都没有），字数、字符数、段落数、页数**一条都没写**；同一件转成 odt 时那枚元素写着八条。
      而 odp 的备注住在 `draw:page` **里面**（`presentation:notes` 是页的孩子），所以「按页之和」与
      「按整份件实算」是同一个数（`stats.odp` 两处都是 75 / 66 / 18 / 9）；pptx 的备注在另一个部件里，
      `ours` 不算它 —— 两族对「备注算不算正文」的答案不同，所以两处都交。
    - 单位与词表都不换算：`words_by_space` 只按空白切（一整段中文可能算一个），键名把口径写在脸上；
      自报那份按串的数交（能进 i64 就交数，进不去就交原串）。
    - 出口：`office-slide` 的 pptx 支与 odp 支各交一份 `statistics`。遗留 `.ppt` 没有这一格（它的自报数在
      `SummaryInformation` 的属性流里，那是 `office-meta` 读的那一本），office-sheet / office-pdf /
      office-text 也没有这个键；`office-doc` 那一族的同问住在 `structure.statistics`，两家不合并。

145. **这一格的字贴哪一边：docx 是一枚元素（同名那枚在节上回答另一个问），pptx 是两枚属性，ODF 一跳且空串照原样交**。
    自产件 6 个格里四格各写一枚 `w:vAlign`，把 ECMA 四态一次摆全（`center` / `top` / `bottom` /
    `just`），另两格连元素都没有；`w:sectPr/w:vAlign` 与它同名而不同事 —— 那一枚说的是
    「这一节的字在纸上顶对齐还是居中」，所以账分成 `vals` 与 `section_vals` 两本，
    而 `word/styles.xml` 这一份里 0 枚（与 `cell_margins`、`table_borders` 那种「样式表里恒有一份」不同）。
    - **「没这枚元素」「有这枚但没写 `@w:val`」「写了空串」是三句话**：ODF 那一跳实测被 LibreOffice
      写成 `style:vertical-align=""` 两枚（账里记成 `(空串)`），而 pptx 的两枚属性都不写是另一格
      （`cells_silent`）—— 都不折成 null、也不推成某个默认词。
    - 三族词表不同名而**不换算**：docx 的 `center` 在 ODF 叫 `middle`、在 pptx 叫 `ctr`；
      odp 那一头转过来**整层不写**（六格 0 格带这一条），所以 odp 那本只有 0 而不是「读不出来」。
    - LibreOffice 重写这份 docx：六格里只剩两格还写着（`center` 与 `bottom`），`top` 与 `just`
      整枚被丢掉（默认值不写是它的算法），而节上那一枚 `center` 留着；
      重写那份 pptx：六格全被写上 `@anchor`（`t` 四、`ctr` 一、`b` 一），`just` 被换成 `t`、
      本来不写的两格补成 `t`，而 `@anchorCtr` 一枚都不剩。
    - 真件普查（本机 32 份 .docx + 1 份 .docm；104 份 pptx、893 枚 `a:tcPr`）：docx 格级只有
      `center` 2898 与 `bottom` 1，`top` 与 `just` **零条**，节上那一枚也**零条**；
      pptx 里 `@anchor` 只有 `ctr` 314 次、`@anchorCtr` 零次，其余 579 枚两枚都不写 ——
      四态、`anchorCtr` 与节上那一支都只能靠合成件守。整库自产件：88 份 word 的 309 个格里
      **只有 8 格**写了、96 个节里 2 个写了；31 份 pptx 的 62 个格里 36 个写了 `@anchor`、
      26 个都没写；47 份 .odt 的 170 格里 5 格跳得到值，17 份 .odp 一个都没有。
    - 出口：`office-doc` 的 docx 支与 odt 支各交 `structure.vertical_align`，`office-slide` 的
      pptx 支与 odp 支各交 `vertical_align`；遗留 `.doc` / `.rtf` / `.ppt` 不交这个键，
      表格那一族（xlsx / .xls）的垂直对齐住在 `alignment/@vertical` 与 XF 那一本里，也不在这个键里。

144. **这一圈有没有线：OOXML 一块里的方向孩子带七个属性，DrawingML 一枚线是一整串，ODF 一跳且三种写法**。
    `w:tblPr/w:tblBorders` 是整张表的默认，`w:tcPr/w:tcBorders` 是这一格改的 —— 两块形状一样
    （方向 `top/left/bottom/right`，表级另有 `insideH`/`insideV`，格级另有对角线 `tl2br`/`tr2bl`），
    每枚带 `@w:val` + `@w:sz`（八分之一磅）+ `@w:space` + `@w:color` 与主题那一套三个指针。
    这份自产件里 3 张表 2 张写了表级块、10 格里 3 格自己改了（其中一格是空壳），
    `@w:val` 一口气出现四态：`single` 9、`nil` 2、`double` 1、`none` 1。
    - **`nil` 与 `none` 是两句话**：前者是「连继承来的那条也关掉」，而且 `nil` 那些**不写**
      `sz`/`space`/`color` —— 于是 `no_sz` 2 是一格单独的账，缺属性不是漏读。真件普查里
      `nil` 39746 条与 `single` 39483 条几乎各半，而带 `sz` 的只有 40446 条。
    - **表上没写不等于没有线**：`word/styles.xml` 恒有 85 枚 `tblBorders` 与 406 枚 `tcBorders`
      （打底模板的表格样式各带一份），整库 86 份 word 件里 54 张表**只有 2 张**写了表级块，
      而 148 个格自己写了块 —— 所以「正文这一处写了没有」与「样式表里有多少枚」分开交。
    - LibreOffice 重写同一份 docx 做四件事：3 张表并成 1 张、表级那份**一份都不剩**而摊到
      每个格上（`cells_with_block` 3 → 10）、方向改名（`left/right` → `start/end`）、
      把 `nil`/`none`/`auto`/主题指针全换成写实的数（`vals` 只剩 `single` 与 `double`，
      `auto_color` 3 → 0、`theme_pointed` 2 → 0、`no_sz` 2 → 0）。
    - 演示那一族线不是属性而是 `a:tcPr` 的**孩子**（`lnL`/`lnR`/`lnT`/`lnB` 与两枚对角线），
      一枚线带 `@w`（EMU）+ `@cap`/`@cmpd`/`@algn` 与 `solidFill`|`noFill` + `prstDash` +
      `round` + `headEnd`/`tailEnd`。本仓按**文档序**交，所以「写了哪几枚」看得见：
      自产件 6 格里 1 格写满四条、2 格只写一两枚、3 格一枚都没有，另有零宽那枚 `@w="0"`。
      LibreOffice 重写时六格全补齐四条、`6350` 换成 `6480`、两枚换成 `12240`、
      **对角线整个丢掉**，还有一枚线**不写 `@w`**（`no_width` 1）——
      「一枚都没有」与「有一枚但没宽度」是两种「没说」。
    - 真件普查（本机 104 份真 pptx、940 个 slide 部件）：893 枚 `a:tcPr` **每一枚**都写满四条，
      `@w` 只有 `6350`（3444 次）与 `0`（128 次），`cap` 恒 `flat`、`cmpd` 恒 `sng`、`algn` 恒
      `ctr`，孩子一律是 solidFill + prstDash + round + headEnd + tailEnd 那一套 ——
      所以「只写一两枚」「一枚都不写」「noFill」「虚线」「对角线」这几形只在自产件里有。
    - ODF 是一跳加**三种写法**：格点 `table:style-name`，数在 `family="table-cell"` 那份样式上，
      可以是短款 `fo:border`、四枚长款 `fo:border-*`，也可以是第三种
      `style:border-line-width-*`（只写线宽）。两族住的 properties **不是同一枚孩子**：
      odt 是 `table-cell-properties`，而 odp 的表格框把边框写在 `paragraph-properties` 上
      （同一份样式的页边距却在 `graphic-properties`，两本各交各的）。整库 46 份 .odt 的 164 格里
      155 格跳得到样式、142 份写短款而 13 份写长款；16 份 .odp 共 25 格，12 格连样式名都不点。
    - 那一跳是有损的：`single` + `sz="8"` 回来是 `1pt solid #000000`、`double` 是 `2.25pt double`，
      `auto` 与主题指针被换成实色（`#000000`、`#c0504d`），而 `nil` 与 `none` 在 ODF 这一格上
      **合成同一个 `none`**（`kinds` 里 none 8）—— 本仓只按交的回答，不替文件分辨。
      一张表是叠画还是各画各的记在 `border_models`（`table-properties/collapsing`）。
    - 单位一律不换算：八分之一磅、EMU、`1pt` 都按写的串交（与 `row_heights`、`cell_margins` 同一处理）。
    - 出口：`office-doc` 的 docx 支与 odt 支各交 `structure.table_borders`，`office-slide` 的
      pptx 支与 odp 支各交 `table_borders`；遗留 `.doc`/`.rtf`/`.ppt` 不交这个键，
      表格那一族（xlsx / .xls）的边框走 `cell_style` 那一本（XF → borders），也不在这个键里。
    - 本机真件里 **.odt / .ods / .odp 一份都没有** —— ODF 那一头只有生产者的凭据，
      这是「没有件可读」，不是「读不出来」。

143. **格子的字离格边多远：OOXML 一份文档里这块有两个住处，ODF 是两跳而且两种写法等价**。
    `w:tblPr/w:tblCellMar` 是整张表的默认，`w:tcPr/w:tcMar` 是这一格自己改的 —— 两块形状一样
    （方向孩子 `top/left/bottom/right`，或双向安全那一对 `start/end`，每枚带 `@w:w` 与
    `@w:type`），而账必须分开数：这份 4 张表里 2 张写了、1 张只写了个空壳、1 张干脆没有那块，
    12 格里 4 格自己改过。**表上没写不等于没有边距** —— `word/styles.xml` 里恒有 100 枚
    `w:tblCellMar`（打底模板的表格样式各带一份），所以「正文里写没写」与「样式表里有多少枚」
    分两格交，不替文件挑一份样式。
    - 序也是笔迹：表级 python-docx 写 `top,left,bottom,right`，格级自己写的是
      `top,bottom,left,right`，同一份件里两种序并存（真件普查 3180 枚表级块里 3143 枚前者、
      232 枚格级块全是后者）。
    - `@w:type` 有 `dxa` 与 `auto` 两态，`auto` 是「这一条让排版自己定」而不是数（真件里
      13583 条方向条目**全部**带 `@w:w` 与 `@w:type`，而 `type` 恒为 `dxa`，`auto` 0 条 ——
      那一形只有自产件能守）。零是一句说过的话：6 枚零与 11 枚非零分开交，缺 `@w:w` 的记
      `missing_w`，不猜一个数。
    - LibreOffice 同格式重写这份 docx 做了四件事：4 张表并成 1 张、方向**改名**
      （`left/right` → `start/end`，四张票各 12）、11 个格各补四条、`auto` 换成 `dxa`，
      还把表级那四条换成第一个格写过的值 —— 块的对调是它的算法，本仓只按文件记下的数交。
    - 同一问在 ODF 是**两跳加两种写法**：格只写 `table:style-name`，数在那份
      `family="table-cell"` 样式的 `style:table-cell-properties` 上（odp 那一族的格是图形对象，
      同一族数住在 `style:graphic-properties` 上，properties 的名字都不一样）；四枚长款
      `style:padding-*` 与一枚短款 `fo:padding` 等价，而 LibreOffice 把全零那一份收成短款、
      其余 11 份仍写长款（整库 144 格里长款 143、短款 1）。样式跨 `content.xml` 与
      `styles.xml` 找，解不开的说「解不开」，不塌成零。
    - 单位一律不换算：twips `113` 到 ODF 变成 `0.199cm`（有损），EMU `36576` 绕 pptx 一圈
      变 `36360`，两族各交自己那个串（与 `row_heights` 同一处理）。
    - 演示那一族只有一处：`a:tcPr` 的四枚属性 `@marL/marR/marT/marB`（EMU），一个都不写就是
      这一格没说。python-pptx 只写被设过的那几枚（6 格里 2 格写满、1 格只有 `marL`、
      3 格一枚都没有），LibreOffice 重写时给每格补齐四枚；真件普查 104 份 pptx 的 893 枚
      `a:tcPr` **每一枚都写满四条**，所以「没写」这一形在真件里没有、只在自产件里有。
    - 真件普查（本机 32 份 .docx + 1 份 .docm、268 个 `word/*.xml` 部件；104 份 pptx、
      940 个 slide 部件）：docx 表级块 3180 枚、格级块 232 枚，另有 28 枚表级块只写左右、
      9 枚写三条；pptx 里 128 个属性值是 `0`。
    - 出口：`office-doc` 的 docx 支与 odt 支各交 `structure.cell_margins`，`office-slide` 的
      pptx 支与 odp 支各交 `cell_margins`；遗留 `.doc` / `.rtf` / `.ppt` 与表格那几个出口
      不交这个键（BIFF 与 ppt 的记录树里没有格子内间距这一层）。

142. **这一段前面画什么：OOXML 把答案写在段自己的 `a:pPr` 上，ODF 写在段点名的那份列表样式里**
    - 形状：pptx 逐页一本 `{family, available, paragraphs, ppr_written, ppr_missing, declared,
      silent, kinds, chars, auto_types, lvl_written, marl_written, indent_written, bu_sz_written,
      spc_before_written, spc_after_written, carriers_found, carriers_seen, rows, listed, cut}`，
      每段一行 16 格 `{para, carrier, shape, ppr_written, kind, char, auto_type, start_at,
      bu_sz_pct, bu_sz_pts, bu_font, lvl, mar_l, indent, align, spc_before, spc_after,
      spc_before_written, spc_after_written, attrs_written}`；另有版式与母版那一本
      `{parts, parts_with_lst_style, levels, kinds, chars, declared, silent, marl_written,
      indent_written, rows, rows_total, listed, cut}`（行是 `a:lvl1pPr`…`a:lvl9pPr` 各一条）。
      odp 逐页一本 `{lists, lists_nested, lists_unnamed, styles_found, styles_unfound, items,
      paras, paras_in_lists, paras_outside, kinds, chars, styles_defined, rows, listed, cut}`，
      每份列表一行；整册那本 `{styles, styles_used, styles_unused, kinds, chars,
      levels_per_style, rows, listed, cut}`。
    - `kind` 四态各是两句话：`buNone` / `buChar` / `buAutoNum` 是文件说了，`"(没写)"` 是壳在而
      里面没有那三枚，`"(无 pPr)"` 是连壳都没有 —— 后两格合起来才叫 `silent`，都不替版式补一个符号。
    - 两家密度差：python-pptx 只在写了符号的段上放壳（13 段里 7 段有），LibreOffice 重写同一份时
      13 段全有并把 `algn`、`defTabSz`、`lnSpc`、`spcBef`、`buClr`、`buFont` 一起写下来；
      同一个左边距它换数（`342900` → `343080`）。`attrs_written` 因此从 2 涨到 4，而那一格是笔迹不是答案。
    - 上面那层是真的存在：这一本 134 枚 `a:lvlNpPr` 里 61 枚什么都不写，重写那本 88 枚一枚不落，
      且母版的 `p:txBody` 在重写里整层消失（`parts` 同为 12、`parts_with_lst_style` 12 → 11）。
    - ODF 那一族：LibreOffice 从 pptx 转来时**一行拆一份 `text:list`**（4 页 2/3/2/0 份，每份一个
      `text:list-item` 套一段字），点名的是 `content.xml` 里的 `L1`…`L5`；整包 54 份列表样式
      （含母版页用的、住在 `styles.xml` 的 `ML1`…`ML10`）各定义十级，页只点 5 份、49 份没人点。
      自动编号那两条变成 `list-level-style-number`：`num-format="1"`、`num-suffix="."`、
      `start-value="3"` 只写在第一条上。而 pptx 里那两条 `buNone` 转过来之后**一页没有列表** ——
      「明确不画」与「这一族不写」在 ODF 这一格上分不开，只按交的回答。
    - 真件普查（本机 104 份 pptx、1866 个页与备注部件、11146 枚 `a:pPr`）：`buNone` 7064、
      `buChar` 3672（`•` 3626、`●` 46）、什么符号都没写 410、`buAutoNum` **0** —— 所以自动编号
      那一支只能合成件守；`marL`+`indent` 10868、两个都不写 278；`spcAft` 4096、`spcBef` 61，
      两族的数全部按文件写着的串交（EMU 与百分之一磅都不换算）。
    - 出口：`office-slide` 的 pptx 支逐页交 `slides[i].bullets` 并交 `bullet_layers`，odp 支同；
      遗留 `.ppt` 与 `.odt` / `.ods` 那些出口不交这个键。
    - 第二读者是 `office_reader.py` 的 `slide_bullets_pptx` / `slide_bullets_layers_pptx` /
      `slide_bullets_odp` / `odp_bullet_styles` / `odp_bullet_ledger`；probe 的 3bv 把三家 producers
      的逐页账与整册账各钉一遍。
141. **这一行多高：`w:trHeight` 的数与那条规则写在行上，而 ODF 的两个键在一跳之外的 table-row 样式里**（三份件）
    - 形状：OOXML 一本 `{family, available, parts_scanned, tables, rows, rows_total, rows_with_height,
      rows_without_tr_pr, zero_height, rules, listed, cut}`，每条行 11 格 `{part, table, row, has_tr_pr,
      tr_pr_children, height_written, height_children, attrs, val, h_rule, h_rule_written}`；
      ODF 一本 `{family, available, tables, rows, rows_total, rows_written, rows_unwritten,
      styles_unfound, zero_height, repeated_rows, listed, cut}`，每条行 16 格（那一跳的目的地、样式在不在、
      两个键、`keep-together`、整张属性表、`repeated` 与 `visibility`）
    - 三种「没有」在三格里分着交：这一行**没有** `w:trPr`（`rows_without_tr_pr`）、有 `trPr` 而里面
      **没写** `trHeight`（`rules` 的 `(没有 trHeight)`）、有 `trHeight` 而**没写** `hRule`
      （`rules` 的 `(没写)`）。缺 hRule 的缺省是 atLeast 是**规范**说的，不是文件写的 —— 这一本不替文件补
    - **零是一句说过的话**：`@w:val="0"` 是 Word 里真用的「藏一行」手法，所以 `zero_height` 单独数，
      不并进「没写高度」那一格；来回一趟它会被写成 `1` + `atLeast`（LibreOffice 不承认零高）
    - 单位一律按串的串交：`1361` twips 正好 2.4cm，而 ODF 那份写的是 `2.401cm`（厘米一绕多出个 1），
      `680` 对 `1.199cm` 同理 —— **不换算也不比对**，两边各交各的原文
    - ODF 那一面**一跳是默认形状**：行只点样式名，值在 `family="table-row"` 的样式的
      `style:table-row-properties` 上，且 exact 与 atLeast 是**两个键**（`row-height` / `min-row-height`）；
      样式里两个键都没有与那一跳解不开是两种形状（`rows_unwritten` vs `styles_unfound`）
    - 表头那一组 `table-header-rows` 只是包着几行，组本身不数一行，而**按文档序**摊进 `row` 的号里
      （`in_header` 记它来自哪一组）；`number-rows-repeated` 与 `table:visibility` 也各交原串
    - 截行不截账：`--limit` 只砍 `rows`，`tables` / `rows_total` / `rules` 那几本数按全量算
    - 边界：这一族只在有 OPC 包与 ODF 的两个分支上交（.rtf 与遗留 .doc 不交这个键，.pptx 的表住在
      页自己的形状树里，`a:tr/@h` 那一族是另一问）；一张表都没写的件交零条的账而不是缺键。
      **不判这一行装不装得下它的内容**（那要算字号与行距，不是文件写着的数）
    - 第二读者是 `office_reader.py` 的 `row_height_docx` / `row_height_odf`（全部 .docx 与 .odt 逐件影子跑过）；
      probe 的 **3bu** 段逐件对两族的整本账，Rust 那侧是 `office_doc.rs` 的
      `a_row_can_say_how_tall_it_is`

140. **图是怎么摆的：`wp:inline` 随字走而 `wp:anchor` 浮着才有环绕那一支；ODF 的环绕在一跳之外的 graphic 样式里**（三份件 + 本机真件量分布）
    - 形状：OOXML 一本包账 `{family, available, parts_scanned, drawings, inline, anchor, other_kind,
      anchor_without_wrap, wrap_elements, listed, rows, cut}`，每条行 21 格 `{part, para, kind, attrs,
      wrap_element, wrap_attrs, children, dist, behind_doc, locked, allow_overlap, layout_in_cell,
      simple_pos, relative_height, doc_pr, locks, graphic_uri, effect_extent, position_h, position_v}`；
      ODF 一本 `{family, available, frames, anchor_types, wrap_values, wrap_unwritten, style_unfound,
      rows, listed, cut}`，每条行 26 格（框自己的 `x/y/width/height/@z-index` 与那一跳目的地里的
      `wrap` / `wrap-contour` / `run-through` / `flow-with-text` / 横纵基准 / 四个边距）
    - 两问分得很开：「随字还是浮着」与「浮着的话文字怎么绕」。`wp:inline` 结构上**没有**环绕那一支，
      所以它的 `wrap_element` 是 null —— 这与「anchor 而文件一个环绕元素都没写」的 null 是同一格两种来路，
      两份账里都靠 `kind` 分得开，而 `anchor_without_wrap` 单独数一个数；
      一枚 `w:drawing` 肚子里既没 `inline` 也没 `anchor` 时交一条**键一个不少、值全空**的行（行与行的键集永远一样）
    - 浮着才写的那几格按文件自己那个串交：`@behindDoc` 是 `"0"` / `"1"` 还是 `"true"` 都照原样交，
      层序 `@relativeHeight` 与四格留白 `@distT/B/L/R` 是 EMU —— **不换算单位**（`114300` 与 `0.21cm` 都不碰），
      位置分横竖两条而「怎么定」写在孩子们上（`wp:align` 或 `wp:positionOffset`，`offset_written` 说的是那一格在不在）
    - 生产者差异（`wrap.odt` → LibreOffice 导回 docx）三条各一样：三份框只剩两张图（按页锚那一张**整张丢掉**，
      `drawings` 3 → 2）、留着的那枚 anchor 把层序号从合成的 `251658240` 换成 `3`（同一意思的两种写法，
      谁也没错，两边都按原样交）、环绕方式它自己挑了一种（ODF 写 `parallel`，docx 这面写 `wrapSquare`）；
      而它把 `wp:positionV` 的孩子写成 `posOffset` 而不是 `positionOffset` —— 两家按同一格判，都不补
    - ODF 那一面**一跳是默认形状**：框只写 `text:anchor-type`，环绕全在它点名的那份 `family="graphic"` 样式里；
      「随字」那一枚的样式里**没有** `style:wrap` 这一格（`wrap_unwritten` 1）—— 「文件没说」与
      「说了不环绕」（`wrapNone`）是两句话，所以两张表都只数文件写着的那些值，null 不进表
    - 真件那一份分布：本机 33 份真件 docx 的 `word/document.xml` 共 161 枚 `w:drawing`，
      只有 1 枚是 `wp:anchor`（写的 `wrapSquare`），另 160 枚全是 `wp:inline`；而那几个目录里一份 `.odt` 也没有 ——
      所以 anchor 侧与三种锚全靠合成件与 LibreOffice 出口守（只证明认得，不证明生产者会这么写）
    - 截行不截账：`--limit` 只砍 `rows`，`drawings` / `inline` / `anchor` / `wrap_elements` 那几本数按全量算
    - 边界：这一族只在有 OPC 包（`word/document.xml` 那族部件）与 ODF（`content.xml` / `styles.xml`）的两个分支上交，
      .pptx / .rtf / 遗留 .doc 不交这个键（图住在页自己的形状树里，不是这条规矩）。
      **不判图在文字上还是下**（`behindDoc` 是文件写的一个串，不是本仓的结论），也**不比对两族的等价性**
    - 第二读者是 `office_reader.py` 的 `picture_layout_docx` / `picture_layout_odf`（全部 .docx 与 .odt 逐件影子跑过）；
      probe 的 **3bt** 段逐件对两族的整本账，Rust 那侧是 `office_doc.rs` 的 `where_a_picture_sits_is_a_different_question`

139. **同一件事写两遍：`mc:AlternateContent` 的块 / Choice / Fallback / 孤儿块是四本数，而「写了两遍」不是常态**（四份件 + 本机真件量分布）
    - 形状：一本包账 `{family, available, parts_scanned, blocks, choices, fallbacks, orphans,
      requires_prefixes, requires_counts, entries, cut}`，每行九格 `{part, blocks, choices, fallbacks,
      orphans, requires, choice_elements, fallback_elements}`。三个数互不相减：`blocks` 数那枚外壳，
      `choices` / `fallbacks` 数肚子里的分支，`orphans` 数「Choice 在、Fallback 没有」的那几块 ——
      一块里可以有几条 Choice（按 `Requires` 挑第一条认得的），所以 `choices > blocks` 不是矛盾。
    - **真件那一份分布**（本机 137 份 OOXML 办公件）：3 份件带 3 块、3 枚 Choice，而 Fallback 只有 1 枚
      （`orphans` 2），块坐在 `word/settings.xml` 与 `word/header2.xml` 那种地方；自产 fixture 里
      13 份带 31 块、31 枚 Choice、31 枚 Fallback、`orphans` 0 —— 两个生产者都写全两遍，
      真件里那两处只写一遍。所以这一格不能按「两遍都在」当默认。
    - 两遍各写了什么是这本账最有用的部分：`tbox.docx` 那一条画布在 Choice 里写 `drawing`
      （DrawingML 的图）、在 Fallback 里写 `pict`（VML 的图）—— 同一个形状两种画法；
      pptx 那一份每页都点 `p14` 且两边写的是**同一个元素名**（`transition`）；
      xlsx 的批注点 `v2`，`Choice` 里 `commentPr`。`requires` 只交文件写的前缀名，
      **不解成 URI**（前缀在那枚根元素的 `xmlns:` 上定义，这一本不再走那一跳）。
    - 空壳那条老规矩在这里第三条：`cell-notes-lo.xlsx` 三块都**配了** `mc:Fallback` 元素
      （`fallbacks` 3）而它肚子里一个元素都没写（`fallback_elements` 空表）——
      「这枚分支在不在」与「它写了什么」是两个数。
    - 生产者差异最狠的一条（`alternate.docx` → LibreOffice 重写）：三块**连字一起丢掉**
      （`blocks` 3 → 0，五句只写在分支里的字一句不剩，只剩打底那一段）。这一族的存在意义就是
      「同一个文件对不同读者说不同的话」，所以只写在某一条分支里的话必须被数出来 ——
      否则下一个读者会以为这份稿子没写过那些字。
    - 边界：`mc:AlternateContent` 是 OPC 的东西，ODF（odt / ods / odp）与遗留 .doc / .rtf 不交这个键；
      没有那一族的包交一本零条的账（`blocks` 0 而 `available` true），不是缺键。
      **不判哪条分支会被用**（那是读者自己解析 `Requires` 的结果，不是文件写着的事实），
      也不比较两条分支的字。
    - 第二读者是 `office_reader.py:alternate_ledger`（四份件与 246 份影子跑过）；probe 的 **3bs**
      段逐件对三族的整本账，Rust 那侧是 `office_doc.rs` 的 `two_branches_do_not_have_to_be_there_twice`。

138. **包里那几份自定义 XML 存储：件自己还剩多少字、那一跳到 `itemProps` 断没断、正文有没有一条手指着它**（三份件 + 本机 25 份真件 docx，真件只量数、不入库）
    - 形状：一本包账 `{family, available, parts_total, items, items_empty, overrides, default_for_xml,
      anchors_custom_xml, anchors_data_binding, binding_ids, unresolved_bindings, entries, cut}`，
      每条 entry 十格 `{part, size, root, children, props_rel, props_found, item_id, schema_uris, bound, declared}`。
      两条链各交各的：件 →（它自己的 `.rels` 里那条 `customXmlProps`）→ props →（`ds:itemID` /
      `ds:schemaRef/@ds:uri`）→ 号与 schema；正文 `w:sdt` 上那条 `w:dataBinding/@w:storeItemID` 指的就是这个号。
    - **这一族在真件里就是「躺在包里、没人指」**：本机 25 份带存储的 docx 里 `w:customXml` 与
      `w:dataBinding` 各 0 处，而那 25 份的 `item1.xml` 全是 Word 引用管理器写的 `b:Sources`；
      `python-docx` 的打底模板自带同一形状的那一枚（`notes.docx` items 1、uri 1 条、手指 0 条），
      所以「自产件里也有存储」不是本仓加的。
    - 「部件在」与「部件里还有字」是两问：LibreOffice 重写 `customxml.docx` 时把三枚 `itemN.xml`
      **整件清空成 0 字节**，而三份 `itemPropsN.xml` 与那一跳、与 `ds:itemID` 全留着 —— 于是
      `items` 3 / `items_empty` 3 / 每行 `root`、`children` 是 null（0 字节没有根元素可读），
      清空不是丢失，两者在这一格里分得开。
    - 同一件事它多写了一份（两份存储变三份），而头两份 props 的 `ds:itemID` 是**同一个号**：
      正文那一条手指于是同时指着两份（`bound` 两行都是 1）—— 「一条手指解到哪一份」在这副件里
      判不住，如实交两个 1 而不是替文件挑一份。另一条手指它不认：`w:customXml` 整条丢掉
      （`anchors_custom_xml` 1 → 0）而 `w:dataBinding` 留着，同族两条手指两种待遇。
    - 包怎么承认这一族也是两格：`customXml/itemN.xml` 自己**不在** `[Content_Types].xml` 上点名
      （只有 props 那一份点，`overrides` 数的就是这个），它靠 `Default Extension="xml"` 兜着
      （`default_for_xml`）—— 所以每行 `declared` 恒 false 而那一格恒 true，合起来才是答案。
      第二枚存储的 `ds:schemaRefs` 在而里面空（真件 25 份里正有一份这样写），所以 `schema_uris`
      是空表，与「那一跳解不开」交 `props_found: false` + null 是两件事。
    - 边界：这一族只在上 OOXML 包的那三个出口交（`office-doc` 的 docx、`office-sheet` 的 xlsx、
      `office-slide` 的 pptx）；odt / odp / .doc / .rtf 不交这个键——ODF 没有 OPC 包，这一层不存在。
      没有存储的包交一本零条的账（`book.xlsx`：`items` 0 而 `default_for_xml` true）而不是缺键。
      存储里的**字不解释**：那是一份别人定义的 schema，本仓只交件名、字节数与孩子条数。
    - 第二读者是 `office_reader.py` 的 `cx_ledger` / `docx_custom_xml`（三份件与 246 份影子跑过）；
      probe 的 **3br** 段逐件对这三族的整本账，Rust 那侧是 `office_doc.rs` 的
      `a_store_can_sit_in_the_package_with_no_pointer_at_all`。正文那两条手指的写法（`w:customXml`
      与 `w:sdt`/`w:dataBinding`）本机真件 0 份写过，是**合成**的（照 ECMA 的写法与真件那一份
      存储的形状拼），与 `notes.docm` 那枚合成宏同一待遇：它只证明「认得这两条手指」。
      这一句中间被改掉过一次——那次普查把仓库的 90 份自产件也算进了「本机 162 份」的分母，于是
      「量出 6 份写过」量的其实是自己写的 fixture；按「真件 / 自产件」分开重扫，真件 33 份里
      `w:dataBinding` 与 `w:customXml` 确实一枚都没有（两个口径与教训都记在事实 147）。

137. **这一页的底色是谁给的：页自己写、版式与母版写、还是整册都不写，而「写了不填充」与「什么都没写」在两家手里不可分辨**（三份 `deck-bg*` + 本机 104 份真件 pptx / 940 页，真件只量数、不入库）
    - 形状：页级 OOXML 17 格 `{family, available, holder, written, via, attrs, fill, fill_element, fill_attrs,
      fill_children, stops, stop_positions, colors, color_names, effect_lst_written, effects, idx}`；整册 8 格
      `{family, available, parts_scanned, parts_with_bg, layers, fills_seen, entries, cut}`，每条 `entries` 在那 17 格
      之外多两格 `layer` + `part`。ODF 页级 13 格（`page_style` / `style_found` / `style_part` / `fill_written` /
      `fill_attrs` / `background_attrs` / `props_written` / `gradient` / `inherited` 那一本），整册 8 格
      `{family, available, pages, pages_unnamed, shared_styles, silent_styles, styles, unfound_styles}`；而样式那一行
      只有 8 格、**不带 `props_written`** —— 那份样式说了几句话只跟着页交，不跟整册那本重复一遍。
    - 两种 null 不是一件事：`bgRef` 那一支的 `fill` / `fill_element` / `fill_attrs` 交 null 而 `fill_children` 交
      空表，因为它的颜色**直接挂在 `bgRef` 下面**、压根没有填充元素那一层；第 4 页那种 null 是「整枚 `p:bg` 不在」，
      于是 14 格全 null 而只有 `written: false` 是真的。把前者当后者会把 `idx` 与那枚主题色名一起丢掉。
    - **DrawingML 的颜色修饰是孩子元素，不是属性**：`tint` / `shade` / `satMod` / `alpha` 都写在颜色元素肚子里。
      所以 `colors` 每行交四格（元素名、`val`、`modifiers` 属性、`modifier_elements` 孩子）—— 只收属性那一格
      在 fixture 与真件里都是**恒空**，等于把「这一站调过色」这句话的证据整批丢掉；真件里量到 17 行带修饰，
      每行恰好一枚，且全是 `a:alpha val="100000"`（写了全不透明 vs 什么都不写，渲染上等价、在这一格里可分辨）。
    - 生产者指纹一（LibreOffice 重写同一册）：它把第 2 页那枚完整的 `<p:bg><p:bgPr><a:noFill/>` **整条丢掉**，
      于是 `deck-bg-lo.pptx` 的第 2 页与第 4 页交出**同一份全 null 的记录**（Rust 测试直接把两行与 python-pptx
      那份的第 4 页对相等）。这不是读不出来，是文件里没了 —— 所以这一格不判「谁覆盖了谁」，只交「写没写」。
    - 生产者指纹二（同一册的层间搬家）：模板只在**母版**写 `bgRef idx="1001"` + `schemeClr bg1`
      （`parts_scanned` 16 / `parts_with_bg` 4，`layers` = master 1 + slide 3），LibreOffice 重写时把它**摊到
      11 份版式上写成字面 `FFFFFF`，而母版自己那一枚没了**（16 / 13，`layers` = layout 11 + slide 2，
      整本里 master 与 notesMaster 两层 0 条）。只看页部件会把这次搬家读成「底色丢了」，这就是 `layers` 的意义。
    - 生产者指纹三（替文件算完了）：python-pptx 的渐变两站都写 `schemeClr accent1` + `tint 100000/50000` +
      `shade 100000` + `satMod 130000/350000`（六枚修饰孩子），LibreOffice 交回两个 `srgbClr` 字面值
      （`3E7FCC` / `A4C1FF`）而 `modifier_elements` 两行都是空表；同一枚渐变方向元素两家点的属性名也不一样
      （`a:lin @scaled="0"` vs `@ang="0"`），真件里第三种是 `@ang="2700000"`，而 `rotWithShape` 三种写法都见得到
      （`1` / `0` / `true`）—— 全按原样交，不折算。空壳 `<a:effectLst/>` 是一句说过的话：`effect_lst_written`
      （在不在）与 `effects`（肚子里几个孩子）是两个数；真件里 210 / 940 页写了空壳，而母版、版式、备注母版
      那三层**一枚都没有**（0 / 233 条）。
    - 真件那本（104 份 pptx / 940 页，只在本机量、不进仓）：页级**全部写了**底色（`written` 940/940、`via` 全
      `bgPr`、`holder` 全 `cSld`），填充只见到两族（`solid` 932 / `gradient` 8，`none` / `blip` / `pattern` /
      `group` 各 0 面 —— 那三族的形状由 fixture 与 null 的规矩守着）；页级 948 行颜色**全是字面 `srgbClr`**、
      31 个不同值、最勤的一枚 `1A1A2E` 顶 628 页，`schemeClr` 在页上 0 处；主题色只在另外三层（`notesMaster`
      103 / `layout` 101 / `master` 25 条，全是 `bgRef` + `idx="1001"` + `schemeClr bg1`），而 master 那一层
      只在 27 份件里出现。`parts_scanned` 13~54 不等、`parts_with_bg` 3~54、`cut` 104 份全 false（只砍逐件列表
      那一条规矩在这本上同样不触发）。还有一份 0 页的 pptx：页级记录 0 份，整册那本照样交 —— 两本不是一份账。
    - ODF 那一面（`deck-bg.odp`，本机真件 `.odp` 0 份，所以这一支只有自产件撑）：页只点名，底色写在
      `drawing-page-properties` 上，句数 `props_written` 量到 dp1 7 / dp2 4 / dp3 5 / dp4 7（content.xml）、
      Mdp1 3 / Mdp2 4（styles.xml）；第 2、4 页**共用一份 dp3**，那份 5 句话里一条 `draw:fill` 都没有，
      于是整册账上 dp3 挂着 2 页（`shared_styles` 1 / `silent_styles` 1）—— 靠页数才看得出来这一格被共用。
      两份**没人点**的样式（dp2 与 Mdp2）写的四句一字不差，而整册那本只列页点名到的那三份，所以它们不在这本上；
      11 份 `style:master-page` 全部点同一份 `Mdp1`。继承那一跳在 ODF 是**三跳**（页 `@draw:master-page-name`
      → `style:master-page/@draw:style-name` → 那一份 drawing-page 样式），实测 `Blank → Mdp1 →
      draw:fill="solid" #ffffff`，这才是那两页看起来「有底色」的来路；任一跳断了交 `found: false`，
      不替文件补一个白色。渐变要两跳：`draw:fill-gradient-name="msFillGradient_20_1"` → styles.xml 的
      `office:styles` 里 `<draw:gradient draw:name=…>`（元素名是 `draw:gradient` 不是 `style:gradient`），
      八格属性原样交（`angle="90deg"`、两端色、两端 `intensity`、`border`、`display-name`、`style`）。
    - 边界：遗留 `.ppt` 不交这个键（它的记录树里没有页底这一层，本机没有凭据），docx / odt 也不交
      （本机 32 份真件与 105 份自产件的 `word/document.xml` 里 `w:background` 与 `w:displayBackgroundShape`
      各 0 处，python-docx 1.2.0 没有这个口，LibreOffice 的 docx 导出一个字都不写 —— 生产者做不出，
      不是读不出来）。
    - 第二读者是 `office_reader.py` 的 `_bg_ooxml` / `pptx_background_ledger` / `slide_page_background_odp` /
      `odp_background_ledger`（246 份影子跑过）；probe 的 **3bq** 段逐件对这两族的整本账，Rust 那侧是
      `office_slide.rs` 的 `a_silent_page_and_an_explicit_no_fill_read_alike`（三份件全钉）。

136. **图自己那串字节说的三本账：声明、签名与摆在页上的那块，而「密度住在字节里」是量出来的**（134 份出口 / 71 行，其中 34 份有图）
    - 形状：账本 19 格 `{family, available, addr, agrees, at_natural, cut, declared_pixels, density,
      detected, distinct_parts, ext_agrees, listed, natural, pixels, placed, read_cap, rows, total}`，
      行 22 格（`where` / `word` / `word_name` / `ext` / `ext_name` / `sig` / `head_hex` / `how` /
      `pixels` / `declared_pixels` / `px_agrees` / `density` / `nat_mm100` / `placed_mm100` /
      `scale_permille` / `aspect_permille` / `stretched` / `at_natural` / `agrees` / `ext_agrees` / `note`）。
    - 三本尺寸各数各的，不相减也不合并：`placed`（页上占的那块）71 行全有、`pixels`（图自己头里的像素）
      62 行有、`natural`（拿自带密度乘回去的自然尺寸）只 24 行算得出 —— 三分之二的图只能靠文档那一侧说话。
      来路三种：`wp:extent` 34 行、`draw:frame/@svg:width` 24 行、`\picwgoal × \picscalex` 13 行，
      `from` 与 `written` 两样都交，因为同一张图三家写的串长短不一（121920 EMU / `0.339cm` / 192 twips）。
    - 地址那一格四态：`read` 70 行落到部件，`none` 只有 `tbox.docx` 那一条（框里既没有 `a:blip` 也没有
      `r:embed`，连字节都没有，于是 `head_hex` 交空串而签名与像素交 null），`unresolved` 与 `missing`
      本库各 0 —— 那两条分支由合成件守着。71 行只指向 64 份不同的字节（`distinct_parts`）。
    - 「它是什么格式」有三处说法，各交各的：`word` 是文件写在引用处的名字（OOXML 的 `image/png`、RTF 的
      `\pngblip` / `\jpegblip` / `\wmetafile`、ODF 的 `draw:mime-type` 可以整条不写所以 7 行 null）、
      `ext` 是部件名尾巴（RTF 恒 null）、`sig` 是头八个字节认出来的。三本从不互相打脸：`agrees` 64 真 /
      0 假 / 7 判不了，`ext_agrees` 51 真 / 0 假 / 20 判不了（13 条 RTF 没名字 + 6 条 `eq.odt` 的 SVM +
      1 条没字节）。名字短写的有 8 行（`.jpg` 4、`.tif` 4），仍按文件自己那一格判真。
    - 签名与「从哪儿读出来的」也各交一本：png 42（`IHDR`）、jpeg 8（`SOF0`）、svm 6、gif 5、tiff 4
      （`IFD0`）、bmp 3（`LogicalScreenDescriptor` 5 与 `BITMAPHEADER40` 3 两式）、wmf 2（头里只有记录
      长度，`how` 交 `Header`）、另 1 行没读到 —— 固定偏移与走目录是两条路，不是一条。
    - 自带密度四态再加一 null：`read` 24 / `unitless` 4 / `absent` 29 / `none` 13 / null 1（那一条就是没字节
      的那一行）。`unitless` 是 JFIF 写了密度字段而单位给 0（`bare0.jpg`），与「这个格式有这一格而这份件
      没写」（`absent`）是两句话；`none` 是这个格式压根没有这一格（WMF）。单位只写得出来三种：ppm 17、
      dpi 7、aspect 4，另 43 行交 null。
    - 拉伸四行、按原尺寸摆的四行，全部出自那四份 `images-dpi`：`stretched` 4 / 58 / 9 判不了，`at_natural`
      4 / 20 / 47 判不了。四行的长宽比偏差是 583‰ / 583‰ / 583‰ / 582‰（判据是 1‰ 的零头而不是相等 ——
      生产者在最后一位上就不一致），原尺寸那四行的偏差恒 2‰。两批**没有一行重合**，只是两处看着像同一行
      —— 被并成同一个部件名的两张图各自顶着一行，「同一份字节」与「同一行」是两问。
    - RTF 独家那一格：`\picw` / `\pich` 是文件自己声明的像素数，13 行全写了、11 行与头里认出来的对上、
      0 行打架、2 行判不了（那两张 WMF 头里没有像素）；OOXML 与 ODF 两族 58 行压根没有可写这一问的地方，
      所以交 null 而计数全落在 `undecided` —— 缺这一问的族交 null，不交 false。
    - 封顶不同本身就是一条实测事实：`read_cap` 在 OOXML 与 ODF 是 65536（够走完 PNG 的块表与 TIFF 的第一个
      IFD），RTF 只有 8192（群头扫 16KB、十六进制解出来的上限）。
    - 生产者指纹一（LibreOffice 存 .odt）：九个框只留六份字节 —— 它按**像素内容**去重帧，于是 72 DPI 那一张
      借了兄弟的 11811 ppm 与自然尺寸 339×203（本该是 2835 / 1411×847），32×16 的 GIF 把同尺寸的 TIFF
      吸走、那一行的 `absent` 密度跟着没了，而 583‰ 那一格没变。教训：**密度住在字节里，不住在像素里**。
    - 生产者指纹二（同一条来回的另两副）：.odt 转回 .docx 时部件后缀 `.jpg` 被写成 `.jpeg`；存成 .rtf 时
      GIF 与 BMP 被重编码成 PNG、两张 TIFF 变成 WMF，于是 `detected` 只剩 png 5 / jpeg 2 / wmf 2，
      gif / bmp / tiff 三个名字在这一族的账里一次都不出现。
    - `note` 在整库 71 行恒 null：这一族的含糊全有格子可放，不需要旁白。
    - 截行不截账（与事实 131–135 同一条规矩）：`images-dpi.docx` 限到 1 时 `listed` 1、`cut` true，
      而 `total` 9、`distinct_parts` 8、`addr` / `agrees` / `density` / `pixels` / `natural` /
      `declared_pixels` / `detected` 各本计数一格没动。
    - 反面凭据：这一格在 office-doc 的三条链上都交（74 份 word / 42 份 .odt / 18 份 .rtf，零行的也交整本），
      而 office-sheet 的 `book.ods`、office-slide 的 `deck.odp` 与遗留 `notes-en.doc` 三处**不带这个键** ——
      表格与放映上那张图另有一本 `pictures`（事实 71），那一家连「零行」都不报。
    - 第二读者是 `lyco_pictures.py` 的 `picture_bytes()`（三处出口与 Rust 的三个调用点对称）；probe 的 3bp
      把**每一份 .docx / .docm / .odt / .rtf** 的整本与逐行和它对（134 份），再钉上面那十几条数。

135. **这一条引用指的是谁、题注序列数到第几：三族各抄一遍域指令，而「能不能解析」要查三本不同的书**（74 份 OOXML 文字包 / 42 份 .odt / 18 份 .rtf 出口）
    - 形状：三族共有那 18 格（`structure.cross_refs`）交 `{family, available, target_rows, listed, cut,
      books{bookmarks, bookmark_marks, style_defs, style_ids, style_names, sequences}, kinds, target_books,
      resolves{true,false,null}, quoted{true,false,null}, resolving_but_no_cache, cached_but_unresolved,
      cache_missing, cache_values, caption_styles, declarations, notes, rows}`；`caption_styles` 多的那一格
      `styles_part` **只属于 OOXML**，`declarations` 那一整块**只属于 ODF**。差别不在键名而在**哪几本填得出东西**：
      OOXML 与 RTF 的 `books.sequences` 恒 null（这一族没有序列声明那一层），ODF 的 `books.style_ids` 恒 null
      （样式只有名字，没有 `w:styleId` 那一格）。
    - 行的来源是**已经交过的那本域账**（`field_ledger`）：这里只把「有目标的那几条」挑出来，不重读文件、
      不替文件补一条指令。所以「没有交叉引用」与「这一族没有这一层」是两件事，零行也整本交。
    - 74 份 word 件每一份都交这一格，可只有 4 份写着这五种域（`target_rows`：70 份 0 条、2 份 1 条、2 份 4 条），
      而 `kinds` 的总和在每一份都恒等于 `target_rows`。SEQ 在 OOXML 判不出成不成立，所以 74 份的
      `resolves.null` 与 `kinds.SEQ` 全对得上，`notes` 里恒写着那一句解释——把它当成 false 就等于替文件编一条「引用坏了」。
    - 题注样式两本各查各的：72 份的 `<w:style w:styleId="Caption">` 两个名字都对上（`w:styleId` 大写 C、
      `w:name` 小写 caption，故 `matched_on` 交两条），`pnum.docx` 与 `tbox.docx` 两条都没有而交 `declared` 0
      （不是缺格）；74 份的 `paragraphs_using_them` **恒 0**——题注段用的是生产者摊出来的直接格式，
      声明了没人用在这一族是常态。样式那三本账（元素数 / `w:styleId` 数 / `w:name` 数）74 份恒等，
      164 是那 35 份模板件的存量，而 LibreOffice 那 39 份从 2 一路散布到 172。
    - ODF 多一本**声明**的账，四格都是数出来的：`elements` 是 `text:sequence-decl` 的枚数（38 份 5 枚、2 份 6 枚、
      2 份一枚不写），与 `books.sequences` 的长度在 42 份里**恒等**；`wrappers` 说「哪个部件写了几枚」（恒在
      `content.xml`）。LO 每次存 .odt 都把 Drawing/Figure/Illustration/Table/Text 那五条模板序列写进来，
      40 份的 `declared_unused` 就是这五个名字。题注样式在 ODF 只有一本可查：39 份命中且 `matched_on` 只一条
      `name`、`style_id` 交 null 而不是 0。
    - RTF 的 18 份全有题注样式，而它的 `style_id` 是**样式号**（`\s` 后面那个数），各家自己写的：
      54 的 12 份、55 的 3 份、24 / 109 / 110 各一份——同一个名字在不同文件里号不同，所以号只按文件交、
      不折成 docx 的 `Caption`。目标解自 `\fldinst` 那一群，指令串消掉转义就是 docx 那一串。
    - 整个语料只有 8 份写了这五种域（4 份 word、2 份 .odt、2 份 .rtf），合起来 19 条。按 `target` × `resolves` ×
      `book` 摊开：`REF` / `PAGEREF` 三族都判得出成不成立（书签那本有名字可查，全 true），`STYLEREF` 两族都 false
      （点的是样式名那本，而 `标题 1` / `标题 1 (user)` 不在库里），`SEQ` 则是 word 与 RTF 交 null、ODF 交 true——
      同一个「查不到」在两类账里是两回事：一类没有那本书，一类有书而没那个名字。
    - 同一段稿子两个手：`fields-mix.docx` 与 LibreOffice 重写那份的 `target_rows` 4、`resolves`、`quoted` 三格
      **完全一致**（切目标的刀不认生产者），差别全在缓存值那一列——Word 那份四行都写了结果（`cache_written` 全 true），
      LO 那份把 REF 的 `w:result` 留空（`cached` null 而 `cache_written` false），于是 `resolving_but_no_cache`
      Word 0 / LO 1。RTF 的 REF 行是这一族独有的形状：`\fldrslt` 在场但里面没字，故 `cached` 空串而
      `cache_written` true——「写了空结果」与「没写结果」分开记账。
    - ODF 的目标是**属性**不是指令串，所以那一行没有 `instruction` 可切：`target_written_quoted` 交 null
      （引号这一问在这一族不存在），另交 `target_written` 说属性在不在场。`text:sequence` 那一行把
      `text:formula`（这一族写 `ooow:图+1`，对位 word 指令里那句 `\* ARABIC`）、`text:ref-name`、`text:num-format`
      与自己的 `text:name` 分开交——「这一枚属于哪个序列」与「它指向哪个序列」是两格。
    - `bookmarks` 与 `bookmark_marks` 两本的差在 ODF 最清楚：`fields.odt` 只有「表锚点」一个名字，可它由
      `text:bookmark` + `text:bookmark-end` 两枚元素写成，故 marks 2 而 names 1；docx / RTF 同一个名字只有一枚
      起始标记，两格同为 1。断链那两份（`bkmks.docx`、`bkmks.odt`）各 5 枚标记、4 个名字，与书签那一族的账对得上。
    - 截行不截账：`--limit 1` 只砍 `rows`（`listed` 1、`cut` true），四条计数仍按整本算——`target_rows` 4、
      `kinds` 四种各一枚、`cache_values` 两个值、`books.bookmarks` 那本也照全。
    - 反面凭据：这一格只在 office-doc 交，三族都有出口。遗留 .doc 的交叉引用住在 piece 流里的 field 指令，
      本机没有第二个读者可核对，所以那一家连「零条」都不报；.ods / .odp 在 office-doc 没有这一格——12 份 .odp 里
      10 份各写 28 条 `presentation` 族样式（名字带 Caption 的那批**是母版样式不是题注**），另 2 份零条，
      把它们算进这一族只会替文件编一本假账，缺键 = 这一族没这一层。
    - 第二读者是 `lyco_cross_refs.py` 的 `docx_cross_refs()` / `odf_cross_refs()` / `rtf_cross_refs()`
      （三处出口与 Rust 的调用点对称）；probe 的 3bo 把**每一份 .docx / .docm / .odt / .rtf** 的整本逐键与它对
      （134 份），再钉上面那十几条数，最后钉 .ods / .odp / .doc 这三家这个键**不在**。

134. **这份稿子的默认字与默认段：OOXML 那一块可能写两遍而 Normal 会再抄一遍，ODF 摊成一族一条且三格各一本账**（74 份 OOXML 文字包 / 42 份 .odt 出口，另 26 份 ODF 只量不出口）
    - 形状：OOXML 那一本（`structure.doc_defaults`）交 `{family, available, styles_part, styles_effects_part,
      parts_with_block, blocks_total, block_shapes, children_total, rpr_names, ppr_names, rpr_rows, ppr_rows,
      wrote_theme, wrote_literal, font_ascii_written, font_ascii_theme, font_blank_attrs, size_written,
      size_cs_written, lang_written, extras, normal_style}`；ODF 那一本同键名而**换一套内容**
      `{family, available, styles_part, defaults_total, defaults_in_content, families, rows, fonts_written,
      sizes_written, langs_written, hyphenation_names, hyphenation_rows}`，其中 `rows` 一族一条、每行 16 格
      （三个字体名槽 + 三个字号槽 + 三对语言槽，再加 `part` / `children` / `props_attrs_total`）。
    - 这一问的答复住在**两处**：`<w:docDefaults>` 那一块，与 Normal 样式自己的 `pPr` / `rPr`。35 份 Word 模板件的
      Normal 光板一块（`normal_style.rpr_rows` 与 `ppr_rows` 都是空表，默认值只在 docDefaults 那一层），39 份
      LibreOffice 重写的件在 Normal 里**又摊平抄了一遍**（那 39 份的 `rpr_rows` 是六条 37 份、两条 2 份）——
      同一句话在不同生产者手里住在不同的格子里，所以两层各交各的、不相减也不合并。
    - 那一块还可能在**两个部件各一份**：35 份在 `word/styles.xml` 之外带一份 `word/stylesWithEffects.xml`
      （`blocks_total` 2、`parts_with_block` 两条、`block_shapes` 一块一行），39 份只有一块。取值按**文档顺序第一块**
      说话、不合并；那两块去掉空白之后逐字相同（原始 388 对 484 字节，35 份全成立）—— 这一条是 python 那一份读者量的，
      Rust 只数不判，交的是「几块、各在哪个部件、每块说了什么」。
    - 四把各自独立的钥匙开同一批 35 份：`<w:docDefaults>` 写两遍、Normal 光板、`w:pPr` 那一条叫 `spacing`、
      字体名**只写主题指针** —— 四个记号在 74 份里逐份同真同假、一个反例也没有（`extras` 不在这把锁里：那 39 份里
      有 6 份多写一条 `kern` 或 `color`）。这是这一格里最像「生产者指纹」的一条。
    - 74 份每份都带这一本，每块恒两个孩子（`rPrDefault` + `pPrDefault`，没有一份是 1 或 3）；`w:rPr` 三种形状
      （68 份 `rFonts/sz/szCs/lang`、4 份多插 `kern`、2 份多插 `color`，多出来的那些交在 `extras`），`w:pPr` 两族
      （`spacing` 35 对 `suppressAutoHyphens` 39）。字号与语言各只有两副答案：`size_written` 恒等于
      `size_cs_written`（22 有 68、24 有 6），`w:lang` 三属性两组（`en-US`/`en-US`/`ar-SA` 68 对 `en-US`/`zh-CN`/`hi-IN` 6）。
    - 字体名是**两个各自的布尔**（主题指针 / 字面名），三种搭配都存在（只写指针 35、两套都写 28、只写字面名 11），
      所以「有没有 `w:rFonts` 这个孩子」问不出这件事；而 `w:cs` 可以是**空串** —— 「写了这个属性」与「点了个字体名」
      是两件事，27 份如此，且七个属性里**只有 `cs` 会被写空**（另 47 份一个空串都没有），所以另交 `font_blank_attrs`。
    - ODF 没有 `<w:docDefaults>`：`style:default-style` **一族一条**，42 份 .odt 里 40 份恒四条（graphic / paragraph /
      table / table-row）、2 份（`pnum.odt` / `tbox.odt`）**有 styles.xml 却一条都不写** —— 那两本交空账而
      `styles_part` 仍是 true，与「没有这个部件」分开两列；`defaults_in_content` 42 份恒 0（两列计数都留着，
      断在另一头也要数得出）。`families` 是排过序的名字表而 `rows` 保持**写的序**（LibreOffice 把 graphic 写在最前，
      而 `.ods` 那两族是 table-cell 在前 —— 排序只为一问，写的序本身另有一本）。
    - 字体名、字号、语言是**三格各一份账**（latin / asian / complex），因为任何一格都能单独不写。出口件里的凭据是
      `tbox-lo.odt` 的 graphic 那条**只写了 asian 的字号、没写它的名字**（`sizes_written` 6 条对 `fonts_written` 5 条）；
      把 68 份 ODF 全量一遍更是：199 条 default-style 里 119 条带 `style:text-properties`，那 119 条把字号与语言的
      三格**全写满**、字体名却只 105 / 104 / 105 条 —— 差额 14 / 15 / 14 全在 .ods 的 graphic 一族（`book.ods` 就是其一：
      名字三格全不写而字号写了 `12pt`）。合成件也补了一格：一族的孩子撞名时先到的说话算。
    - `table` 与 `table-row` 那两条**从来没点过字体**（80 行全 null，`props_attrs_total` 只有 1）；断字那 13 条
      只住在 paragraph 下面（39 份全写这 13 条、3 份一条没有）；语言那三格里 graphic 一族有一份整对写 `none`
      （`tbox-lo.odt`），`none` 与「没写」也不是同一句话。
    - 这一本对 `.ods` / `.odp` **没有出口**（量到了但不交：15 份 .ods 恒两条、12 份 .odp 里 11 份一条 graphic 而
      `eqs.odp` 零条），RTF 把默认值混在样式表 Normal 那一条里（`{\s0 …}` 与文档默认分不开），遗留 .doc 的在表流里 ——
      两族连这个键都不出现，缺键 = 这一族没这一层。
    - `--limit` 只砍列表、砍不动算术（与事实 131–133 同一条规矩）：宏文档限到 1 时 `block_shapes` 只交第一条，
      而 `blocks_total` 仍是 2、`parts_with_block` 仍两条、`rpr_names` 仍四条、`size_written` 与 `normal_style` 一格没动；
      `nset.odt` 限到 2 时 `rows` 两条（graphic / paragraph）、三本 slot 账各只交前两条，而 `defaults_total` 4、
      `families` 四条、`hyphenation_names` 13 条全按整本数。
    - 第二读者是 `office_reader.py` 的 `docx_doc_defaults()` / `odf_doc_defaults()`（两处出口：office-doc 的
      OOXML 支与 ODF 支，与 Rust 的调用点对称）；probe 的 3bn 把**每一份 .docx / .docm / .odt** 的整本逐键与它对，
      再钉上面那几条数（含四把钥匙同批、两块同文 388/484、27 份 `w:cs` 空串、fonts 5 对 sizes 6），最后钉
      .ods / .odp / RTF / .doc 这四族这个键**不在**。

133. **这份文件是按哪个版本的排版规则排的：OOXML 一种问话两种写法，ODF 摊平成另一套词汇，两边不折算**（74 份带 `<w:compat>` 的 OOXML / 40 份有 `settings.xml` 的 odt）
    - 形状：OOXML 那一本（`structure.layout_compat`）交 `{family, available, settings_part, compat_written,
      compat_total, children_total, mode, mode_total, named, switches, named_names, switch_names,
      names_in_both_encodings, uris}`；ODF 那一本同键名而**换一套内容** `{family, available, settings_part,
      item_set_written, items_total, booleans_total, booleans_true, types, compat_items, compat_item_total,
      same_name_rows}`。键名相同不代表同一问：ODF 根本没有 `<w:compat>` 这一格，LibreOffice 把兼容开关摊进
      `settings.xml` 的 `ooo:configuration-settings`，类型在 `config:type` 上、值在正文里。
    - 两种写法的**语义差别**是这一本存在的全部理由：`<w:compatSetting>` 的值在 `w:val` 属性上，
      而裸开关 `<w:useFELayout/>` **身上什么都没有**，在场即为开。实测 74 份里**一枚 `w:val` 都没写过**，
      所以「在场即开」是本族料唯一走得通的读法；`w:val="0"` 与 `"false"` 说「明确不要」那一条读法
      只有合成件能测（`layout_compat.rs` 的单测），真件里一个例子都没有。
    - 裸开关只出现过四个名字（按 73 份 .docx 数：`useFELayout` 34、`doNotUseHTMLParagraphAutoSpacing` 6、
      `doNotBreakWrappedTables` 4、`adjustLineHeightInTable` 2），具名项只出现过六个名字，`w:uri` 恒
      `http://schemas.microsoft.com/office/word`，而两种写法的名字**互不重叠**（`names_in_both_encodings`
      74 份全空）。`compatibilityMode` 三个答案：14（61 份）、15（7 份）、12（5 份）。
    - 同一个 `<w:compat>` 两套笔迹（按 `docProps/app.xml` 的 Application 分）：34 份写着 Microsoft Macintosh Word
      （python-docx 那个模板）**都只带 `useFELayout`**、也都写满四条具名项；LibreOffice 写的 39 份里 33 份
      一个裸开关都不补。按「几条具名项 + 几枚裸开关」数是**六种搭配**：`4+1` 34 份、`4+0` 28 份、`1+2` 4 份
      （`nset` 那一家）、`3+0` 4 份、`3+2` 2 份、`2+0` **只有一份** —— 「具名项至少写四条」是生产者的习惯，
      不是这一格的规矩，所以两个数各交各的，不合成一个「有没有 compatSetting」。
    - 那一份 `.docm` 也带这一格：账与那 34 份模板件同形（`compatibilityMode=14` 加三条、一枚
      `useFELayout`），而具名项的名字总数与 `w:uri` 都**不增** —— 宏文档不是新形状，只是这一格不
      独属于 .docx。整批 OOXML（134 份包的 2546 份 xml 部件，含 `.rels`）里 `<w:compat` 只出现在
      `word/settings.xml`，且没有一份是空的（孩子数 2 到 5）。
    - ODF 那一本不是常量也不是套话：`ooo:configuration-settings` 在 40/43 份 odt 里（另 2 份整个没有
      `settings.xml` → 交空账，`settings_part` / `item_set_written` 是 false，而不是「有 0 条的一组」），
      条数 121 / 122 / 123（37 份 122），`booleans_total` 只有 106 / 107 / 108 三种值，而 `booleans_true`
      从 32 到 63（35 份 61）。名字里点了 Word 的四条 40 份**全写**，可 `MsWordUlTrailSpace` 40 份全 false、
      另外三条多数 true，而 `tbox-lo.odt` 三条全 false、`images-float.odt` 只错开一条 —— 「兼容模式」在
      这一族是四条各管一件事的开关，不是一枚版本号。
    - 跨族**只核对同名，不译语义**：与 OOXML 裸开关同名的只有一条 `DoNotBreakWrappedTables`
      （首字母大小写正好差一位），40 份 odt 里 2 份写它，而带那枚开关的 .docx 有 4 份 —— 一问两转，两头各丢。
      同一组配置在另外两族也在（15 份 .ods 里 14 份 39 条、`workbook-settings.ods` 40 条（多的那格是 `CodeName`）、12 份 .odp 里 11 份写 42 或 43 条、1 份没有 settings），
      **但那四类点了 Word 的名字一条都没有** —— 所以这一本只在 office-doc 交。
    - 缺键 = 这一族没这一层：RTF 与遗留 .doc 连 `layout_compat` 这个键都不出现（`.doc` 的兼容位在 FIB 的位段里，
      而改那些位要 Word 本尊，本族料的 .doc 全出自 LibreOffice，判不住就不报）。放映设置那一类开关同理还没开。
    - `--limit` 只砍列表、砍不动算术（与事实 131、132 那两本同一条规矩）：宏文档限到 2 时 `named` 交按文档顺序的
      前两条，而 `named_names` 仍四条、`children_total` 5 不动；`nset.odt` 限到 1 时 `compat_items` 只剩按名字排的
      第一条（正是同名那条），而 `compat_item_total` 5、`items_total` 123、`booleans_true` 63 全按整本数。
    - 第二读者是 `office_reader.py` 的 `docx_layout_compat()` / `odf_layout_compat()`（两处出口：office-doc 的
      OOXML 支与 ODF 支，与 Rust 的调用点对称）；probe 的 3bm 把**每一份 .docx / .docm / .odt** 的整本逐键与它对，
      再钉上面那几条数（含 `w:val` 一条没写、六种搭配、两条空账），最后钉 RTF 与 .doc 那两族这个键**不在**。

132. **正文里那只手指的账：三种点法、三条来路，而「解到哪一格」与「算出什么色」是两问**（144 个 OOXML 包 / 34067 条指针）
    - 事实 131 数的是格子里写了什么；这一本数的是正文**怎么指过去**：Word 的 `w:color/@themeColor`、
      DrawingML 的 `a:schemeClr/@val`、Excel 样式上的 `theme="N"`。三条点法各自数得回来
      （28266 + 5691 + 110 = 34067），走过 2005 个 `.xml` 部件、`parts_unread` 3 个读不开（那三件是 `customxml-lo.docx` 里被 LibreOffice 清空的 0 字节 `customXml/itemN.xml` —— 件在而里面没字，不是解析器不行），
      其中手指在场的 589 个、主题部件 206 个（与事实 131 同一数，两问共用一份底账）。
    - 名字到十二格有三条来路，逐条分列而不是合成一个「解出率」：名字本身就是一格（`via = name`）6702、
      Word 那一族的别名表 24342（这一族只写四个别名：`background1`→lt1、`dark1`→dk1、`text1`→dk1、
      `text2`→dk2）、文件自己写的 `a:clrMap` 631、Excel 的序号 110，剩下 2282 条交 null。
    - 三条来路按族拆开各交各的，这也是「同一个名字在两种包里两个答案」的另一半凭据：
      Word 那 82 个包 29601 条里名字自己是一格 4185、四个别名 24342、解不出 1074；
      Excel 那 39 个包 677 条里名字 12、序号 110、解不出 555；幻灯片那 23 个包 3789 条里
      名字 2505、`a:clrMap` 631、解不出 653。横着加回来才是那五个数：6702 / 24342 / 631 / 110 / 2282。
    - 那 2282 条不是「读不到」而是文件自己没说，而且两个方向算出同一个数：`phClr` 2223 条
      （主题占位色，压根不是那十二格之一）+ `dark2` 12 条（5 份件的 `word/styles.xml`，`crep-r.docx` 是头一份：这个名字
      既不在十二格也不在那四个别名里，而它们又都带着 `themeShade`，两条理由同时成立）
      + `tx1` / `bg1` 那 678 条里落在没写对照的包里的那 47 条（678 − 631 = 47 —— 走对照解出的那 631 条仍是 `tx1` 与 `bg1` 两个名字。
    - `a:clrMap` 只有幻灯片这一族写：23 份 pptx 全写（85 份部件写着对照，而主题部件里一份都不写），
      Word 与 Excel 那 121 个包一个都不写；85 份说的都是同一套十二对（`alias_conflict` 0，`alias_names` 276 = 23 × 12）。于是同名的一指
      在两种包里是两个答案：`deck.pptx` 的 62 条 tx1/bg1 由文件自己解到 dk1/lt1，而 `chart-lo.xlsx`
      那 8 条同一名字落在没写对照的包里就交解不出（`slot` null、`via` null，部件是
      `xl/charts/style1.xml` 与 `style2.xml`）—— 不是替文件猜一个补上。
    - Word 那一路是自己跟自己核对的，所以这一支的「判得住」是硬的：那 29601 条里 29328 条 `w:color`
      每一条都另写了一遍六位实色当影子（`skip_no_literal` 那 273 条没写影子），无修饰符的
      25272 条与本包主题那一格逐条对上、`mismatched` **0 条**；带 `themeTint` / `themeShade` 的 3723 条
      不判（与事实 131 里「tint 与 shade 两支都判不住所以不交色」是同一个决定），点不出格的 333 条不判。四种分列，加上对上的那些正好是那一路的 29021 条，一条也没被揉成一格。
    - 序号那一族两读都交，因为这里真有两个答案：110 条点的是 `1` 与 `4` 两个数字。`1` 那 102 条按规范的
      槽位顺序说 lt1，而 Excel 自己实际用的那张映射表说 dk1（`index_disagree`）；`4` 那 8 条两边说的是
      同一格 accent1（`index_agree`）。谁胜出不是这本的活，把两格并排放着（`slot` 与 `alt_slot`）才是答案。
    - 一条手指 14 格：`part` / `kind` / `at` / `holder` / `name` / `slot` / `via` / `alt_slot` / `literal`
      / `tint` / `shade` / `mods` / `in_slots` / `matches`。合计里 `by_holder` 说坐在哪个座位上、
      `by_name` 说名字的分布：Word 那一路 25635 条 `w:color` 全在 `color/rPr` 这一个座位上，而 DrawingML
      分在六处（`solidFill` 3654、渐变stops 的 `gs` 1165、`fontRef` 102、`lnRef` / `fillRef` /
      `effectRef` 各 94、`colorStyle` 24、`bgRef` 12），Excel 的 110 条序号又分五处（`color/font` 49、
      `color/rPr` 53、页边 `left` 1 / `right` 1 / `top` 2 / `bottom` 1）与 `bgColor/patternFill` 3。
      第一条钉在 `bkmks.docx` 的 `word/styles.xml`：名字 accent1 本身就是一格、影子实色 `365F91`、
      这一格带着 `themeShade=BF`（`shade` 交的就是文件写的那个 BF），于是 `matches` 交 null
      —— 带修饰符的不判，要判就得先算色。
    - 分家要按包族按 glob 分，不能按「这个包里有没有 `w:color`」分：74 个 Word 包里有 6 份一个
      `w:color` 都不写（`notes-end.docx` / `notes-foot.docx` / `nset.docx` / `nset-lo.docx`
      / `pnum.docx` / `tbox.docx`，各 6 条 DrawingML 指针、共 36 条），拿「写了才算这一路」去数会量成
      68 个包 / 26806 条而没人报错。三族各自数：Word 74 包 26842 条（932 个部件，25635 + DrawingML 1207）、
      Excel 39 包 677 条（370 个部件，567 + 序号 110）、幻灯片 23 包 3789 条（553 个部件，全是 DrawingML）。
    - 限额这一格只管列几条，不管这份包里的账（与事实 131 同一条规矩）：`bkmks.docx` 用 `--limit 5` 交 5 条，
      而 `total` 与合计那一本仍然是 543 条的账（`parts_scanned` 13、`parts_with_refs` 3、`matched` 466
      一个都不动）；整库被 400 截住的只有那 39 个 Word 包，`cut` 就是说给你听的。
    - ODF 那 70 份（44 文字 / 14 表格 / 12 演示）压根没有主题这个概念：交零条而不是缺键——
      键一个不少、十二格每格都空着，而 `parts_scanned` 是 218 / 76 / 68、`parts_unread` 0，
      `total` / `listed` / `cut` 也是 (0, 0, False) 的样子；「这一层不存在」与「我没读」是两件事。
      老的 `.doc` / `.ppt` / `.xls` 与 RTF 目前**不交** `color_refs` 这个键：那一份主题数据坐在 CFB 的
      `Theme` 流与 RTF 的 `{\*\themedata}` 群里，本机做不出凭据，所以那一本还没开——开不了就说没开，
      别交一本零条的账冒充读过了。
    - 第二读者是 `office_reader.py` 的 `theme_refs()`（六处出口挂钩：doc / sheet / slide 各 OOXML 与 ODF
      一支，与 Rust 的六个出口对称）；probe 的 3bl 把**每一份 docx / docm / xlsx / pptx / odt / ods / odp**
      的整本逐件账与合计那一本逐键对（含 `mods` 那串数组与 `matches` 的 null），再钉上面这几条数、
      两读的 `slot` / `alt_slot` 配对，以及那四族 `color_refs` 键的有无。

131. **主题那一本账：十二格颜色有两套写法、字体角色有三态，而 tint 认得、shade 不认**（202 份 theme 部件 / 140 个包）
    - 老的只交了一份文件名清单（`themes` 把包名以 `ppt/theme/` 开头的成员列出来给 pptx），问不出「这十二格各写了什么颜色」「字体角色到底填了没」。这一本改成逐件一本账，再另记一本整套包的合计。
    - 颜色有两套写法要分开数：`a:srgbClr/@val` 直接给十六进制，而 `a:dk1` 这一格常写成 `a:sysClr` —— 颜色坐在 `lastClr`，`val` 那格是系统名字（`windowText` / `window`）。`written` 交文件写的那串，`system` 只在 sysClr 那格非 null，`sys_clr` / `srgb_clr` / `other_kind` / `empty_slot` 四格并存，不折成一枚「有没有颜色」。
    - 三格 `@name` 是三套各自独立的分布（196 件量出来的）：theme 是 189 个 Office Theme + 7 个 Office，clrScheme 是 188 Office + 8 LibreOffice，fontScheme 196 全写 Office，而 fmtScheme 只在那 69 件里写了名字、另 127 件空着 —— 所以 `fmt_named` / `fmt_unnamed` 与 `by_theme_name` / `by_scheme_name` 各记各的，不互相推。
    - 字体角色是三态不是两态：`latin` 392 个角色全写了，`ea` 与 `cs` 各 166 写 / 226 交空串 / 0 缺键 —— 空串是生产者明说「这里没有」，缺键才是压根没写，所以 `*_blank` 与 `*_missing` 分列。`a:font` 整册 6710 行，逐角色的行数分布是 0 有 166、29 有 70、30 有 156，而 `script="Hans"` 的 typeface 全场只有一个值（宋体）。
    - 两个生产者落在同一份件上是相反的两件事，钉在 `bkmks` 与 `deck-lo`：LibreOffice 重写 word 那一份时把 dk1 从 sysClr 换成 srgbClr、把 `objectDefaults` 与 `extraClrSchemeLst` 整个丢掉、并留空 fmtScheme 的名字，而十二格颜色与 60 行 `a:font` 一个字没动；它重建 deck 时把 24 个角色的 `a:font` 全清光（`faces` 0、`roles_without_faces` 24）却给 ea 填上 DejaVu Sans —— 「保住了字面」与「丢掉了清单」是两问。
    - 两条并集是量出来的巧合而不是规则：dk1 用 sysClr 的那 69 件，与写了 `extraClrSchemeLst` 的那 69 件，正好是同一批（Word 的行为）。`fmt` 那一份清单 196 件全是 fillStyleLst / lnStyleLst / effectStyleLst / bgFillStyleLst 各 3 条，`slot_total` 全是 12、`canonical` 196/196 —— 顺序按文件自己的文档顺序交、不排，所以这一格说的是「这批里没有一份打乱过」，不是「不可能打乱」。
    - `--limit` 只切列表不切算术：`deck-lo.pptx` 限到 5 时 `listed` 5、`cut` true，而 `totals` 仍然是 12 件 / 144 格（`theme_parts` 与 `slots` 不跟着缩）。
    - 这一本**不算色**：正文的指针可以带 tint / shade，而两支都判不住。tint 那 515 条与 `c×t + 255×(1−t)` 逐格吻合（515/515），按字面读「掺进 t 的白」却一条也不吻合（0/515）—— 同一枚数两种读法正好相反，认哪一种都是替文件猜；shade 那 2186 条没有一种算法说得出全部：候选铺到 32 种（RGB 与 HSL 两个空间 × 四种取整 × 四种式子）也只有 1078 条被其中某一说中，而「往黑压」那一式对上的 98 条底子全是 `000000`（黑往哪个方向仍是黑），等于没判。所以每一格只交文件写的那串，模块里连 `tint` / `shade` 两个函数都不存在。
    - ODF 三族（odt / ods / odp）压根不带 theme 部件，出口交零本账而不是缺键；老的 `.doc` / `.ppt` / `.xls` 与 RTF 目前**不交** `theme` 键（CFB 里那条 `Theme` 流与 RTF 的 `{\*\themedata` 还没接）。probe 用「键在不在」钉住这两种差别的不同：前三族是「读了，没有」，后一族是「这一版还没读」。
    - 第二读者是 `office_reader.py` 的 `theme_ledger()`（六处出口挂钩：OOXML 三族各一处、ODF 三族各一处）；probe 的 3bk 把**每一份 docx / docm / xlsx / pptx / odt / ods / odp** 的整本逐件账与合计那一本逐键对，再钉上面这几条数与那三族键的有无。

130. **哪几格并成了一块：三种拼法一份账，而「几条」与「这块底下有没有字」是两问**（`merges` 那一家三份件）
    - 老的 `merged` 只是一枚数（OOXML 数 `<mergeCell>` 的条数，ODF 只数**有字**的合并格），问不出「哪一块」与「这块底下有没有字」。这一本把每一条区间摊开，再去对文件自己声明的那个 `count`。
    - OOXML 的 `ref` 有两种写法要认：可以**没有冒号**（`ref="A12"` 是合法的单格「合并」，这一本给它名字叫 `solo`），可以带 `$`（`written` 照文件原样交，`anchor` / `end` 交解出来的规范地址）。
    - 生产者的三件事是量出来的：同一条 `merge_cells` 调两遍 **openpyxl 自己去重**，所以 `duplicated` 在这一族只能由 `cell_merges.rs` 的手搓单测顶；互相盖住的两条它照写（`A3:A5` 与 `A3:C5`）；LibreOffice 重写同一份时把**单格那条与重叠里较小那条一起丢掉**，`count` 改成 3、重叠归零。
    - ODF 既没有区间串也没有 count：跨度写在格子自己的两个属性上，区间从锚点加出来，所以 `declared` 与 `declared_matches` 交 **null 而不是 0**。
    - 两本账的关系钉在这三份件上：OOXML 族内 `merged == merges.total`（5=5、3=3、1=1 各一张表），跨族不成立 —— `只有合并块` 那张表 `merged` 是 **0** 而 `merges.total` 是 **1**，因为空锚点这一本是数进来的。
    - 三格三本口径：`covered_cells` 是每条 `cells - 1` 的累加（`merges.xlsx` 第一张表 18、重写那份 16、`.ods` 第一张表 16），**与 ODF 那个 `table:covered-table-cell` 元素无关**（有意不读）；`overlapping` 只数后来那一条（一对算 1 不是 2）；`anchors_with_text` 只数带字的锚点。
    - 反着写（`D1:A1`）与解不动的（`nope`）三种件都做不出来，只能手搓：前者几何按 min/max 摊平照报、另给一枚 `reversed`，但不进任何几何合计；后者只留原样串与那枚有无字，几何全交 null、`bad_ref` 立起来。
    - 第二读者是 `office_reader.py` 的 `merge_ledger`（两家共用那本算法）；probe 的 3bj 把**每一份 .xlsx 的每一张表与每一份 .ods 的每一张表**的整本区间账逐键对，再钉上面这几条数。限额那一格同样测了：`cut` / `listed` 只管 `rows`，十二枚合计仍是整份的账。

129. **域那一份账：一枚域一行，三家把「域」写成三种形状，而种类与开关是同一把尺子**（`fields-mix` 那一家 + `fields` / `fields-lo` / `toc.rtf` / `lists.rtf`）
    - 为什么另起一本：`structure.fields` 那一格数的是**正文里出现过的域标记**（`fields.docx` 交 9 = 三枚域 × begin/separate/end），
      而「这份文件里有哪些域」要按域逐行列、还要跨部件那一跳 —— 同一份件里那一枚页码域坐在 `word/footer1.xml`，
      正文那棵树根本看不见它。两格并存，各自口径写在键名上。
    - OOXML 有两种形状，各交一条链的账：复杂式是 `w:fldChar` 的 begin → `w:instrText`\* → separate → 结果 → end，
      简单式是 `w:fldSimple` 一行装完。链断在哪就报在哪：`markers` 数文件里写了几枚，`loose` 数游离在外的
      （没有 begin 就出现的 separate/end），`unclosed` 数开了头没闭合的那一行（`closed` 交 `false`，不替它补一句）——
      「没带 separate」与「没闭合」可以是两个不同的行号（`fields-mix.docx` 第 10 行与第 11 行）。
    - 没闭合那一枚不是「少一句」而已：它的链还开着，**后面每一段的字都被算进它的缓存**（第 11 行 depth 仍是 0，
      缓存串却吞下了第 12 行那句「空指令」的字；第 12 行开在这条链里，所以 depth 1）。这就是 `nested` 那一本存在的意思。
    - `pieces` 记的是生产者把**同一条指令切成几段 `w:instrText`**（Word 切两段、LO 一段），
      与 `markers.instrText` 那个总数不是同一问；`empty_instruction`（只有空白）与 `no_instruction`（一段都没写）
      也是两格 —— LibreOffice 那份正有一枚 begin 什么指令都不写，所以它的 `instrText` 12 比 `begin` 13 少一根。
    - LibreOffice 重写同一份稿子改了六处，账本一处都不遮：15 行 → 13 行（两枚 `w:fldSimple` 被摊平成复杂式，
      `forms.simple` 2 → 0）、`\* MERGEFORMAT` 剩一枚、`\r` 一枚变两枚、`w:dirty` 整族不写（`markers.dirty` 1 → 0）、
      separate/end 全补齐（`unclosed` 1 → 0）、`STYLEREF` 的缓存换成那句错误文字「错误: 引用源未找到」。
    - ODF 没有「域指令」这个东西：种类就是元素名（认得 42 个 `text:` 名字，其中 7 个的缓存文字实测量过，其余进 `unmeasured`），
      `instruction` 与 `switches` 两格整本为空。跨族对应只在这里成立：`MERGEFIELD` → `text:database-display`；
      而 `HYPERLINK` → `text:a`，它**不是一门域**，所以 `kinds` 里查不到 hyperlink。两枚 `text:bookmark-ref` 靠
      `text:reference-format` 分成 number / page 两种读法（种类数 2、格式各 1）；序列号得先有声明
      （`sequence_declarations` 6 条，名字另交 `sequence_declared`）。页码那一枚住在哪一份件里有两个答案：`fields.odt` 那一枚在 `styles.xml` 的页版式里（`parts` 因此是 content.xml 3 + styles.xml 1，这一本两份件都要扫），而 `fields-mix.odt` 那四枚全写在正文里。
    - RTF 一枚 `\field` 群一行，`control_words` 与行数同数。群里的指令写成双反斜杠，解一遍之后与 docx 的
      `w:instrText` **逐字同一个形状**，所以种类与开关两族共用一把尺子；但这一族解完顺手 trim，docx 那本交原样
      （同一枚 REF：`fields-mix-lo.docx` 交 ` REF _RefMix1 \r \r \h `，前后两个空格留着；`fields-mix.rtf` 交 `REF _RefMix1 \r \r \h`，两格空白没了 —— 差的就是那两格），因此**比开关不比整串**。显示文字取 `\fldrslt`；
      群里连字都没有时 `instruction` 与 `kind` 一起交 null，而那一行照样在账里 —— 群在场就是文件写过。
    - 开关这把尺子的口径：只认**单反斜杠 + ASCII 字母或 `*`**（`\h`、`\*`、`\o`、`\r`）。`\@` 与 `\-` 那种
      带引号格式串的不算开关（`DATE \@ "yyyy-MM-dd"` 的种类仍解为 `DATE`）—— 要扩就三家一起扩，别一家先扩。
    - 第二读者是 `office_reader.py` 的 `docx_field_ledger` / `odf_field_ledger` 与 `lyco_rtf.py` 的 `field_group_row`；
      probe 的 3b6 把**每一份 .docx、.odt、.rtf**（71 / 41 / 17 份，共 129 本账）的整本域账与读者逐格对，
      再钉上面那几条跨族与跨生产者的数。限额那一格也测了：`--limit 3` 只截 `rows`，
      `fields_total` 与 kinds / switch_tokens / parts 三本簿仍是整份的账。

128. **文字走向那五处各说各的：一家写在元素身上，一家全在样式里，而样式有两种词法**（`dir-cell` 那五份件 + `dir-cell.rtf` 不交）
    - OOXML 是五处五本账：格上的 `w:textDirection`（五个枚举 `lrTb` / `tbRl` / `btLr` / `lrTbV` / `tbRlV` 按写的交，
      一个不折算）、表身的 `w:bidiVisual`（python-docx 写出来是**空元素** → `present` true 而 `val` null，在场就是开着）、
      段上的 `w:bidi`、run 上的 `w:rtl`、节上的 `w:bidi`。`dir-sect.docx` 只点节那一处：只看格与段的读者会把这份件
      报成「没有走向这回事」。
    - LibreOffice 的 docx → docx 重写做了四件事：丢掉两个「说了等于没说」的正向枚举（有值的格 5 → 3）、把 `tbRlV`
      换成 `tbRl`（字头方向没了，所以 `textDirection=tbRl` 那枚数是 2）、给 `w:bidiVisual` 补上 `w:val="true"`、
      给节补一枚 `w:textDirection w:val="lrTb"`；反过来把段上那句从 2 段摊到 11 段（其余每段各补一句 `w:val="0"`，
      关掉也要写出来）。同一份稿子两处一处多一处少 —— 不存在「这份文档是不是竖排」这么一个数。
    - ODF 一处都不写在正文上：段与格只点样式名，那句话坐在样式的 `style:table-properties` /
      `style:table-cell-properties` / `style:paragraph-properties` 上；**页面那一处的父元素不是 `style:style`**，
      而是 `style:page-layout` 里的 `style:page-layout-properties`（实测 `Mpm1`）—— 四种父元素，一处也不并。
    - 两种词法必须分开交：`loext:writing-mode` 是 LibreOffice 扩展，**局部名与 `style:writing-mode` 一模一样**
      （`bt-lr` 那一格走的正是它），只按局部名收就会互相盖掉，所以每一行带 `vocabulary`。枚举也不通用：ODF 多一枚
      `page`（这张表自己不说、由页面定），OOXML 那边根本没有这个值。
    - 「点了样式名」与「样式说了话」是两个数：`dir-cell.odt` 十四段全部点了样式，其中 13 段的答案来自**命名样式**
      `Standard` 那枚默认值（`declared_in` 是 `styles`、`style_part` 是 `styles.xml`），只有第 2 段是自己那份自动
      样式 `P1` 写着 `rl-tb`；`paragraphs_from` / `cells_from` / `tables_from` 三本各数 automatic / named / other，
      「文件说了」与「默认值替它说了」不混成一个数。
    - 跨族两处各改一次：节上的 `w:bidi` 变成页面版式那条 `rl-tb`，表身的 `w:bidiVisual` 变成表样式那枚 `rl-tb` ——
      同一句「整份倒过来」在两族落在不同的层，两边都按自己写的词交，不互相翻译。
    - RTF **不交这个键**：那一族把同一件事写成 `\cltxtbrl` ×2、`\cltxbtlr` ×1、`\rtlrow` ×2（OOXML 写在表上的
      一句在这里落到**每一行**）、`\rtlpar` ×1、`\ltrpar` ×27（默认值被逐段重发）、`\rtlcol` 0；归属要按行群与
      格群切开才判得住，而本读者在这一族连「几张表」都判不住（见事实 100），所以规则记在模块头上、交回来的是缺键
      而不是一个猜的数。
    - 第二读者是 `office_reader.py` 的 `docx_text_direction` / `odf_text_direction`；probe 的 3b5 把全语料的 docx 与
      odt 整本走向账与它对，再钉上面那几条跨族的数与那五份件各自的账。

127. **表上那张位图四族四本账：`other_anchors` 与 `also_object` 就是「这个不是坏掉的位图」的两种说法**（`sheet-pictures` 那一家四份件 + `chart` 三份件）
    - 共享的一份账八格：`drawings`（画法部件几份 —— ODF 这一族交 null，它没有部件那一层可数）、
      `total`（**数在截断之前**）、`distinct_media`（按图部件地址去重）、`unresolved`（配不上地址的）、
      `missing_media`（地址写了而部件不在包里）、`other_anchors`、`listed`、`cut`。前两个数与中间两个
      数的口径不同：`total` 是全量，`unresolved` / `missing_media` 只在看得到的那几行里数 —— 一把算不出来。
    - `distinct_media` 摆的就是那一句「几张图」与「几个图部件」：openpyxl 那份 sheet1 是 5 个锚块 4 个
      地址（红点、蓝点、另一张红点、JPEG，加一条断的），LibreOffice 重写后是 4 个锚块 3 个地址 —— 同一个
      `image1.png` 被两条关系指着。两份都原样交，不并成一把。
    - `other_anchors` 是这一批改出来的闸门：画法部件里不只有位图，图表走的 `graphicFrame` 也挂在锚块上。
      `chart.xlsx` 与 `chart-lo.xlsx` 各有 2 个这种锚块，旧版本把它们算成**两张坏掉的图**（`unresolved` 2）。
      判据是文件自己写的：那个锚块里有没有 `xdr:pic`。没有就是别的形状住在画法层，交 `other_anchors`，不进 `total`。
    - `also_object` 是 ODF 那一侧同一件事的第二种形状：`chart.ods` 的 `数据` 表里两个 `draw:frame` **既写**
      `draw:image`（预览缓存：`Pictures/…jpg`，8550 / 8484 字节）**又写** `draw:object`（图表本体，指 `ObjectReplacements/Object 1`、
      `Object 2`）。这一族把「图」与「嵌入对象」在同一条 frame 里并排写，只数 `draw:image` 就会把图表的预览当成位图，
      所以每条交一个布尔，两本账都在 —— 与事实 112 那条嵌入对象那一份账是同一件事的两个视角。
    - 同一个家族里两个生产者写成两套明细，两处都记：openpyxl 用**三种锚元素名**（`twoCellAnchor` /
      `oneCellAnchor` / `absoluteAnchor`）、不写 `xfrm`、锚块一个属性都不写、blip 上写 `cstate="print"`；
      LibreOffice 只用 `twoCellAnchor`、把区别写进 `editAs`、每个 `pic` 补一份 `spPr/xfrm/off+ext`、不写 `cstate`，
      连同一个「跨三格」的 EMU 也是另一组数（95250 vs 95040）。**元素名相同不等于摆法相同，摆法相同不等于数相同。**
    - 断掉的那条关系两家待遇不同：openpyxl 那份留着 `rId5` 的号，地址与字节全 null（`unresolved` 1，而
      `xl/media/image5.png` **还在包里** —— 号配不上，不是部件丢了，所以 `missing_media` 是 0）；LibreOffice 重写时
      把那个锚块整个删了（sheet1 的 `total` 5 → 4）。丢掉的不替它补，断的不替它圆。
    - `.xls` 只交两格：`shapes`（0x005D 里偏移 4 的类型 = 8 的那几条，5/1/1/0）与 `drawing_records`（0x00EC，
      5/1/1/1 —— **没有图的那张表也写了一条**）。图的字节全在整本共用的 0x00EB（偏移 1054、正文 1217 字节）的
      嵌套层里，按表分不出来，所以那一族不硬凑一个「每张表几张图」。三条 BLIP 的自报长度 178/171/722、字签都在
      正文第 61 字节、从字签到末尾正好 117/110/661（与三份 OOXML 报的媒体字节同一把尺，跨族自证）。
    - 走不通的两条路在 biff 那一层就交实底：嵌套记录自报的长度装不下 → 那条照收、字段全 null 然后止步；
      OfficeArt 的容器（0xF000 / 0xF001）钻过 8 层就不钻。这两条真件做不出来，是 `biff.rs` 里自己拼字节测的。
    - 反面对照：`book.xlsx` / `hidden.xlsx` / `cell-links.xlsx` / `size.xlsx` 四份 xlsx、`book.ods` /
      `cell-notes.ods` / `errors.ods` / `print-area.ods` 四份 ods、`book.xls` / `hidden.xls` / `cell-links.xls`
      三份 xls 全报 0（键在、值为零；`book.xls` 连 0x00EB 也写，正文 106 字节，而一条 BLIP 也没有）。
    - 第二读者：`office_reader.py` 的 `xlsx_pictures_by_sheet`（三跳；认画法部件只看结尾 `.xml` 与名字里有
      `/drawings/`，不看关系的类型名）、`ods_pictures`（一跳）、`lyco_legacy.py` 的 BIFF 分支（0x005D / 0x00EC /
      0x00EB 与 OfficeArt 0xF007 的嵌套走法，深度闸 8）。probe 的 3a6d 把四份件整本账逐行逐字段对，再钉 `chart`
      三份件的 `other_anchors` / `also_object` 与那十一份反例。

126. **格子里的链接四种存法各交各的账：来路不同就不并成一个形状**（`cell-links` 那一家四份件）
    - 四条来路，`family` 与 `hop` 说清这一条走的是哪一路：xlsx 的地址在**这一张表自己的关系表**那一跳上（格子里只有 `r:id`，
      站外开关 `TargetMode` 也写在那一跳上，没写就交 null，所以 `external` 是三态而不是两态）；`.ods` 把地址**挂在段的字上**
      （`text:a/@xlink:href`，没有第二跳）；`.xls` 写成**同一条流里的一条 0x01B8 记录**；而 `=HYPERLINK("…","…")` 谁也不挂 ——
      它是公式，所以另计 `formula_cells`（xlsx 与 `.ods` 数得出，`.xls` 的公式是二进制 ptg 串、这一版不反汇编，那一格就交 null 而不是 0）。
    - 「站外 / 站内」这一刀四族是四把，各按各的文件写：xlsx 看关系表写没写 `TargetMode="External"`；`.ods` 没有那个开关，
      只能按地址的长相分（认得出 scheme 的算站外、`#` 开头算站内）；`.xls` 看记录里名字串后面那 16 字节是不是**第二个** GUID ——
      两条分支的长度字段数的东西不一样（一支字节、一支码元），所以判据不能是长度。同一个站内跳转是三个串：xlsx 与 `.xls` 写
      `'数据'!A1`，`.ods` 写 `#'数据'.A1`（点号不是感叹号），三个串各自原样交，不折成一个。
    - 两个生产者对同一批字给两份账，两份都原样交：openpyxl 那份 `口径` 6 条（`with_id` 5、`with_location` 1、`with_tooltip` 1、
      `with_display` **0**、`unresolved` 1 —— 就是那条关系号被删掉的），LibreOffice 重写后 5 条（丢的正是那条指不到的；
      `with_display` **5**，`G7` 的 display 干脆就是地址本身；tooltip 一个不写；`mailto` 的 subject 从 `%E9%A2%84%E7%AE%97` 变回「预算」）。
      丢掉的不替它补，改口的不替它圆。
    - 同一批字过一遍 ODF 又是一次改口：`.ods` 里 `mailto` 的百分号写法回来了，而 `G7` 那条（xlsx 里那一格没有字）变成**格子的字
      就是那个地址**。所以这一族没有「四份件一字不差」这种话可讲，只有四本账各摆各的。
    - `.xls` 另有一本自报的数：0x01B7 那条记录自报链接条数，实测 LibreOffice 那份写的是 **0**，同一条流里却有 **6** 条 0x01B8。
      文件自己写的按写的交（`workbook.links_written`，一条数一个），数出来的另放一格 —— `links/total`、`links/records` 与 `links/whole`
      说的是「解出来的 / 记录有几条 / 自报字数切满的」三问，不是一把。
    - 每行只交这一族写了的东西：`id` 与 `tooltip` 在 `.xls` 是 null（这一族没有那两样），`range`（首末行列四个数）只有 `.xls` 交 ——
      那条记录写的是**行列范围**，单格时 `ref` 就是那一个格，跨格才成 `A1:B2`。
    - 反面对照：`book.xlsx` / `hidden.xlsx` / `book.ods` / `book.xls` / `hidden.xls` 五份没链接的件报 `workbook.totals.links` = 0
      （键在、值为零），而 `book.xls` 连 0x01B7 也写（`links_written` 是 `[0]`）。
    - 第二读者：`office_reader.py` 的 `xlsx_sheet_links` 与 ODS 每张表的 `links`（`_ods_link_nodes` 递归下去，`office:annotation`
      那一棵子树不进去 —— 批注的字不是链接的字），`lyco_legacy.py` 的 0x01B7 / 0x01B8 分支只吃标准库，那两个 u32、GUID、
      码元数与 UTF-16 名字一条一条切，切不动的那条 `continue` 而不是硬凑。probe 的 3a6c 把四份件逐行逐字段对，再钉 `C3` 那三种写法。

125. **RTF 的段流水：一段一行整份列，为的是「第 5 段是什么」这一问**（15 份 .rtf 全过）
    - 已有的三本都是**筛过的**：`headings` 只交标题行、`numbering.list` 只交列表项、
      `contents.entries.list` 只交目录条目。拿它们拼整份是拼不出来的，而 `toc-full.rtf`
      正是那个反例 —— 「结构：一级」在文件里有两处（正文标题一处、目录条目一处），
      按字去对号会把一条对成两条。所以 `structure.para_flow` 是**同一批账的第二次交法**：
      一次走出来的 `para_rows` 一段一行，按文件切段的顺序整份列，谁也不筛。
    - 不另起解码器：这一本就骑在 markdown 那一批用的同一辆车上（`para_rows`），
      段号跟着 `\par` 收，所以「在不在目录域里」（`in_index`）与「在不在列表里」（`in_list`）
      是同一行上的两格，两问各答各的。每行交 `at`、段的字、段自己点的样式号（`style_index`）
      与那个号在样式表里的名字（`style_name`）、样式名解出的层级（`heading_level`）、
      `\ilvl` / `\ls`、`level_found`、那一级写的 `nfc`、文件自己写下的标签（`label` /
      `label_written`），解不出的样式号另计 `styles_unresolved`（实测 15 份全是 0）。
    - 两本「有几段」是**一道减法**：`paragraphs` = `structure.paragraphs` + `empty`
      （量过 15 份：`toc-full.rtf` 28 = 24 + 4、`notes.rtf` 9 = 7 + 2、`toc.rtf` 11 = 9 + 2、
      `lists.rtf` 与 `tables.rtf` 没有空段所以两个数相同）。`empty` 说的是「这一段切出来
      没有字」，与 Word 自己统计口径的 `empty_paragraphs` 不是同一个问，所以两格各交各的。
    - 目录那一问在这本里另有一个数：`toc.rtf`（有域、没重排）的 `in_index` 是 **1**
      而 `contents.entries` 是 **0** —— 域里那一段占位文字在流水里算一段（它确实在群里），
      但它不是条目（没 `\tab`、没页码、点的是 `Normal`）。**「段在域里」与「段是条目」是两问**，
      这一本只答前一个。
    - `rows` 听 `--limit` 的话（`listed` / `cut` 说截了多少），**上面那七本计数不跟着截**：
      `lists.rtf` 截到 3 行时 `paragraphs` 仍是 7（这条也钉进 probe 的 3b4）。
    - 第二读者是 `lyco_rtf.rtf_text(..., with_rows=True)` 交出的同一份 `para_rows`，
      probe 的 3b4 把 15 份 .rtf 逐行比对，并另钉两道：每份的减法
      （`paragraphs` == `structure.paragraphs` + `empty`）与 `toc-full.rtf` 前六行
      （目录那两段的样式号 140 / 141 与名字 `toc 1` / `toc 2` 一起交）。

124. **.ppt 里「这一块是什么」写在它前面那条四字记录里；段的项目符号在这族没有凭据**
    - 怎么量的：手上 `.ppt` 只有一份（`deck.ppt`），一个问题一份件不算量过。于是把三份现成的
      `.pptx`（`deck-ph-lo` / `deck-tables-lo` / `deck-hidden-lo`）用
      `soffice --headless --norestore --convert-to ppt` 各转一份 —— 同一篇稿子的两副面孔，
      可以横着对。前两份进了库，第三份只作对照（它证明的是隐藏页那一问，与这一条无关）。
    - 每一块文字**前面**都有一条 `recType 0x0F9F` 的四字记录。pptx 那头 `p:ph/@type="title"`
      的那一块，在这里写的数值是 **0**；正文占位符、自由文本框与被摊平成一块块字的表格格子
      写的都是 **4**；母版与版式里那些块的标题也是 0、正文是 1。三份进的件里 5 个标题块
      一字不差，没有出现第三种数值 —— 所以账上只交 `type_written`（文件写的数）与整截字，
      **不背规范里的名字**（那份规范手上没有；而 LibreOffice 连 MS-PPT 说容器该写的版本半字节
      都没写 `0xF`，靠版本位判容器在这位生产者上直接走不通，页归位用的是「正文能不能铺成
      一条完整的记录流」，见事实 84 那条）。
    - 反过来，有一件**没做**的要写清楚：段这一级没有「这一项带项目符号」的凭据。
      `deck-ph-lo.ppt` 里，pptx 那头带 `a:buChar` 的两段与带 `a:buNone` 的两段，转过来
      那段属性（`0x0FAA`）的载荷**一字不差**（只有开头那个「字属性流有多大」在变）。
      所以 `--markdown` 的第六族不做：不是「还没搬」，是这一族的文件里没那句话
      （对照：pptx 写 `a:buChar`、odp 写 `text:list-header` 上的 `buChar`、RTF 写 `levelnfc`）。
    - 顺带量到一处形状：pptx 那张 3×3 的表在 .ppt 里是**八块文字**（一格一块，两块里
      有文件自己写的 `\r`），所以「这页有几块字」与「这张表有几格」不是同一个问，
      两边各交各的，不拿一边替另一边圆场。
    - 还有一颗没人用的常量要记下来：`ppt.rs` 里 `TEXT_BYTES = 0x0FA8` 在这三份新件里
      **一次也没出现**，而出现成对的是 `0x0FA1` 与 `0x0FA6`（46/46、42/42、36/36），
      它们的载荷不是字。常量留着（删它是另一件事），但这一族的 8 位文本原子这条分支
      **没有真件命中过** —— 说读过就是假的。

123. **RTF 也交目录的缓存条目了：同一个「容器里有几段」，三家是三个答案**（`toc-full.rtf`）
    - 生产者与另两族同一条路（LibreOffice 的 Basic 宏：load → refresh fields → index.update()
      → storeToURL，见事实 119），所以三份件是同一篇文档的三种排法，可以横着对。
    - 这一族既没有 `w:sdt` 那个壳，也没有 `text:index-body` 那个块 —— **域本身就是壳**，
      所以「哪几段算条目」只能按字节跨度判：`{\field{\*\fldinst { TOC …}}{\fldrslt …}}`
      那一跳量到关掉 `\field` 的 `}` 为止，段自己那个 `\par` 落在里头才算一条。
      判在段收尾的那一刻，所以搭 `para_rows` 那辆车（`in_index` 一格），不另起解码器。
    - 同问三个答案：标题「目录」那一行在 docx 是 `sdtContent` 的直接孩子（`paras` 3 而
      `entries` 2），在 ODF 嵌在 `text:index-title` 里（`paras` 2 == `entries` 2），
      在 RTF 它**在开域之前**就 `\par` 了（`\s139`，样式表里那名字就叫 `TOC Heading`），
      所以这一族的 `scope` 里 `paras` 只有量出来的两条。三本账都交，不折成一个数。
    - 级别与 docx 同一个取处（段自己点的样式名），只不过这一族写的是 `toc 1` / `toc 2`
      —— 小写、数字前有个空格（那是样式表里的**显示名**，样式号是 140 / 141，两个都交）。
      同一把 `level_from_style` 吃下 `TOC1` 与 `toc 1` 两种拼法，`level_from` 都是
      `paragraph-style`。页码还是制表符之后的字面（`{` + `\tab 1}`：`\tab` 后面那一个空格是
      控制字的界限符，不进字），所以 `tab_written` 与 `page_written` 是一对凭据。
    - 两处读法上的坑，都是量出来的：第一条的段属性写在**开域那一段**上（`\field` 之前
      的 `\s140`），只在 `{\fldrslt` 群里找 `\s` 会一条也读不到；第二条才自己写
      `\pard\plain \s141`。段号跟着 `\par` 收，所以两条都拿得到自己的号。
    - 条目这头**一个地址也不写**（`with_anchor` 0、`targets_found` 0 都是数过了的零），
      而**被指那几段**写着 `__RefHeading___Toc52_744132712` 那类书签（`target_marks_written` 2）。
      两头各交各的：这一族没有那一跳可走，不拿「两条对两个标题」的顺序去替它连上。
      `toc.rtf`（有域、没重排）是同一问的反面：`paras` 1、`entries` 0 —— 那一段字既没有
      `\tab` 也没有页码，而它点的样式是 `Normal`，所以 `level` 也是 null。
    - 第二读者是 `lyco_toc_entries.rtf_toc_entries`（跨度判在 `lyco_rtf.py` 的走查里，
      与 Rust 同一条规则各写一遍），probe 的 3b2 把 15 份 .rtf 整本对，2b 另比整份 `contents`。

122. **RTF 是 markdown 的第五族：三条规矩都是从真件量出来的，不是照规范推的**（15 份 .rtf 全过）
    - 做法上最要紧的一条是**不另起解码器**：先拿一次性脚本自己解 RTF，`第一行` 被解成 `ff：aff`
      那种垃圾（`\uN'?'`、`\'hh`、`{\*…}`、`{\v0 …}` 要一起对才解得对，而 `rtf.rs` 那台三千行的
      扫描机早就做对了）。这一族只读它扫过一遍的结果里新加的 `para_rows`（一段一行：`at`、
      段的字、段点的样式号与那个号解出来的名字、层级、`\ilvl` / `\ls`、那一级自己写的
      `levelnfc`、文件写下的标签）。`headings` 与 `numbering.list` 两本都是**筛过的**，
      拿字去对号会错 —— `toc.rtf` 里「一级标题：预算口径」既是目录条目又是正文标题（两行都在）。
    - 第一条：段里**已经带着**文件自己写的那枚列表标签。`{\listtext\pard\plain  1.\tab}` 不是
      带 `\*` 的那一类（不会整群跳过），所以 `1.\t编号列表第一项` 整截都在正文里；要加 markdown
      的记号必须先摘掉那一截（`labels_stripped` 记摘了几枚），否则两个号叠在一起。
    - 第二条：圆点还是编号看**那一级定义自己写的** `levelnfc`（实测 23 是圆点、0 是阿拉伯数字），
      不看样式名 —— `List Bullet` 这种名字是给人看的；号指不到那一级（`\ls` 点了个不存在的号）
      的那一段**不替它编记号**，按正文排并计 `items_unlabelled`。
    - 第三条：表在这一族是**一整段带制表记号的字**（`tables.rtf` 十段里有 5 段带制表记号），
      所以 `--markdown` 交的不是表格结构：制表记号按 docx 那一族同一口径换成一个空格（`tabs`
      记几段有过），`tables` 交 **null 而不是 0**（看了，这一族判不住几张表 —— 事实 100）。
    - 空白段照旧不进口正文，只计 `empty_dropped`（`toc.rtf` 是 2）；遗留 `.doc` 这一族**还是不交
      这个键**（缺键 = 这一族没搬）。第二读者是 `lyco_rtf.rtf_markdown`，probe 的 3b3 逐份比整本。

121. **同一个格子两家的字不一样：折行尾是读者的活，不是替文件说话**（`pipes.xlsx` 与它的两份重写）
    - 现象：`--markdown` 第一次跑就把一格铺成 `第一行\r<br>带`，而镜像读者给的是 `第一行<br>带`。
      两本账的 CSV 那条 lane 一直没红，是因为这副件是这一批新做的 —— 旧库里根本没有带换行的格。
    - 为什么文件里有 `\r`：openpyxl 在 Windows 上把值里的 `\n` 写成 `\r\n` 塞进 `<t>`；
      LibreOffice 重写同一格时写成 `&#10;`（量过：`pipes.xlsx` 三个 CR、`pipes-lo.xlsx` 与 `pipes.ods` 零 CR）。
      也就是说这一条差异是**生产者**的，不是读者的选择。
    - 规范怎么说：XML §2.11 要求处理器把输入里的 `\r\n` 与裸 `\r` 当成一个 `\n`；
      而 `&#13;` 那样由字符引用解出来的 CR 是文件明确要的那个字符，**不**在归一之内。
      ElementTree 就是这么做的（所以镜像一直是 LF）。
    - 于是改的是 Rust 这一侧：`xmlscan::unescape` 现在**先**折行尾**再**解实体
      （`fold_newlines` → `unescape_entities`），正文与属性值同一口径。
      顺序反了就会把 `&#13;` 也折成 LF —— 那才是替文件说话。
    - 存量影响是零：库里所有 CR 都在 XML 声明那一行（`?>` 之后，不经过 `unescape`），
      16 个带 CR 的部件逐个量过；Rust 侧也没有一条期望串里带 `\r`。
      新加的三条断言在 `xmlscan` 的测试里（CRLF、裸 CR、`&#13;` 不折）。
Rust 测试里每个期望值都来自第二读者对这些文件的独立读取：
`scripts/acceptance/office_reader.py`（OOXML / ODF / MS-CFB / OLE 属性集，只用标准库）、
文档里那几张图在它的 `docx_picture_rows()` / `odt_picture_rows()` 里：两处尺寸各自换算、
两处替代文字、两处锁、绕排与摆放那三合一的元素，与 Rust 逐字段整份对（`.docx` 三副、`.odt` 两副）。
RTF 那第三种存法在 `lyco_rtf.py` 的 `picture_ledger()`（三种单位、形状属性那一对一对、
数据头八个字节与折行），三份件（`images.rtf` / `notes.rtf` / `toc.rtf`）逐张整份对。
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
主题那一本在同一份 `office_reader.py` 里（`theme_ledger` / `theme_part_row` / `theme_totals`）：
十二格按文件的文档顺序交、不排，`sysClr` 的 `lastClr` 与 `val` 分两格，字体角色的空串与缺键分两列，
`unread` 那一行把十八个键一个不少地交出来、只是每格没内容 —— 与 Rust 的 `theme_ledger.rs` 逐键同形，
键名两边各列一份清单，少一个键 probe 的整本相等断言当场就红。
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

153. **公式引用了哪些格子：两族两种拼法，冒号后面那一段也要再点一次**
    （`shared.xlsx` / `shared.ods` 那一对，加上语料里全部 19 份带公式的件；`cell_deps` 挂在
    `formula_elems` 这本账里，因为它吃的就是同一份 `text`）
    - OOXML 写 `A1*2`、`SUM($B$2:$B$9)`，ODF 写 `of:=[.A1]*2`、`of:=SUM([.B2:.B3])` ——
      引用一律**照文件写的原样交**：不展开区间、不去 `$`、不补表名，一个区间算一条引用。
    - 三处不挡就数出不存在的格子：字符串字面量整段先剔掉（`HYPERLINK("https://…/formula",…)`
      里那个 `formula` 与 `A1` 形状的片段都不是引用）；后面紧跟 `(` 的不算（`LOG10(` 的 `LOG1`
      会撞进「1~3 个大写字母 + 数字」这个形状）；前面是字母数字或 `$ ! : .` 的不算。
    - 第一版把 ODF 的区间读丢了：`[.B2:.B3]` 是**两段各点一次**，冒号后面还带一个点，
      当时那个模式不许它带，于是 12 条引用被数成「方括号里是别的东西」（`brackets_other`）。
      量出来才发现 —— 括号里的东西要先长得像格子才算引用，其余另交一格。
    - 全库摊开：74 行写了正文的公式里 59 条引用、36 条是区间、24 条带 `$`、12 条点了表名；
      15 行一个引用都没有（`1/0`、`TRUE()`、`"甲"&"乙"`、`NA()`、HYPERLINK 那两条），
      那一支叫 `rows_literal_only` 而不是被丢掉。**自引用 0 条**：本库没有一份公式指向
      自己所在那一格，所以 `self_refs` 是数出来的 0，而 `.ods` 那 16 行连地址都没有
      （`holders_known` 0），那一族连「是不是自引用」都问不出 —— 两回事各自交。
    - `cross_sheet` 恒交 null：OOXML 手里只有部件名（`xl/worksheets/sheet1.xml`），
      没有一张表名可以去对上 `Sheet1!` 那个前缀，判不住就不填 0。

154. **行/列分组（分级显示）两份账：元素自己的 `outlineLevel` 与 `sheetFormatPr` 的声明级。**
    四族出口那一格的键都叫 `outline`，住在每张表自己身上（`sheets[*].outline`）而不是
    `layout` 里 —— ODF 与 .xls 两族没有尺寸账那一层，塞进 `layout` 会让三条出口各走各的路。
    `groups.xlsx`（openpyxl）与 `groups-lo.xlsx`（LibreOffice 另存同一份）：
    - 行 3-4 一级、行 5-6 一级且折叠隐藏、行 7 二级；列 B-C 一级、列 D 二级且隐藏。
    - openpyxl 只给**分了组**的那 5 行 3 列写 `outlineLevel`，既不写 `collapsed`，
      也不给 `sheetFormatPr` 写 `outlineLevelRow/Col` —— 声明那一格就是空的（null），
      实算那一格是 2；LibreOffice 每条行都写一句 `outlineLevel="0" collapsed="false"`
      （所以「说过话」8 条、「分了组」5 条是两个数），声明写 2/2；没分组的第二张表它
      照样写声明 `0/0` —— 「写了 0」与「没写」在出口上必须分开。
    - 列范围两家不同：openpyxl 一列一条（3 条盖 3 列），LibreOffice 把 B、C 并成
      `min="2" max="3"`（2 条盖 3 列）—— 按元素个数数列会少报一列。
    - **ODF 与 .xls 两族的存法已经量过**：ODF 里 `grep -c outline content.xml` 是 0，
      分组写成**嵌套的** `<table:table-row-group>` / `<table:table-column-group>`，
      元素不带属性，级别＝嵌套深度、成员＝顺序加 `number-*-repeated`，折叠写
      `table:visibility="collapse"`。`groups.ods` 进语料的直接后果是揪出一处两家共有的
      盲区：两边的 ODS 遍历原先都只取 `table` 的**直接子元素**（Rust 那侧是 `all()`，
      它的定义就是「按名字筛直接孩子」；深看要用 `descendants()`），于是被 group 包住的
      第 3-7 行整批看不见 —— 第一张表量到的对比是**行元素 3 条 vs 8 条、隐藏行 0 vs 2**
      （CSV 少 5 行）。现在两家都改成走全部后代，这一条由 probe 里
      「三种存法报同一个数」那道闸门钉住。级别那一份账也读了：`groups.ods` 第一张表
      行方向两层嵌套包 5 条成员（展开 5 行、折叠 2 行）、列方向两层嵌套包 2 条元素
      （展开 3 列、折叠 1 列），最大级 2 —— 与 LibreOffice 那份 xlsx 一字不差；
      而 openpyxl 那份列方向 grouped 是 3（一列一条）。这一族说不出来的三格
      （元素自己写的级、collapsed、`sheetFormatPr` 的声明级）交 null，不拿 OOXML 的语义补。
      **`.xls` 那一族也读了**（`groups.xls`，同一份内容让 LibreOffice 另存）：级不在属性里
      也不在嵌套里，而在两个字的位上 —— 行是 `ROW` 正文偏移 12 那个字的**低三位**
      （同一格的 bit5 就是既有隐藏行账用的那一位），列是 `COLINFO` 的 grbit（偏移 8）
      **第 8-10 位**（bit0 是这段列隐藏）。量到的数与 LibreOffice 那两份 xlsx/ods 一字不差：
      行 [[5,5,2,2]]、列 [[2,3,1,2]]。`collapsed` 这一族没地方说（LO 从不写 MS-XLS 说的
      info1 fCollapsed 那一位，偏移 4 恒 0x0005；`GUTS`(0x0100) 一条都没有），
      加上「位一直都在、问不出写没写」，所以 `level_spoken`、`collapse_spoken`、
      `collapsed_grouped`、`group_elements` 与两个声明值一律交 null。

155. **数据区（`table:database-range`）与筛子：区住在工作簿，筛是区的孩子。**
    17 份 .ods 里 3 份写 `<table:database-ranges>`：`book.ods` 与 `locked-sheet.ods` 各一条区，
    `size.ods` 两条区（其中一条带筛，见下面第二条），区自己只有两枚属性（`table:name=预算表`、
    `table:target-range-address=预算表.A1:预算表.B3`），**没有 `display-filter-buttons`**，
    也没有任何筛（`with_filter` 0、`conditions_total` 0）。三条要紧的：
    - 区不写在表上，工作簿里也没有一张指针说「这是哪张表的区」—— 归属只能从地址前面
      那个表名读（`sheet_from_address`）；而同一份件里 `table:named-*` 的地址是带 `$` 的
      第三种写法（`$预算表.$A$1`），两族地址不能互推。
    - **这条已经进语料**：`size.ods` 就是让 LibreOffice 把带 `autoFilter` 的 `size.xlsx` 转成
      .ods（本机实测 `size.xlsx` 与 `size-lo.xlsx` 两份源件转出来形状一致，取前者）。筛整个搬进
      **区**，成为
      `<table:filter><table:filter-and><table:filter-condition table:value=甲
      table:operator="=" table:field-number=0/>`，区自己还多写
      `table:display-filter-buttons=true`；同一份里另一条区（`table:name=台账`，来自 OOXML
      的表对象）不带筛 —— 所以「几个区」与「几个区在筛」是两个数，不能并成一个。
    - 被筛掉的那一行在 ODF 里写 `table:visibility=collapse` —— 与手工分级折叠**同一个词**
      （这一族没有 `collapsed` 也没有 `filterMode`），于是「为什么看不见」问不出来；
      条件那一本现在有样本了（元素名是 `table:filter-condition`，不是 `table:condition`）：
      `size.ods` 交 `total=2 / with_filter=1 / conditions_total=1`，两条区都写
      `table:display-filter-buttons=true`，而 book.ods 那条区干脆没写这枚（null 不是 false）。

156. **ODF 的条件格式不写在表上、也不叫 `style:conditional`，而是单元格样式身上的一条 `style:map`。**
    `rules.ods` 就是 LibreOffice 把带 cfRule 的 `rules-lo.xlsx` 转成的 .ods（配方在
    `scripts/office_fixtures.py`）。量到的四件事：
    - 整本 `<style:conditional>` **一个也没有**（20 份 .ods 全是 0）—— 这是读到的 0，不是没读；
      谁要是按子串 `conditional` 普查，就会把「LO 不写条件格式」当成结论，那是假阴性。
    - 条件落在 **content.xml 的自动样式**上：`ce2` / `ce3` 两条 `family="table-cell"` 的样式各挂
      `<style:map style:condition="cell-content()&gt;100" style:apply-style-name="ConditionalStyle_5f_1"
      style:base-cell-address="规则.B2"/>`。「这是给哪个格子写的」在 `base-cell-address` 里，
      而 OOXML 那一族是区间（`sqref`）在规则身上 —— 同一个功能两族各写各的半边。
    - 「满足之后长什么样」不在同一条元素里，而是指到一条具名 table-cell 样式
      `ConditionalStyle_5f_1`（`display-name="ConditionalStyle_1"`、`parent-style-name="Default"`），
      它的 `fo:color="#9c0006"` 就是 OOXML 那条 dxf 写的 `FF9C0006` —— **dxf 的颜色穿过转格式活下来了**。
    - **同名两义要分家**：`styles.xml` 里那 24 枚 `<style:map style:condition="value()&gt;=0">`
      住在 `number:number-style` 里，是数字格式的正负分支（格式码 `[>=0]` 那一层），每份 .ods 都是 24 枚，
      与条件格式无关。判定同时看父样式的 `style:family` 与条件串的前缀，两本账各数各的
      （`number_format_maps` 与 `maps_total`）。跨格式同问也是两个答案：OOXML 第一张表 3 个区间共 4 枚规则，
      .ods 只剩 2 枚条件 —— 只留 `cell-content()` 说得出来的那几条，不替它补回去。

**157. 数据透视表：OOXML 摊成四类部件，ODF 收成一棵树（`pivots`，`pilot.xlsx` / `pilot.ods`）**

    同一份合成数据（区域 / 品类 / 月份 / 数量 / 金额，36 行）先由 openpyxl 写成种子表，
    再让 **LibreOffice 自己挂两枚数据透视表**：`--convert-to` 不做这件事，得走 pyuno socket 桥
    （LO 自带的 `program/python.exe`，见 `scripts/office_fixtures.py:add_pivot_tables`）。
    Pilot1 摆成区域在行、品类在列、月份在页、数量求和 + 金额平均、「Data」假字段也在行；
    Pilot2 只留区域在行 + 数量求和。同一份内容存成 `.ods` 与 `.xlsx` 各一份。量到的五件事：
    - OOXML 一枚表摊在**四个地方**：表本体 `xl/pivotTables/pivotTable1.xml`、缓存定义
      `xl/pivotCache/pivotCacheDefinition1.xml`、缓存正文 `pivotCacheRecords1.xml`，
      外加 `xl/workbook.xml` 里那条 `<pivotCache cacheId="1" r:id="rId5"/>`。两枚表共用一条缓存
      （所以 `caches_written` 是 1 而不是 2）：「几枚表」与「几份缓存」是两个数。
    - **表上不说自己属于哪张工作表**：归属在那张表自己的关系表里（两条 `pivotTable` 类型的关系）；
      表上那条关系只指缓存定义部件，跳到缓存靠 `cacheId` 对上工作簿那一条。
    - **表上也没有字段名**：`pivotField` 只写 `axis` 与下标，名字要回头查缓存的 `cacheField` 名单；
      轴上的 `<field x="-2"/>` 那个负数是这一族表示「Data」假字段的方式（页轴用的又是另一个元素
      与另一个属性名：`<pageField fld="2">`）。
    - ODF 全收在 `content.xml` 的一棵 `<table:data-pilot-tables>` 里：字段名直接写在
      `table:source-field-name`，没有缓存部件可跳。而 LibreOffice 这两枚表**没写**
      `table:source-range-address` —— 数据源在哪也没落进文件，那一格交 null，
      不拿 OOXML 那一份补过去；写着的只有落点 `销售.I1:销售.O17` 与五枚按钮地址。
    - 同一枚表两族各报各的落点：ODF 从 I1 起（页轴那几格算在里面），OOXML 的 `location/@ref`
      写 I4:O17 并把 `firstHeaderRow` / `firstDataRow` / `firstDataCol` 分开交 —— 差的行数是
      这一族自己的排法，不换算。`.xls` 的透视表在 BIFF 记录流里，本机没有第二个读者能核对，
      所以 `pivots` 这个键在该族**整个不在场**（`book.xls` 读到 null，不是读到 0）。

**158. 手动分页符：OOXML 写在表部件的两个容器里，ODF 写在行列的自动样式上（`print_breaks`，
`breaks.xlsx` / `breaks-lo.xlsx` / `breaks.ods`）**

    生产者：openpyxl 往一张 24 行的表上写三道行分页符（**故意**把 `id=9` 写两遍）与一道列分页符，
    第二张表什么都不写；再由 LibreOffice 把同一份分别重存成 `.xlsx` 与 `.ods`。量到的五件事：
    - OOXML 的两个容器各自自报两个数：`@count` 与 `@manualBreakCount`。这一份 `count="3"` 而文件里
      确实三条 —— 但只有**两个不同的 id**。「声明几条」「实数几条」「几个不同的号」在这本账里是
      三个键（`declared` / `found` / `distinct_ids`），不替文件去重。
    - 没写容器与写了容器但一条都没有是两件事：第二张表两族的容器都不在场，所以 `present=false`
      而 `found` 是 0，`distinct_ids` 交 null（无从可数）。
    - LibreOffice 重写同一份**不无损**：行那本剩 2 条（重复的那道被去掉）、`man` 从 `1` 换成 `true`、
      属性顺序从 `id min max man` 换成 `id man max min`，而列那段的 `max` 从 `16383` 换成 `65535`
      —— 那正是两族各自的最大列数写法，两家各按各的文件交。
    - ODF 根本没有「分页符」元素：断页写在行与列**自己的自动样式**上
      （`<style:table-row-properties fo:break-before="page"/>` 与列那一份同名属性），所以要跳一跳。
      第一张表 24 行全都说了话（`resolved` 24、`style_missing` 0）而只有 2 行是 `page`（同一枚
      `ro2` 用了两次），其余 22 行明写 `auto` —— `auto` 与「整条属性没写」是两件事，值本身留着。
    - 同族另一条常被忽略的形状：列那本只有 3 条元素，第三条带着 `number-columns-repeated="16382"`
      —— 一条元素顶 16382 列（LibreOffice 把每张表补到 16384 列），所以「几条元素」「盖几列」
      在这族里永远是两个数，这里按写的原样交，不展开。
    - `.xls` 的分页符在 BIFF 的 0x001B / 0x001A 记录里，那些 16 位行号数组本机没有第二个读者能核对，
      所以该族不交这个键（`book.xls` 读到 null，而不是读到 0）。语料 304→307 份。

**159. 页面网格与默认制表位：文档级一条 + 每条节一本（`grid_tab`，无需新件）**

    这一本不用新凭据 —— 仓库里 94 份 .docx 全都写着这两处，而两种生产者恰好各写各的半边。
    量到的四件事：
    - 文档级：`word/settings.xml` 里 `<w:defaultTabStop w:val="720"/>`。94 份**每份都写**，
      值只有两种：`720`（python-docx 模板那条）与 `1134`（`pnum.docx` 那一份自己改过）。
      交的是文件写的字符串，不换算成厘米或字符宽。
    - 节级：每条 `w:sectPr` 里一条 `w:docGrid`。python-docx 只写 `w:linePitch="360"`；
      LibreOffice 重写同一份时补齐成 `w:type="default" w:linePitch="360" w:charSpace="0"` 三条。
      所以 `kind` / `line_pitch` / `char_space` 三格各按各的写与不写交：
      「没写 `w:type`」与「写了 `default`」是两个答案（全库 54 条前者、48 条后者），
      而 `charSpace="0"` 与「没写 charSpace」也是两句话。
    - 节上也能写一条 `defaultTabStop` 覆盖文档级那一条：这批件里**一条都没有**，
      那一格交 null，不拿文档级的值补。
    - 两头各有得失：python-docx 在节上留三枚 rsid（`rsidR` / `rsidRPr` / `rsidSect`），
      LibreOffice 重写后一枚都不留 —— 所以 `written`（原样属性表）与 `written_names`
      （按名字排序的集合，问的是「写了哪几条」而不是「按什么顺序」）两格并存。
    - `.odt` 这一族一个都没写 `style:default-tab-stop`，RTF 与 `.doc` 没有这一层，
      所以 `grid_tab` 在那三族的出口里**整个不在场**（读到 null，不是读到 0）。

**160. 主题里那三本样式表：三本列表在 `fmtScheme` 底下，而真正的效果在 `a:effectLst` 里层**

    这一本也不用新凭据 —— 仓库里 175 份 OOXML 件（docx / xlsx / pptx 三家）带着 239 份主题部件。
    量到的形状（由直接读 zip 的第三方脚本数出，不是两份读者自证）：
    - `fillStyleLst` 每主题三条：一条 `solidFill` + 两条 `gradFill`。两条渐变**全走 `a:lin`**
      （`ang="16200000" scaled="0"` 一个字都不差，304 枚里 126 枚额外带 `a:tileRect`），
      而本仓**一条 `a:path` 都没有** —— 所以 `path` / `path_shape` 常态是 null，
      那是「没走这条」而不是「没有渐变」。停止点 `a:gs/@pos` 只有 `0` / `35000` / `80000` /
      `100000` 四种（万分之一，按写的字符串交），颜色 804 枚全是 `schemeClr val="phClr"`。
    - `effectStyleLst` 每主题三条，而 `a:effectStyle` 的直接孩子只有 `a:effectLst`（openpyxl /
      python-pptx 那份的第 3 条额外带 `a:scene3d` + `a:sp3d` 两枚壳，本仓 87 份件有）。
      **问「这条样式有没有阴影」要再跳一跳**：里层只有 `a:outerShdw` 一枚（239 份部件共 267 枚），
      写的是 `blurRad` / `dist` / `dir` / `rotWithShape` 四枚（`algn` 一次都没写），它的孩子是
      `srgbClr val="000000"`。`a:innerShdw` / `a:glow` 三家一个也没写。
    - `lnStyleLst` 每主题三条 `a:ln`，`w` / `cap` / `cmpd` / `algn` 四枚每次写满；里面的孩子是
      三种组合：`solidFill`+`prstDash`（267）、`prstDash`+`miter`（261）、只有 `prstDash`（189）。
      子元素只报名与自己写的属性，里面那层填充色不替它猜默认值。
    - LibreOffice 重写不是无损的：仓库里 73 对「手写 vs 重打」中，渐变整本丢掉的 14 对
      （三条填充全成 `schemeClr` 单色）、里层阴影丢掉的 **71 对**、两家都写着阴影的 **0 对** ——
      而 `effect_styles` 每次还是 3 条，所以「效果样式还在」与「效果样式里还有东西」是两件事，
      里层交空数组而不是少一条样式。
    - `.ods` / `.odt` / `.odp` / RTF / `.doc` / `.xls` 里 `fmt_styles` 这个键**整个不在场**
      （ODF 的样式表住在自己的 `styles.xml`，遗留那几家的主题在 CFB 的 `theme` 流里、
      RTF 是 `{\*\themedata}` 那一段 base64，本机都没有第二个读者能核对）。

**161. ODF 条件格式还有第三种存法：`table:conditional-format`，而 `apply-style-name` 有两本名表**

    仓库里 20 份 .ods 只有 `rules.ods`（LibreOffice 从带 8 枚 `cfRule` 的 `rules-lo.xlsx` 转出来的那份）
    写了这一族的新写法，共 3 条。量到的四件事（用直接读 zip 的第三方脚本数过，不是两份读者自证）：
    - 区间写在 `@target-range-address` **一枚属性**里，而一条可以塞好几段：第二条是
      `规则.A2:规则.A6 规则.B2:规则.B4`（空格分隔）。所以账里同时交原样串与分词后的段数，
      全库 3 条规则共 4 段 —— 只数「几条规则」会把那两段区间读成一段。
    - 里面挂的孩子有三种词汇，各按各的交：`table:condition` 两枚、`table:icon-set` 一枚
      （`@icon-set-type="3Arrows"`，带三枚 `table:formatting-entry`）、`table:color-scale` 一枚
      （带三枚 `table:color-scale-entry`）。
    - 条件式本身也有两种写法并存：比较式 `>100` 与 `formula-is([.$B2]>200)`，都带
      `@base-cell-address`，按写的字符串交。
    - **`@apply-style-name` 有两本名表**：在那 480 条老 `style:map`（每份 .ods 24 条，条件写
      `value()>0` / `=0` / `<0` / `>=0`）上，它点的是 `number:*-style` 的名字（`N116P0` 那一类 =
      数字格式的正负三段，480/480 全在这一本里解到）；而在用户写的条件格式上，它点的才是
      `style:style`（同一件里 `cell-content()>100` 那两条指的 `ConditionalStyle_5f_1` 就在这一本）。
      只建样式名表会把 480 条全报成「点不到」，那是读法错而不是文件缺。反过来，新写法的这 2 枚
      condition 点的 `ConditionalStyle_1` / `ConditionalStyle_2` **两本都解不到**（转格式时老 map
      换了名，这一本没换）—— 交 null，不替它猜「应该是指那条」。

**162. 「这份文档还能动吗」在一族里有三处可说，而真件只说了半句**

    `office-doc` 的 `protection` 这一本以前只报「开了没开、限制类型是什么」，看不出**这一枚到底写了
    几行字**。补完之后量到的三件事（用直接读 zip 的第三方脚本数过 810 份真件，真件不进仓库）：
    - 自产的 `protected.docx` 与 LibreOffice 重写的 `protected-lo.docx` 在 `word/settings.xml` 的
      `w:documentProtection` 上写满九枚属性名：`cryptAlgorithmClass` / `cryptAlgorithmSid` /
      `cryptAlgorithmType` / `cryptProviderType` / `cryptSpinCount` / `edit` / `enforcement` /
      `hash` / `salt`（两家一字不差，重写把这一枚整枚照搬）。
    - **真件不是这个形状**：本机 810 份 OOXML 件里带这一枚的只有 4 份，而那 4 份**只写了
      `w:enforcement`** —— 没有 `@edit`、没有 crypt 那一串。所以 `edit` 交 null（那不等于
      「不限制类型」），`written_names` 把写了的按写的交回来；`enforcement_written` 留原样串，
      因为这一族说「开」有 `1` / `true` / 空串三种写法，而「没写」是第四种。
    - 同族另两处 —— `w:writeProtection`（Word 的编辑限制与打开密码）与 `w:readOnlyRecommended`
      （建议只读）—— 在自产件与那 810 份真件里**一个都没有**，本机也没有会写它们的生产者；
      这一支只交 `elements` 里那两个 0（0 是「看过了没有」，缺键才是「没看」），不写读法。
    - ODF 那一族完全不写 `documentProtection`：文档级的开关在 `settings.xml` 的 config-item 上，
      `protected.odt` 写的是五枚 —— `ProtectForm=false`、`ProtectBookmarks=false`、
      `ProtectFields=false`、`LoadReadonly=true`、`RedlineProtectionKey` 空串。同一份 docx 转成
      odt 之后那三个保护开关全是 false，而 `LoadReadonly` 留 true —— 所以「转格式丢了什么」
      要两本账并排看，不折成一个布尔。

**163. 样式表顶上那份内建样式清单：自报的 `count` 与真写的覆写条数是两个数**

    `word/styles.xml` 的第一块常常是 `w:latentStyles` —— Word 用它说「我没点名的那些内建样式按什么算」，
    再逐条 `w:lsdException` 覆写单个内建样式（名字、别名、下一段样式、`sortOrder` / `uiPriority`、
    `semiHidden` / `unhideWhenUsed` / `locked` / `qFormat`）。本仓 94 份 .docx 里 **80 份带这一块**，
    另外 14 份有 `styles.xml` 却不写这块（两种缺法都交：`part` 与 `block` 各说一件事）。
    - 那 80 份的头属性一字不变：`defLockedState=0`、`defUIPriority=99`、`defSemiHidden=1`、
      `defUnhideWhenUsed=1`、`defQFormat=0`、`count=276`；而实写的 `w:lsdException` 只有 **137 条**。
      276 是 Word 那份清单的总条数、137 是这份文件真覆写的条数 —— 所以 `declared_count`（按写的字符串）
      与 `exceptions_total`（数出来的）并排放，`declared_matches_written` 只说这一份对不对得上。
      全库对得上的那几份是特例，不是常态。
    - 逐条那一本按「每条写了哪几枚属性名」数（`attrs_written`），因为不同生产者写的枚数不同；
      `sample` 给前若干条原样（`--limit` 只管列几条，计数不跟着截）。
    - 对照：真件里带这块的 .docx 也几乎都写了 exceptions（本机 118 份里 117 份），
      所以「读不到」多半是解析没走到，不是文件没有。

**164. 表格样式身上那批条件分支：`w:tblPr` 那枚空壳与 `w:if` 一枚都没有**

    「这张表套的是哪个样式」写在表身上（见事实里的 `table_styles` 那一本），而
    **表头加粗、隔行底纹、四个角各长什么样**写在样式自己的 `w:tblStylePr` 上：一枚分支 =
    一个条件（`w:type`）+ 它要覆写的那几本盒子（`w:pPr` / `w:rPr` / `w:tblPr` / `w:tcPr`）。
    本仓 94 份 .docx 里 **80 份写了这批分支**、合计 **51920 枚**，另外 14 份有 `styles.xml`
    却一种表格样式都没有（LibreOffice 从 ODF 转回来的那一路就是这样，`table_styles_total` 交 0）。
    - 条件只有 `w:type` 这一种写法：十种取值（`firstRow` / `lastRow` / `firstCol` / `lastCol` /
      `band1Vert` / `band1Horz` 各 7840 枚，`nwCell` 2160 / `band2Horz` 1120 / `neCell` 1040 /
      `swCell` 560），而 `w:if` **全库一枚都没有**（本机另测的 33 份第三方 .docx 的 12332 枚里也是一枚没有）。
      所以 `if_written` 交 0 是有凭据的一句话，不是「没找到」。
    - **`w:tblPr` 在这批分支里全是空壳**：47388 枚没一枚写过属性或子元素（有内容的 0 枚）。
      `bkmks.docx` 的 649 枚分支里 `w:tblPr` 在场 546 枚、`w:tcPr` 在场 546 枚，而后者**全都有内容**
      —— 同一位置一枚空壳一枚有话，空掉那枚本身是一句说过的话（表级属性在条件分支里改不动）。
    - 底纹 33600 枚，`w:shd/@val` 只有 `clear` 这一种拼法；`@fill` 给实色、`@themeFill` +
      `@themeFillTint` 给主题那一格的指针与浓度（`LightShading` 的 `band1Vert` 两处一起写：
      `C0C0C0` 与 `text1` + `3F`，同时四条边全写 `nil`）。按写的交，不换算成实色。
    - 同一份内容 LibreOffice 重写后差三处：`w:tblPr` 从 546 枚补满到 649 枚（每枚分支都补一枚空壳）、
      另留 **7 枚空壳 `w:rPr`**（有内容的从 369 掉到 362）、`w:iCs` 那 28 枚整个不见，
      而浓度指针的大小写从 `3F` / `7F` 变 `3f` / `7f`。分支的种类与枚数两边一致 —— 都是写法，不圆场。
    - 反面凭据：这一层只住在 OOXML 文字那一家。本仓 51 份 .odt、18 份 .rtf、3 份 .doc 都不交这本账
      （读者侧与 lbin 侧都是缺键，不是空账）。

**165. 演示稿那张表的样式指针：`a:tblPr` 的空壳与那本一条声明都没有的清单**

    页上那张表的「表头算不算一行、隔行要不要底纹、长相指点给哪个表格样式」写在 `a:tbl/a:tblPr`：
    六枚开关（`firstRow` / `lastRow` / `firstCol` / `lastCol` / `bandRow` / `bandCol`）加一枚
    `a:tableStyleId` 子元素。包级另有 `ppt/tableStyles.xml`，根是 `a:tblStyleLst`，
    **它只写一枚 `@def`，一条 `a:tableStyle` 声明都没有**（本仓 17 份带这份部件的 .pptx 与
    本机 102 份第三方件全是这样）。
    - 三种状态要分开数：本仓 33 份 .pptx 共 18 张表，`a:tblPr` 18/18 在场，其中 **9 枚是自闭合空壳**
      （零属性零子元素）、9 枚写满 `firstRow="1" bandRow="1"` 与那枚指针；第三方那 32 张表则**全部**
      是空壳，零开关、零指针。「没写这一层」与「写了但里面是空的」在两格里各自说话。
    - 指针指不到包里的东西：`a:tableStyleId` 与 `a:tblStyleLst/@def` 都是同一个
      `{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}`，而清单声明条数为 0 —— 所以 `style_ids_declared`
      与 `style_ids_resolved` 都交 0，另用 `style_ids_same_as_default` 说「它等于包默认」这件真事。
      样式长相住在应用自己的画廊里，不在包里；本机也没有会写 `ppt/tableStyles/` 那种定义目录的生产者
      （LibreOffice 重写的那份连 `ppt/tableStyles.xml` 都不留，见 `deck-lo.pptx`）。
    - 开关的值按写的字符串交（`"1"` 与 `"0"`），不折成布尔 —— 两家生产者连写不写都不一样。


**166. 一份 .docx 里可以有两份样式表：`stylesWithEffects.xml` 不是超集，是另一份**

    `word/styles.xml` 是现在那份样式表，Word 那一路还会再写一份 `word/stylesWithEffects.xml`
    （给旧读者看的），并在 `word/_rels/document.xml.rels` 里**各给一条关系**
    （`Type=.../styles → styles.xml`、`Type=.../stylesWithEffects → stylesWithEffects.xml`）。
    本仓 95 份带样式表的文字件里 **45 份有第二份、50 份没有**；带第二份的 45 份里
    **逐字节相同的是 0 份**，而 LibreOffice 自己写的那一路（`bkmks-lo.docx`）只写一份、
    rels 里也只有 `styles` 一条 —— 「第二份在不在」是生产者习惯，不是文档内容差别。
    - **方向与直觉相反**：第二份的样式**条数更少**（`alternate.docx` 主那份 164 条、第二份 160 条，
      全库 45 份一律 −4 条）而**字节更多**（349,458 对 438,131）。逐本元素计数一起动：
      `basedOn` 158→154、`link` 38→34、`name` 164→160、`tab` 11→7、`uiPriority` 163→159，
      而 `latentStyles` / `lsdException` / `tblStylePr` 三本**一模一样**（1 / 137 / 649）。
    - 连根元素自己写的那句都不同：主那份 `mc:Ignorable="w14"`，第二份 `mc:Ignorable="w14 wp14"`。
      这是「哪一份声明了哪个命名空间可以忽略」的话，两份各自声明。
    - 这一本同时是本仓其它样式类出口的**适用范围声明**：`latent_styles` / `table_style_branches` /
      `doc_defaults` / 字符样式那一本**只读 `word/styles.xml`**。在那 45 份件上，那些数说的是
      主那一份，不是「第二份里同样的东西」—— 需要两份都用时按 `entries` 里的部件名分别取。
    - `differs_from_main` 只在真有第二份时交 true / false，**只有一份的 50 份交 null**；
      别把「没有第二份」读成「有两份且相同」。
