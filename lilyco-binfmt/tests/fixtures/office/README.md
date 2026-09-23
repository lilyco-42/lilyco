# `lbin office-*` 的 fixture：每一份都有署名的生产者

这些文件**不是手搓的字节**。断言如果来自我自己写的字节，那就只证明了「我写的东西能被我自己读回来」，
不证明能读真实世界里的办公文件。所以这里的每一份都由一个独立程序写出，
重跑 `python scripts/office_fixtures.py --force`（需要 LibreOffice；python-docx / python-pptx /
openpyxl 装在 `D:/app/scoop/apps/python/current/python.exe` 那套解释器里，PATH 上的 uv python 没有）。

| 文件 | 生产者 | 里面故意放了什么 |
|------|--------|------------------|
| `notes.docx` | python-docx 1.2 | 两级标题、正文、2×2 表、站外超链接、一张 PNG、一条批注、分页符、核心属性 + 自定义属性 |
| `notes.docm` | 由 `notes.docx` 加部件 | `word/vbaProject.bin` + `vbaSignature.xml` 与对应内容类型：**合成**的宏样本（见下） |
| `book.xlsx` | openpyxl 3.1 | 三张表（其中一张 hidden）、公式、合并格、命名区域、表格部件、中文表名 |
| `deck.pptx` | python-pptx | 两页：标题 + 正文 + 备注 + 图片；第二页一张 2×2 表；4:3 尺寸 |
| `notes.odt` / `book.ods` / `deck.odp` | LibreOffice（从上面三个 OOXML 文件转来） | 真 ODF 写入者产出的三种 ODF |
| `notes.doc` | LibreOffice（从 `notes.docx`） | MS-CFB 复合文档 + WordDocument 流 + `1Table` 里的 piece 表 |
| `notes-en.doc` | LibreOffice（从纯 ASCII 的 `notes-en.docx`） | 中英一视同仁仍写 16 位 piece —— 记下这个事实，见下 |
| `book.xls` | LibreOffice（从 `book.xlsx`） | BIFF8：BOUNDSHEET（含隐藏表）、SST + CONTINUE、LABELSST / RK / FORMULA |
| `deck.ppt` | LibreOffice（从 `deck.pptx`） | PowerPoint 97 记录树 + 一个 59 万字节、走 FAT 的属性集流（大流那条分支的样本） |
| `notes.rtf` | LibreOffice（从 `notes.docx`） | 字体表、颜色表、样式表、`\*\userprops`、域代码与 `\'hh` 回退字节 |

## 三件只有踩过才会记下来的事

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

## 这些数字从哪来

Rust 测试里每个期望值都来自第二读者对这些文件的独立读取：
`scripts/acceptance/office_reader.py`（OOXML / ODF / MS-CFB / OLE 属性集，只用标准库）、
`lyco_rtf.py`（RTF）、`lyco_legacy.py`（`.doc` piece 表、`.xls` BIFF8、`.ppt` 记录树）。
CI 的 `apps` job 会把编出来的 `lbin` 再跑一遍 `office_probe.py` 与它们逐字段对账，
不一致就红 —— 而不是只跑一遍单元测试说"自己跟自己也挺一致"。
