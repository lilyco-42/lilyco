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
| `styled-text.docx` | python-docx（`write_runs_docx`） | 一段只点一个字符属性（粗 / 斜 / 下划线 / 删除线 / 上标 / 红 `C00000` / 黄 / 9 磅写成 `sz="18"` 半磅 / 宋体），另有点「明确不粗」（`<w:b w:val="0"/>`）、一串字里两个孩子（`<w:b/><w:i/>`）与**一段里三种字各一串**；没格式那几串**不写 `w:rPr`** |
| `styled-text-lo.docx` | LibreOffice（`styled-text.docx` → .docx） | 同一份件重写一次：每一串字都补一个**空的** `<w:rPr></w:rPr>`（30 串里 16 串是空的），而 `w:val="0"` 换成 `w:val="false"` —— 「有没有这一格」与「这一格说不说不」两家正好一边一种 |
| `styled-text.odt` / `styled-text.rtf` | LibreOffice（从 `styled-text.docx` 导出） | 第三种与第四种存法：ODF 把格式搬到 `text:span/@text:style-name="T1"…T12"`，值在**同一份 content.xml** 的 `style:text-properties` 上（`fo:font-weight="bold"`、`style:text-underline-style="solid"`、`style:text-position="super 58%"`、`fo:color="#c00000"`），「明确不粗」成 `fo:font-weight="normal"`；RTF 只在群头写 `b` / `i` / `strike` / `super` / `cf23` / `highlight7` / `fs18` / `af9`，否定是 `b0`，CJK 的下划线落在 `aul` 那个口袋，而颜色与字体只是**一个号**，要跳文件自己那两张表 |
| `charstyles.docx` | python-docx（`write_styles_docx`） | 三段各点一个**字符样式**（`w:rStyle` 在 `w:rPr` 的第一个孩子位上），定义在 `word/styles.xml`：`Strong` 里写 `<w:b/><w:bCs/>`、`Emphasis` 里写 `<w:i/><w:iCs/>`，第二段还**同时**在段上写 `<w:b/>`（一处一半）；第三段点的号是 `SubtleEmphasis` 而名字写着「Subtle Emphasis」（带空格），定义里除了斜体还有 `w:color val="808080" themeColor="text1" themeTint="7F"` |
| `charstyles-lo.docx` | LibreOffice（`charstyles.docx` → .docx） | 重写留着 `w:rStyle` 与那三个号（`Strong` / `Emphasis` / `SubtleEmphasis` 一字未改），照旧给没格式的串补空 rPr（7 串里 4 串是空的）；样式定义自己那份也没动，只有 `w:rsid` 换了大小写 |
| `charstyles.odt` / `charstyles.rtf` | LibreOffice（从 `charstyles.docx` 导出） | 同一件话的另两种存法：ODF 把 `Strong` 换成 `Strong_20_Emphasis`（住 **styles.xml**，带 `style:display-name="Strong Emphasis"` 与父 `Default_20_Paragraph_20_Font`），而段上自己写的粗体变成 content.xml 里的自动样式 `T1` —— 于是那一句被**套成两层 span**（外 `Emphasis` 内 `T1`）；RTF 在群头写 `\cs34`，而 `{\*\cs34 … Strong;}` 那条定义**同时**被它把自己的 `\b` 抄进群头（号与话都在） |
| `sections.docx` | python-docx（`write_sections_docx`） | 两节：第一节点名页眉与页脚（`rId9` / `rId10`），第二节只补一条指着关系表里**不存在**的号 `rId999` 的偶数页页眉，所以它的页眉与页脚两格都是「沿用第一节」；两节都写 `w:titlePg`，settings 写 `w:evenAndOddHeaders`（不带值）。**顺带量到一条生产者脾气**：python-docx 新加的节默认 `linked_to_previous` —— 给第二节写页眉等于改写第一节那份 `word/header1.xml`，全件仍然只有两份页眉页脚部件 |
| `fields.docx` | python-docx（`write_fields_docx`） | 正文里三条域链（SEQ 编号 / DATE 带 `w:dirty` / PAGE），第四条 PAGE 写在 `word/footer1.xml` 里（不进正文那份账）；两个站内跳转：一个指着真书签 `表锚点`，另一个指着 `没这个书签`；书签是 `bookmarkStart` / `bookmarkEnd` 一对（`w:id="3"` 配对，名字只写在 start 上） |
| `fields-lo.docx` / `fields.odt` / `fields.rtf` | LibreOffice（从 `fields.docx` 导出） | 同一批域在三条来回里各变一次样：docx 重写丢了 `w:dirty`、给指令补一个尾空格、把日期格式里的 `-` 转义成 `\-`，并把缓存值换成它自己算出来的数；ODF 把 SEQ 拆成 `text:sequence`（`text:name="表"` / `text:formula="ooow:表+1"` / `style:num-format="1"`）**并往 `text:sequence-decls` 里补一条 `表`**，页码变成页脚样式里的 `text:page-number`，书签只剩名字不再有号；RTF 写成六条 `\field{\*\fldinst …}{\fldrslt …}`，中文序列名成了 `\u-30616\'3f` 一串码位转义 |
| `cell-links.xlsx` | openpyxl（`write_cell_links_xlsx`） | 一格只改一个变量：站外 http（`A1`）、`mailto:` 且 subject 用百分号写法（`B2`）、只有 `location` 的站内跳转（`C3`，没有第二跳）、`=HYPERLINK()` 公式（`D4`，它不写链接对象）、带悬浮提示的（`E5`，唯一一家写 tooltip）、关系号被删掉的（`F6`，格上留着 `r:id` 而那张关系表里没有它）、没有字的一格挂着一条链接（`G7`）；`数据` 表另给一条回跳 |
| `cell-links-lo.xlsx` | LibreOffice（`cell-links.xlsx` → .xlsx） | 同一批字的第二种写法：`F6` 整个丢了（6 → 5），每条补一个 `display`（`G7` 那个就是地址本身），tooltip 一个也不写，`mailto` 的 subject 从 `%E9%A2%84%E7%AE%97` 解回「预算」，关系号从 `rId1` 起重新编 |
| `cell-links.ods` | LibreOffice（`cell-links.xlsx` → .ods） | 第三种存法：地址挂在段的字上（`text:a/@xlink:href`，一条 `table:hyperlink` 也不写），站内跳转变成 `#'数据'.A1`（点号不是感叹号），`mailto` 的百分号写法又回来了，而 `G7` 那条地址被写成格子的字 |
| `cell-links.xls` | LibreOffice（`cell-links.xlsx` → .xls） | 第四种：同一条 Workbook 流里的一条 0x01B8 记录（显示字、地址、行列范围与两个 GUID 并排写），外部支与站内支的长度字段一个数字节、一个数码元；0x01B7 自报的条数是 **0** 而流里有 6 条 0x01B8 |

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
