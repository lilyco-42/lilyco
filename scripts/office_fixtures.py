#!/usr/bin/env python3
"""生成 lbin 办公文件命令的**生产者 fixture**（doc / docx / xls / xlsx / ppt / pptx / odt / ods / odp / rtf）。

为什么需要这个脚本：`lbin office-*` 的断言不许来自我自己搓的字节 —— 那样只有「我写的
东西能被我自己读回来」这一种证据。这里全部走**独立生产者**：python-docx 写 docx，
LibreOffice（soffice headless）写 doc / xls / ppt / odt / rtf 与真正的 OOXML 表格与演示文稿。
测试断言里的每个数字都由 `office_probe.py` 从同一批文件独立读出来核对（两个读者一致才算数）。

重跑：`python scripts/office_fixtures.py [--force]`
产物落点：`lilyco-binfmt/tests/fixtures/office/`（会一起提交 —— 每个文件都是几十 KB 量级，
远低于 hygiene 的 2 MB 闸门）。
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
OUT = REPO / "lilyco-binfmt" / "tests" / "fixtures" / "office"
SCRATCH = OUT / ".producer"

# 这些串是全部断言的锚点：改了它们就得同时改测试
MARK_TITLE = "季度预算说明"
# 演示稿那两张图的锚点（改了它们就得同时改 office-slide 的测试）
PPT_CHART_HEAD = "逐月收支"
PPT_CHART_CATS = ("一月", "二月")
PPT_CHART_SER_1 = "收入"
PPT_CHART_SER_2 = "支出"
PPT_CHART_PIE_CATS = ("服务器", "网络")
PPT_CHART_PIE_SER = "占比"
PPT_CHART_PLAIN = "这一页一张图也没有"
MARK_BODY = "第三季度服务器预算为十二万四千元"
MARK_HEADING = "一级标题：预算口径"
MARK_SHEET = "预算表"
MARK_CELL_A1 = "科目"
MARK_CELL_B2 = "服务器"
MARK_TOTAL_LABEL = "合计"
MARK_SLIDE_TITLE = "预算评审"
# 页上那张表那两件的页标题（改了它就得同时改 office-slide 的表断言）
MARK_TABLE_PAGE = "表格那一页"
MARK_LINK_PAGE = "链接那一页"
MARK_SLIDE_BODY = "新增两台 64 核应用服务器"
MARK_NOTES = "评审时先讲口径再讲数字"
MARK_COMMENT = "这里要补上不含税口径"
MARK_AUTHOR = "liuqi"
MARK_COMPANY = "lilyco"
MARK_KEYWORD = "budget,quarterly"
# 表格批注那两句（MARK_COMMENT 已经被文档批注用了，这两句只住在这两份表格里）
MARK_CELL_NOTE = "第二张单已确认"
MARK_CELL_NOTE_2 = "同一个作者再来一条"
# 批注的第四种存法（.xls 那条记录流）要一次只改一个变量的样本：作者名有 ASCII 的也有
# 中文的、字数有 2 也有 3，注的字有单行也有带换行的，格子拉到 AA100 让列名进两位数，
# 而且分到两张表上 —— 这四件事各自决定那三条记录里的某一位（编码旗标、字数、按表归位）
MARK_NOTE_SHEET_2 = "第二张"
MARK_NOTE_ASCII_AUTHOR = "AB"
MARK_NOTE_ASCII_TEXT = "one"
MARK_NOTE_LONG_AUTHOR = "欧阳锋"
MARK_NOTE_TWO_LINE = "第一行\n第二行"
# 尾注那句：notes-foot.docx 只有脚注，notes-end.docx 在这句上才走得到 `endnote` 那一支
MARK_ENDNOTE = "Endnote: the totals exclude the carry-over."
# 那张纸的第二尺寸（A4）与横过来的那一节：两份件的标题句，见 write_paper_a4_docx
MARK_PAPER_A4 = "A4 纵向的这一节"
MARK_PAPER_LAND = "横过来的那一节"
# 合并格那两张表的锚点句，见 write_merged_tables_docx
MARK_MERGED_HEAD = "合并格的样本"
MARK_MERGED_WIDE = "跨两列"
MARK_MERGED_TALL = "跨两行"
# 列表与编号那七段：一次把「编号从哪来」的三条路都摆开，见 write_list_docx
MARK_LIST_PLAIN = "这一段不在列表里"
MARK_LIST_NUM_1 = "编号列表第一项"
MARK_LIST_NUM_2 = "编号列表第二项"
MARK_LIST_BULLET = "圆点列表第一项"
MARK_LIST_DEEP_1 = "直接挂在段上的第一级"
MARK_LIST_DEEP_2 = "直接挂在段上的第二级"
MARK_LIST_DANGLING = "点了一个不存在的编号"
# 格子底色/边框/垂直对齐那三格的锚点句，见 write_shaded_docx
MARK_SHADE_FILL = "黄底"
MARK_SHADE_BORDER = "上边一条双线"
MARK_SHADE_ALIGN = "底对齐"
MARK_SHADE_PLAIN = "什么都不设"
# 域与站内跳转那两份锚点句，见 write_fields_docx
MARK_FIELD_LINK = "跳到那张表"
MARK_FIELD_DEAD = "跳一个坏了的名"
MARK_FIELD_MISSING = "没这个书签"
MARK_FIELD_ANCHOR = "被内部链接指着的那一段"
# 字符格式那三份件的锚点句，见 write_runs_docx：那三个字是每一个格式开关各自点的
# 同一串字，所以「哪一串是点了的」只能按段与按位置对上，不能靠字本身分
MARK_RUN_BASE = "基准段：什么都不点。"
MARK_RUN_TAIL = "甲乙丙"
MARK_RUN_TAIL_END = "尾部一段：一个字都不点。"


def need_soffice() -> str:
    for name in ("soffice", "libreoffice"):
        found = shutil.which(name)
        if found:
            return found
    for guess in (
        r"C:\Program Files\LibreOffice\program\soffice.exe",
        r"C:\Program Files (x86)\LibreOffice\program\soffice.exe",
        "/Applications/LibreOffice.app/Contents/MacOS/soffice",
    ):
        if Path(guess).exists():
            return guess
    sys.exit("需要 LibreOffice（soffice）才能生产遗留格式 fixture —— 装一个再跑")


def convert(exe: str, src: Path, fmt: str, dest: Path) -> Path:
    """soffice --convert-to：目标格式扩展名自己拼，因为不同平台输出目录行为不一致"""
    dest.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [exe, "--headless", "--norestore", "--convert-to", fmt, "--outdir", str(dest), str(src)],
        check=True,
        capture_output=True,
        timeout=240,
    )
    return dest


def add_hyperlink(paragraph, text: str, url: str):
    """python-docx 没有 add_hyperlink，走关系表那条正规路子（Word 自己也是这么写的）"""
    from docx.opc.constants import RELATIONSHIP_TYPE as RT
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    r_id = paragraph.part.relate_to(url, RT.HYPERLINK, is_external=True)
    link = OxmlElement("w:hyperlink")
    link.set(qn("r:id"), r_id)
    run = OxmlElement("w:r")
    props = OxmlElement("w:rPr")
    style = OxmlElement("w:rStyle")
    style.set(qn("w:val"), "Hyperlink")
    props.append(style)
    run.append(props)
    node = OxmlElement("w:t")
    node.text = text
    run.append(node)
    link.append(run)
    paragraph._p.append(link)


def add_field(paragraph, instr: str, cached: str, dirty: bool = False) -> None:
    """python-docx 没有「域」这一层：按 Word 自己的写法补 begin / instrText / separate / 结果 / end

    `w:instrText` 里那句是「要算什么」，夹在 separate 与 end 之间的那些 run 才是页面上看得见
    的字（上一次算出来的缓存值）—— 两件事，所以两份都要有。`w:dirty` 是「这域脏了，下次要重算」，
    只有 Word 会写，别家根本不认识这个开关。
    """
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    def fld(kind: str, is_dirty: bool = False):
        # `w:fldChar` 必须住在**串**里：第一版把它直接挂在段上（与 instrText 那个 run 并列），
        # Word 的账本照样数得出，而 LibreOffice 读进去时那三条壳全被丢，
        # 剩下「指令那一串」与「缓存那一串」当成两句普通的字写回 odt/rtf ——
        # 页面上凭空多出「 SEQ 表 \* ARABIC1」这样一句死字（实测，见 README 事实 77）
        run = OxmlElement("w:r")
        node = OxmlElement("w:fldChar")
        node.set(qn("w:fldCharType"), kind)
        if is_dirty:
            node.set(qn("w:dirty"), "true")
        run.append(node)
        return run

    def word(name: str, value: str):
        run = OxmlElement("w:r")
        node = OxmlElement(name)
        node.set(qn("xml:space"), "preserve")
        node.text = value
        run.append(node)
        return run

    paragraph._p.append(fld("begin", dirty))
    paragraph._p.append(word("w:instrText", instr))
    paragraph._p.append(fld("separate"))
    paragraph._p.append(word("w:t", cached))
    paragraph._p.append(fld("end"))


def add_internal_link(paragraph, text: str, anchor: str) -> None:
    """站内跳转：不占关系表，地址写在 `w:anchor` 上（与外部链接那条 `r:id` 各写一处）"""
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    link = OxmlElement("w:hyperlink")
    link.set(qn("w:anchor"), anchor)
    run = OxmlElement("w:r")
    node = OxmlElement("w:t")
    node.text = text
    run.append(node)
    link.append(run)
    paragraph._p.append(link)


def add_bookmark(paragraph, name: str, ident: str) -> None:
    """书签是**两条**元素夹住那一段（`w:id` 配对，名字只在 start 上）"""
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    start = OxmlElement("w:bookmarkStart")
    start.set(qn("w:id"), ident)
    start.set(qn("w:name"), name)
    end = OxmlElement("w:bookmarkEnd")
    end.set(qn("w:id"), ident)
    paragraph._p.insert(0, start)
    paragraph._p.append(end)


MARK_FONT_DECLARED = "点一个字体表里有的名"
MARK_FONT_UNDECLARED = "点一个字体表里没有的名"
MARK_FONT_THEME = "点主题里的那一个"
MARK_FONT_EAST = "只点东亚那一路"
MARK_FONT_PLAIN = "什么都不点：这一段一个字都不点字体"


def write_fonts_docx(path: Path) -> None:
    """字体那份账：一张字体表 + 五种点法，一次只改一个变量

    五段各问一句：点一个表里**有**的名（Courier）、点一个表里**没有**的名（Courier New ——
    这才是「这份文档要用的字体没随文件走」的真形状）、点主题里的那一个（`w:asciiTheme`
    而没有 `w:ascii`，要再跳一跳才落到字面名）、只点东亚那一路（`w:eastAsia` 而 ascii/hAnsi
    一个字都不写 —— 同一句「用什么字」按书写系统分成四处各写各的），最后一段自己
    一个字都不点（它用样式给的东西）。

    python-docx 的 `font.name` 一次写 `w:ascii` 与 `w:hAnsi` 两遍（同一个名两个属性），
    东亚那一路与主题那一路它没有开关，所以那两段是手写的 `w:rFonts`。
    """
    from docx import Document
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    doc = Document()
    doc.add_heading("字体表与点它的地方", level=1)

    one = doc.add_paragraph()
    one.add_run(MARK_FONT_DECLARED).font.name = "Courier"

    two = doc.add_paragraph()
    two.add_run(MARK_FONT_UNDECLARED).font.name = "Courier New"

    three = doc.add_paragraph()
    run = three.add_run(MARK_FONT_THEME)
    fonts = OxmlElement("w:rFonts")
    fonts.set(qn("w:asciiTheme"), "minorHAnsi")
    fonts.set(qn("w:hAnsiTheme"), "minorHAnsi")
    run._element.get_or_add_rPr().append(fonts)

    four = doc.add_paragraph()
    run = four.add_run(MARK_FONT_EAST)
    fonts = OxmlElement("w:rFonts")
    fonts.set(qn("w:eastAsia"), "ＭＳ 明朝")
    run._element.get_or_add_rPr().append(fonts)

    doc.add_paragraph(MARK_FONT_PLAIN)
    doc.save(path)


def write_sections_docx(path: Path) -> None:
    """两节 + 首尾页开关 + 奇偶页开关，再加一条指着不存在关系的引用

    「这一节的页脚是什么」在这里有两个答案：它自己写的，与它沿用上一节的那一个
    （Word 界面上叫「与上一节相同」）。`w:titlePg` 与 `w:evenAndOddHeaders` 是 python-docx
    自己的开关（`different_first_page_header_footer` / `odd_and_even_pages_header_footer`），
    不是我们手写的；最后那一条 `r:id="rId999"` 是故意留的坏引用 —— 关系表里根本没有这一个号。

    量到的一条生产者脾气：**python-docx 新加的节默认与上一节共用同一份页眉部件**
    （它不给第二节写 `headerReference`，`section.header` 直接指回第一节那份）。
    所以下面那句「第二节页眉」并没能变成第二节自己的页眉 —— 它把第一节那份
    `word/header1.xml` 里的字**改掉了**，两份件里都只有一份页眉部件。
    """
    from docx import Document
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    doc = Document()
    first = doc.sections[0]
    first.different_first_page_header_footer = True
    first.header.paragraphs[0].text = "第一节页眉"
    first.footer.paragraphs[0].text = "第一节页脚"
    doc.add_paragraph("第一节正文")
    rest = doc.add_section()
    # 这一句写进的是第一节那份 header1.xml（默认 linked_to_previous），不是新的一份部件
    rest.header.paragraphs[0].text = "第二节页眉"
    doc.add_paragraph("第二节正文：这一节的页眉与页脚自己都没写，沿用上面那一节的")

    # 坏引用：在第二节的 sectPr 上补一条 even 页眉，号指到关系表里不存在的那一个
    sect = rest._sectPr
    bad = OxmlElement("w:headerReference")
    bad.set(qn("w:type"), "even")
    bad.set(qn("r:id"), "rId999")
    sect.append(bad)
    doc.settings.odd_and_even_pages_header_footer = True
    doc.save(str(path))


def write_fields_docx(path: Path) -> None:
    """一份件里放四种「文件自己算出来的东西」：SEQ 编号、DATE、页脚里的页码，与一个站内跳转"""
    from docx import Document

    doc = Document()
    doc.add_heading("域与跳转", level=1)

    one = doc.add_paragraph()
    one.add_run("题注：")
    add_field(one, ' SEQ 表 \\* ARABIC', "1")

    two = doc.add_paragraph()
    add_internal_link(two, MARK_FIELD_LINK, "表锚点")
    add_internal_link(two, MARK_FIELD_DEAD, MARK_FIELD_MISSING)
    two.add_run("（站内跳转，不占关系表；第二条点的是一个不存在的名）")

    three = doc.add_paragraph(MARK_FIELD_ANCHOR)
    add_bookmark(three, "表锚点", "3")

    four = doc.add_paragraph()
    four.add_run("自动日期：")
    add_field(four, ' DATE \\@ "yyyy-MM-dd"', "2026-09-24", dirty=True)

    five = doc.add_paragraph()
    add_field(five, " PAGE ", "2")
    five.add_run("（页码写在正文里一次，页脚里一次）")

    foot = doc.sections[0].footer.paragraphs[0]
    foot.add_run("第 ")
    add_field(foot, " PAGE ", "2")
    foot.add_run(" 页")
    doc.save(str(path))


def tiny_png(path: Path) -> Path:
    """Pillow 造一张 8×8 的 PNG：给 docx / pptx 当真实媒体部件（media + 关系）"""
    from PIL import Image

    img = Image.new("RGBA", (8, 8), (37, 99, 235, 255))
    for x in range(8):
        img.putpixel((x, x), (255, 255, 255, 255))
    img.save(str(path), "PNG")
    return path


def write_docx(path: Path, art: Path) -> None:
    """python-docx：标题 / 正文 / 表格 / 超链接 / 图片 / 批注 / 自定义属性都在里面

    这些特性不是「凑数」：`lbin office-doc` 要报段落与表格与超链接，`office-objects` 要报
    图片与外部链接，`office-meta` 要报自定义属性 —— 每一项都得有真生产者写出来的样本，
    否则测试只能验我自己搓的字节。
    """
    from docx import Document
    from docx.enum.text import WD_ALIGN_PARAGRAPH

    doc = Document()
    doc.add_heading(MARK_HEADING, level=1)
    body = doc.add_paragraph(MARK_BODY)
    doc.add_heading("二级标题：明细", level=2)
    table = doc.add_table(rows=2, cols=2)
    table.cell(0, 0).text = MARK_CELL_A1
    table.cell(0, 1).text = "金额"
    table.cell(1, 0).text = MARK_CELL_B2
    table.cell(1, 1).text = "124000"
    tail = doc.add_paragraph("口径见 ")
    add_hyperlink(tail, "预算制度", "https://example.com/budget")
    pic = doc.add_paragraph()
    pic.add_run().add_picture(str(art), width=None)
    doc.add_page_break()
    last = doc.add_paragraph("最后一页说明：数字为含税口径")
    doc.add_comment(last.runs[0], text=MARK_COMMENT, author=MARK_AUTHOR, initials="L")
    first = doc.paragraphs[0]
    first.alignment = WD_ALIGN_PARAGRAPH.LEFT
    props = doc.core_properties
    props.title = MARK_TITLE
    props.author = MARK_AUTHOR
    props.company = MARK_COMPANY
    props.keywords = MARK_KEYWORD
    props.comments = "fixture produced by python-docx"
    props.category = "预算"
    props.subject = "季度预算"
    doc.save(str(path))
    add_custom_props(path)


def add_custom_props(path: Path) -> None:
    """补一份 `docProps/custom.xml`：python-docx 1.2 没暴露这块，而 Word 天天在写它。

    做法是纯 OPC 层的三件事：加部件、`_rels/.rels` 里加一条包级关系、
    `[Content_Types].xml` 里加一个 Override。少任何一件，Word 都会说包坏了 ——
    所以 `office-package` 的「关系指向不存在的部件 / 部件没声明类型」两条检查正好有样本可验。
    """
    custom = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n'
        '<Properties '
        'xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" '
        'xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">'
        '<property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name="口径">'
        "<vt:lpwstr>含税</vt:lpwstr></property>"
        '<property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="3" name="预算额度">'
        "<vt:i4>124000</vt:i4></property>"
        "</Properties>"
    )
    with zipfile.ZipFile(path) as box:
        rels = box.read("_rels/.rels").decode("utf-8")
        types = box.read("[Content_Types].xml").decode("utf-8")
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    used = [int(one) for one in re.findall(r'Id="rId(\d+)"', rels)]
    rid = f"rId{(max(used) if used else 0) + 1}"
    rels = rels.replace(
        "</Relationships>",
        f'<Relationship Id="{rid}" '
        'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties" '
        'Target="docProps/custom.xml"/></Relationships>',
    )
    types = types.replace(
        "</Types>",
        '<Override PartName="/docProps/custom.xml" ContentType='
        '"application/vnd.openxmlformats-officedocument.custom-properties+xml"/></Types>',
    )
    parts["_rels/.rels"] = rels.encode("utf-8")
    parts["[Content_Types].xml"] = types.encode("utf-8")
    parts["docProps/custom.xml"] = custom.encode("utf-8")
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as box:
        box.writestr("[Content_Types].xml", parts["[Content_Types].xml"])
        for name, data in parts.items():
            if name != "[Content_Types].xml":
                box.writestr(name, data)


def write_english_docx(path: Path) -> None:
    """纯 ASCII 的一份 docx：转成 .doc 后 Word 会用「压缩 piece」存它

    这个样本存在的唯一理由：.doc 的 piece 表每个 piece 自己说字符宽
    （fc 的 bit30 = 压缩 → 8 位字符，而且偏移还要除二）。中英混排那份走的是
    UTF-16 那条路，光有它就无法证明 8 位这条路也读对了。
    """
    from docx import Document

    doc = Document()
    doc.add_heading("Quarterly budget note", level=1)
    doc.add_paragraph("The server budget for Q3 is 124,000 yuan.")
    doc.add_paragraph("Second line: numbers are tax-inclusive.")
    doc.core_properties.title = "Quarterly budget note"
    doc.core_properties.author = "liuqi"
    doc.save(str(path))


def write_header_docx(path: Path) -> None:
    """带页眉与页脚的 docx：两节的页眉不一样，第二节显式断开链接才会多出 header2.xml

    这份样本存在的理由：`office-text` 早就按部件名扫 `header*.xml` / `footer*.xml` 了，
    可前面那批 fixture 一份都不带页眉 —— 那条分支从来没被真件走过一次。
    """
    from docx import Document

    doc = Document()
    doc.add_heading("带页眉的一页", level=1)
    doc.add_paragraph("正文只有一句：这一份是用来测页眉页脚那条分支的。")
    first = doc.sections[0]
    first.header.paragraphs[0].text = "公司机密 · 预算评审"
    first.footer.paragraphs[0].text = "第 1 页 / 共 3 页"
    second = doc.add_section()
    # 不显式断开，python-docx 会让第二节沿用第一节，就不会有第二个页眉部件
    second.header.is_linked_to_previous = False
    second.header.paragraphs[0].text = "第二节的页眉不一样"
    doc.add_page_break()
    doc.add_paragraph("换节之后的一段正文。")
    doc.core_properties.title = "带页眉的说明"
    doc.core_properties.author = "liuqi"
    doc.save(str(path))


def write_tables_docx(path: Path) -> None:
    """两张表的 docx：3×2 与 2×2，中间夹一段正文，首尾各一个标题

    这份样本存在的理由：RTF 那条流里「几张表」判不住。`\row` 与 `\cell` 的条数在两份件上
    都与 docx 那副账一字不差（2 行 4 格、5 行 10 格），可「连续的 \trowd 算一张表」这条
    规则在单表上对、在两表上把两张数成一张 —— 所以要能同时摆一张与两张，
    才知道哪一半敢报、哪一半只能留 null。
    """
    from docx import Document

    doc = Document()
    doc.add_heading("两张表的样本", level=1)
    doc.add_paragraph("中间一段普通话")
    for rows, cols in ((3, 2), (2, 2)):
        table = doc.add_table(rows=rows, cols=cols)
        for row in range(rows):
            for column in range(cols):
                table.cell(row, column).text = "R%dC%d" % (row, column)
        doc.add_paragraph("两张表之间的一段")
    doc.add_heading("二级标题", level=2)
    doc.core_properties.title = "两张表的样本"
    doc.save(str(path))


def write_paper_a4_docx(path: Path) -> None:
    """A4 纵向 + 横过来的一节：两节各写自己的纸与边距（Letter 那批件两节是一样的）

    这份样本存在的理由：`page_setup` 那本账目前只在 Letter（12240×15840 twips）上量过，
    而 Letter 恰好是 python-docx 模板自带的默认值 —— 换一个尺寸才知道换算不是凑上的。
    A4 还能把「生产者自己不一致」量出来：Word/OOXML 与 RTF 用 twips，A4 写作 11906×16838，
    换成 0.1mm 是 21001×29701；LibreOffice 的 ODF 导出写 21cm×29.7cm，即 21000×29700。
    两家差的那一个单位（0.1mm）是它们各自舍入的结果，不是我们算错 —— 所以三份件各报各的，
    不挑一个当准。第二节显式 `orientation=landscape` 并把宽高对调，
    这样「orient 只交文件写了的」这一条也第一次有真件可走（Letter 那批三家都不写方向）。
    """
    from docx import Document
    from docx.enum.section import WD_ORIENT
    from docx.shared import Cm, Mm

    doc = Document()
    first = doc.sections[0]
    first.page_width = Mm(210)
    first.page_height = Mm(297)
    first.top_margin = Cm(2)
    first.bottom_margin = Cm(2)
    first.left_margin = Cm(3)
    first.right_margin = Cm(3)
    doc.add_heading(MARK_PAPER_A4, level=1)
    doc.add_paragraph("这一节的纸是 210×297，边距 2 厘米与 3 厘米。")
    second = doc.add_section()
    second.orientation = WD_ORIENT.LANDSCAPE
    second.page_width = Mm(297)
    second.page_height = Mm(210)
    second.top_margin = Cm(1.5)
    second.bottom_margin = Cm(1.5)
    second.left_margin = Cm(2)
    second.right_margin = Cm(2)
    doc.add_heading(MARK_PAPER_LAND, level=1)
    doc.add_paragraph("同一份文件里两节，纸的宽高对调，边距也不同。")
    doc.core_properties.title = MARK_PAPER_A4
    doc.save(str(path))


def write_merged_tables_docx(path: Path) -> None:
    """两张带合并格的表：一张横向合并（gridSpan），一张纵向合并（vMerge）

    这份样本存在的理由：`office-doc` 的 `tables` 只交行数与格子数，而现在这两张件
    （`tables.docx` / `.odt`）恰好都没有合并格，所以「格子数」在两家是一样的。
    一旦有合并，两家的**写法**就不一样了 —— 同一张视觉上 2×3 的表：
    OOXML 把横向合掉的那一格**不写**（第一个格子带 `w:gridSpan="2"`，所以那一行只有 2 个 `w:tc`），
    而 LibreOffice 的 ODF 把被盖住的那一格照样写出来（空格子 + 前一个格子
    `number-columns-spanned="2"`，所以那一行是 3 个 `table-cell`）。
    纵向合并两家都是「继续的那一格照样写、字是空的」（`vMerge` 无值 = continue /
    ODF 什么都不写）。也就是说格子数与「这一行有几个格子」是**存储的数**，
    不是页面上那张表的数 —— 这份件就是这条口径的唯一出处。
    """
    from docx import Document

    doc = Document()
    doc.add_paragraph(MARK_MERGED_HEAD)
    wide = doc.add_table(rows=2, cols=3)
    wide.cell(0, 0).merge(wide.cell(0, 1))
    wide.cell(0, 0).text = MARK_MERGED_WIDE
    wide.cell(0, 2).text = "第三列"
    for column, text in enumerate(("a", "b", "c")):
        wide.cell(1, column).text = text
    doc.add_paragraph("两张表之间")
    tall = doc.add_table(rows=2, cols=2)
    tall.cell(0, 0).merge(tall.cell(1, 0))
    tall.cell(0, 0).text = MARK_MERGED_TALL
    tall.cell(0, 1).text = "右上"
    tall.cell(1, 1).text = "右下"
    doc.core_properties.title = MARK_MERGED_HEAD
    doc.save(str(path))




def write_table_style_docx(path: Path) -> None:
    """四张表，一次只改一个变量：不点样式 / 点内置样式 / 改 `w:tblLook` 的一位 / 把样式那一格删掉

    python-docx 的 `table.style` 写的是**样式 id**（`Light Grid Accent 1` 落成
    `LightGrid-Accent1`），而 `w:tblLook` 里除了六个位还有一个十六进制缓存值：改了位**它不重算**
    （实测 `w:firstRow` 已经 0 而 `w:val` 还是 `04A0`），LibreOffice 重写同一份会重算成 `0480`
    并把它写成小写 —— 两本账都按写的交，不互相修。
    """
    from docx import Document

    w_main = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
    doc = Document()
    doc.add_paragraph("表样式那份账")
    plan = (
        ("默认表", None, None),
        ("内置样式", "Light Grid Accent 1", None),
        ("改了 tblLook", "Light Grid Accent 1", {"firstRow": "0"}),
        ("没了 tblStyle", "Light Grid Accent 1", None),
    )
    tables = []
    for title, style, look in plan:
        doc.add_heading(title, level=2)
        table = doc.add_table(rows=2, cols=2)
        for row in range(2):
            for col in range(2):
                table.cell(row, col).text = "%s r%d c%d" % (title[:2], row, col)
        if style:
            table.style = style
        props = table._tbl.tblPr
        if look:
            holder = props.find(w_main + "tblLook")
            if holder is None:
                holder = props.makeelement(w_main + "tblLook", {})
                props.append(holder)
            for key, value in look.items():
                holder.set(w_main + key, value)
        tables.append(table)
    fourth = tables[3]._tbl.tblPr
    for kid in list(fourth):
        if kid.tag == w_main + "tblStyle":
            fourth.remove(kid)
    doc.save(path)


def write_keep_docx(path: Path) -> None:
    """五条段，一次只改一个变量：基线 / keepNext / keepLines / pageBreakBefore / widowControl=False

    这四个开关 python-docx 都有**真的**属性（`paragraph_format.keep_with_next` 等）。要紧的是
    落进文件的样子不一样：前三个写成**空元素**（在场就是开着，不给值），第四个反过来写
    `w:val="0"` —— 所以「在场」与「开着」是两件事，读者两种都要交。LibreOffice 重写同一份会
    给每段补上 `w:pPr`、把值改写成 true/false，并把 `w:pageBreakBefore` 那一格整个丢掉。
    """
    from docx import Document

    doc = Document()
    doc.add_paragraph("基线段：四个开关都不写")
    one = doc.add_paragraph("段一：与下段同页（keepNext）")
    one.paragraph_format.keep_with_next = True
    two = doc.add_paragraph("段二：段中不分页（keepLines）")
    two.paragraph_format.keep_together = True
    three = doc.add_paragraph("段三：段前分页（pageBreakBefore）")
    three.paragraph_format.page_break_before = True
    four = doc.add_paragraph("段四：孤行控制关掉（widowControl=False）")
    four.paragraph_format.widow_control = False
    doc.save(path)


def write_line_docx(path: Path) -> None:
    """五条段：不写 / 1.5 倍 / 2 倍 / 固定 22 磅 / 至少 18 磅（一次只改一个变量）

    这份样本存在的理由是**同一个数在两种单位下长得一模一样**：1.5 倍写 `w:line="360"`，
    「至少 18 磅」也写 `w:line="360"` —— 只有紧跟的 `w:lineRule`（`auto` 对 `atLeast`）说得清
    那个数是 1/240 倍还是 twip。固定 22 磅则是 `w:line="440"` 配 `exact`。
    LibreOffice 转成 ODF 后倍数换成百分数（`150%`）、长度换成长度串（`0.776cm`），而
    `atLeast` 那一段**四个相关属性一个都不写** —— 不是写 0，是整格消失。
    """
    from docx import Document
    from docx.enum.text import WD_LINE_SPACING
    from docx.shared import Pt

    doc = Document()
    doc.add_paragraph("段零：行距什么都不写")
    one = doc.add_paragraph("段一：1.5 倍")
    one.paragraph_format.line_spacing = 1.5
    two = doc.add_paragraph("段二：2 倍")
    two.paragraph_format.line_spacing = 2.0
    three = doc.add_paragraph("段三：固定 22 磅")
    three.paragraph_format.line_spacing = Pt(22)
    three.paragraph_format.line_spacing_rule = WD_LINE_SPACING.EXACTLY
    four = doc.add_paragraph("段四：至少 18 磅")
    four.paragraph_format.line_spacing = Pt(18)
    four.paragraph_format.line_spacing_rule = WD_LINE_SPACING.AT_LEAST
    doc.save(path)


def write_border_docx(path: Path) -> None:
    """五条段：不写 / 四边单线 / 只有上面一条双线 / 只有底纹 / 空的边框壳 + 主题色底纹

    python-docx 没有段边框的公开属性，所以走 `OxmlElement` 那条正规路子。要点有三个：
    `w:pBdr` 是**装边的壳**（壳可以在而里面一条边都没写），一枚边的四个属性（`val` / `sz` /
    `space` / `color`）里 `sz` 是 1/8 磅而 `space` 是「边离字多远」的磅；底纹 `w:shd` 另有
    一枚 `w:themeFill`（点主题色而不写死颜色）。LibreOffice 重写同一份时把 `w:color="auto"`
    换成 `000000`、把那个**空壳整个丢掉**；转成 ODF 时四边合成一条 `fo:border` shorthand、
    单边则补三条明写的 `none`，而 `w:val="solid"` 那一段的底纹变成 `#ffffff`。
    """
    from docx import Document
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    doc = Document()
    doc.add_paragraph("段零：边框与底纹都不写")

    def edge(name, val, sz, space, color):
        one = OxmlElement("w:" + name)
        one.set(qn("w:val"), val)
        one.set(qn("w:sz"), sz)
        one.set(qn("w:space"), space)
        one.set(qn("w:color"), color)
        return one

    one = doc.add_paragraph("段一：四边单线（六分之一点、离字一磅、红色）")
    box = OxmlElement("w:pBdr")
    for name in ("top", "left", "bottom", "right"):
        box.append(edge(name, "single", "6", "1", "FF0000"))
    one._p.get_or_add_pPr().append(box)

    two = doc.add_paragraph("段二：只有上面一条粗双线")
    box = OxmlElement("w:pBdr")
    box.append(edge("top", "double", "18", "0", "auto"))
    two._p.get_or_add_pPr().append(box)

    three = doc.add_paragraph("段三：只有底纹（黄），一条边也没有")
    shd = OxmlElement("w:shd")
    shd.set(qn("w:val"), "clear")
    shd.set(qn("w:color"), "auto")
    shd.set(qn("w:fill"), "FFFF00")
    three._p.get_or_add_pPr().append(shd)

    four = doc.add_paragraph("段四：`pBdr` 元素在而一条边都没写（空的那一种）")
    four._p.get_or_add_pPr().append(OxmlElement("w:pBdr"))
    shd = OxmlElement("w:shd")
    shd.set(qn("w:val"), "solid")
    shd.set(qn("w:fill"), "00B050")
    shd.set(qn("w:themeFill"), "accent6")
    four._p.get_or_add_pPr().append(shd)
    doc.save(str(path))


def write_bookmark_docx(path: Path) -> None:
    """八段，一次只改一个变量：完整对 / 跨段对 / 只有起 / 只有止 / Word 的光标 / 重名 / 站内跳

    `w:bookmarkStart` 带 `id` 与 `name`，而 `w:bookmarkEnd` **只带 id** —— 闭没闭只能按号配。
    LibreOffice 重写同一份时把两个断的整个删掉（5 起 5 止 → 4 起 4 止）、号整批重排、
    第二条重名的 `口径` 改名 `口径_副本_1`（ODF 那一面写成带空格的「口径 副本 1」），
    而站内跳转的 `w:anchor="跨段"` 一字未改。转成 ODF 时同段起止的一对变成**一枚** `text:bookmark`。
    """
    from docx import Document
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    doc = Document()

    def start(para, pid, name):
        one = OxmlElement("w:bookmarkStart")
        one.set(qn("w:id"), str(pid))
        one.set(qn("w:name"), name)
        para._p.append(one)

    def end(para, pid):
        one = OxmlElement("w:bookmarkEnd")
        one.set(qn("w:id"), str(pid))
        para._p.append(one)

    one = doc.add_paragraph("第一段：一对完整的书签")
    start(one, 1, "口径")
    end(one, 1)
    two = doc.add_paragraph("第二段：书签从这里开始")
    start(two, 2, "跨段")
    three = doc.add_paragraph("第三段：在后面的段落里才结束")
    end(three, 2)
    four = doc.add_paragraph("第四段：只有开始，没有结束")
    start(four, 3, "断了")
    five = doc.add_paragraph("第五段：只有结束，没有开始")
    end(five, 9)
    six = doc.add_paragraph("第六段：Word 自己塞的那个")
    start(six, 4, "_GoBack")
    end(six, 4)
    seven = doc.add_paragraph("第七段：与第一段同名的第二条")
    start(seven, 5, "口径")
    end(seven, 5)
    eight = doc.add_paragraph("第八段：站内跳过去")
    link = OxmlElement("w:hyperlink")
    link.set(qn("w:anchor"), "跨段")
    run = OxmlElement("w:r")
    text = OxmlElement("w:t")
    text.text = "跳"
    run.append(text)
    link.append(run)
    eight._p.append(link)
    doc.save(str(path))


def write_tbox_odt(path: Path) -> None:
    """一份「页上有一个文本框」的 .odt：`draw:frame` 里套 `draw:text-box`，框里两段字

    这一族本机没有会写 OOXML 文本框的生产者（python-docx 不会加框），所以反过来走：这份 odt 用
    zipfile 写（形状照 OpenDocument 的写法：mimetype 第一成员、manifest 三条），再让 LibreOffice
    转成 docx —— 那份 OOXML 是 LibreOffice 自己写的。要点三条：同一个框它写**两份**
    （`w:drawing` 里一份、`w:pict` 里一份，两份 `w:txbxContent` 字一模一样）；尺寸只在 DrawingML
    那一份的 `wp:extent` 上（EMU），VML 那一份的 `v:shape` 连 `style` 都没写；而它
    重写 odt 时会挂 `draw:style-name="Frame"`、把 `svg:x` / `svg:y` / `draw:z-index` 整个丢掉，
    并把 5cm 换成 `5.001cm`。
    """
    content = """<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
 xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0"
 xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
 xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"
 xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"
 xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"
 office:version="1.2">
 <office:automatic-styles>
  <style:style style:name="P1" style:family="paragraph">
   <style:paragraph-properties fo:margin-top="0cm" fo:margin-bottom="0cm"/>
  </style:style>
 </office:automatic-styles>
 <office:body><office:text>
  <text:p text:style-name="P1">正文第一段</text:p>
  <text:p text:style-name="P1"><draw:frame draw:name="框一" text:anchor-type="as-char"
    svg:x="1.2cm" svg:y="0.5cm" svg:width="5cm" svg:height="2.4cm" draw:z-index="0">
    <draw:text-box><text:p text:style-name="P1">框里的第一段</text:p><text:p text:style-name="P1">框里的第二段，长一点的字</text:p></draw:text-box>
   </draw:frame>这一段在框外面</text:p>
  <text:p text:style-name="P1">正文最后一段</text:p>
 </office:text></office:body>
</office:document-content>"""
    styles = """<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
 office:version="1.2"><office:body><office:text/></office:body>
</office:document-styles>"""
    manifest = """<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"
 manifest:version="1.2">
 <manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/>
 <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
 <manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>
</manifest:manifest>"""
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as box:
        box.writestr(zipfile.ZipInfo("mimetype"), "application/vnd.oasis.opendocument.text")
        box.writestr("META-INF/manifest.xml", manifest)
        box.writestr("content.xml", content)
        box.writestr("styles.xml", styles)


def write_comments_docx(path: Path) -> None:
    """两条批注的一份 docx：作者名一个纯 ASCII、一个纯中文

    这份样本存在的理由：RTF 那一族把批注写在流里（`{\\*\\atnauthor …}` + `{\\*\\annotation …}`），
    而两条列表怎么配、日期是什么历法，一份只有一条注的件判不出来。第二条故意用中文作者名 ——
    LibreOffice 的 RTF 导出**写不出那个名字**（那一群里剩下两个问号），而它自己的 docx
    导出把「刘奇」照抄：同一批字在两家是两个答案，这一族照文件写的交，不替它认回来。
    """
    from docx import Document

    doc = Document()
    first = doc.add_paragraph("第一段：不含税口径")
    second = doc.add_paragraph("第二段：金额待确认")
    doc.add_paragraph("第三段：这一段没有批注")
    doc.add_comment(first.runs, text="这里要补上不含税口径", author="liuqi", initials="L")
    doc.add_comment(second.runs, text="这个数要找财务确认一下，第二行接着写", author="刘奇", initials="刘")
    doc.save(str(path))


def write_revisions_docx(path: Path) -> None:
    """带修订的一份 docx：插入、删除、改格式、整段新加（含段落标记）各一处

    这份样本存在的理由：`office-doc` 从前只报 `w:ins` / `w:del` 的**个数**，答不了
    「谁在什么时候改了哪一段」。四种修订要分开摆，因为它们在文件里的存法各不相同：
    删除的字存在 `w:del` 里（`w:delText`），改格式存在 `w:rPrChange` 里（不带字），
    整段新加还额外在 `w:pPr/w:rPr/w:ins` 标一次段落标记。
    """
    from docx import Document
    from docx.oxml import parse_xml
    from docx.oxml.ns import nsdecls

    W = nsdecls("w")
    doc = Document()
    doc.add_heading("预算说明（带修订）", level=1)
    doc.add_paragraph("第一段没有改动。")

    p2 = doc.add_paragraph()
    p2.add_run("预算总额为")
    p2._p.append(
        parse_xml(
            f'<w:ins {W} w:id="11" w:author="张三" w:date="2026-03-05T09:12:00Z">'
            f'<w:r><w:t>124000 元</w:t></w:r></w:ins>'
        )
    )
    p2._p.append(
        parse_xml(
            f'<w:del {W} w:id="12" w:author="李四" w:date="2026-03-06T11:45:00Z">'
            f'<w:r><w:delText>89000 元</w:delText></w:r></w:del>'
        )
    )
    tail = p2.add_run("，请复核。")
    tail.bold = True
    # 改格式这一条不带字：它说的是「这段字变成粗体」，字本身没动
    tail._r.get_or_add_rPr().append(
        parse_xml(
            f'<w:rPrChange {W} w:id="16" w:author="王五" w:date="2026-03-07T08:00:00Z">'
            f'<w:rPr><w:b w:val="0"/></w:rPr></w:rPrChange>'
        )
    )

    # 整段是新加的：正文那条 w:ins 之外，段落标记自己也标一条
    body = doc.element.body
    sect = body[-1]
    body.insert(
        list(body).index(sect),
        parse_xml(
            f'<w:p {W}>'
            f'<w:pPr><w:rPr><w:ins {W} w:id="14" w:author="张三" w:date="2026-03-05T09:20:00Z"/></w:rPr></w:pPr>'
            f'<w:ins {W} w:id="15" w:author="张三" w:date="2026-03-05T09:20:00Z">'
            f'<w:r><w:t>整段是新加的。</w:t></w:r></w:ins></w:p>'
        ),
    )
    doc.core_properties.title = "预算说明（带修订）"
    doc.core_properties.author = "liuqi"
    doc.save(str(path))


def patch_part(path: Path, edits: dict) -> None:
    """按部件改 zip 里的 XML：锚点出现次数不对就抛，不做静默的 no-op

    （`str.replace` 没匹配上是这里最容易犯的错 —— 那样提交出去的 fixture 根本没有那个元素。）
    """
    with zipfile.ZipFile(path) as box:
        items = {one.filename: box.read(one.filename) for one in box.infolist()}
    for part, (anchor, replacement) in edits.items():
        text = items[part].decode("utf-8")
        if text.count(anchor) != 1:
            raise SystemExit("%s 里锚点 %r 出现 %d 次，不是 1 次" % (part, anchor[:24], text.count(anchor)))
        items[part] = text.replace(anchor, replacement).encode("utf-8")
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as box:
        for name, blob in items.items():
            box.writestr(name, blob)


def write_protected_docx(src: Path, out: Path) -> None:
    """带编辑限制的 docx：保护写在 `word/settings.xml` 的 `w:documentProtection` 上

    存在的理由：`office-doc` 要回答「这份能动吗」。Word 的保护有三个地方 —— 编辑限制
    （`w:documentProtection`，`w:edit` 是限制类型、`w:enforcement` 才是开没开）、
    真正加密（另一回事，office-info 已经报），以及 `w:readModeInkLock` 那种无关的东西。
    """
    shutil.copyfile(src, out)
    patch_part(
        out,
        {
            "word/settings.xml": (
                "</w:settings>",
                '<w:documentProtection w:edit="readOnly" w:enforcement="1" '
                'w:cryptProviderType="rsaAES" w:cryptAlgorithmClass="hash" '
                'w:cryptAlgorithmType="typeAny" w:cryptAlgorithmSid="14" '
                'w:cryptSpinCount="100000" w:hash="AAAA" w:salt="BBBB"/></w:settings>',
            )
        },
    )


def write_locked_sheet_xlsx(src: Path, out: Path) -> None:
    """带表级保护与工作簿结构锁的 xlsx（在 openpyxl 写的那份 book.xlsx 上补两处）

    元素的位置是 schema 定死的：`sheetProtection` 在 `sheetData` 之后、`mergeCells` 之前，
    `workbookProtection` 在 `bookViews` 之前 —— 摆错了 LibreOffice 就读不出来。
    openpyxl 已经写了一个空的 `<workbookProtection/>`，所以这里是**改那一个**而不是再插一个：
    插第二个会造出同一段里两个同名元素（`maxOccurs=1`），而 LibreOffice 只读第一个，
    于是锁就"凭空丢了" —— 这个坑是第一次跑这脚本时踩到的。
    """
    shutil.copyfile(src, out)
    patch_part(
        out,
        {
            "xl/worksheets/sheet1.xml": (
                "<mergeCells",
                '<sheetProtection sheet="1" formatCells="0" insertRows="1" password="6E4E"/><mergeCells',
            ),
            "xl/workbook.xml": (
                "<workbookProtection/>",
                '<workbookProtection lockStructure="1" password="1234"/>',
            ),
        },
    )


def write_locked_second_xlsx(src: Path, out: Path) -> None:
    """与上一份唯一的差别：锁放在**第二张**表上（sheet2.xml = 「说明」）

    为什么要费这个事：`.xls` 那一族的 PROTECT(0x0012) / PASSWORD(0x0013) /
    SCENPROTECT(0x00DD) 到底是「整本工作簿一层」还是「每张表一层」，凭印象说不得。
    拿这两份一对，三条记录跟着锁挪窝（锁第二张时它们在第二个子流里），而
    LibreOffice 自己 import 回 .ods 也只在被锁那张上写 `table:protected` —— 于是
    「按表记」是量出来的。`sheetProtection` 的位置照 schema：`sheetData` 之后。
    """
    shutil.copyfile(src, out)
    patch_part(
        out,
        {
            "xl/worksheets/sheet2.xml": (
                "<pageMargins",
                '<sheetProtection sheet="1" formatCells="0" password="6E4E"/><pageMargins',
            )
        },
    )


def write_xlsx(path: Path) -> None:
    """openpyxl：多表、隐藏表、公式、合并格、命名区域、真表格 —— 一个电子表格里
    `lbin office-sheet` 要报的东西基本都在这里，而这些东西 LibreOffice 转出来的样本未必有。
    """
    from openpyxl import Workbook
    from openpyxl.styles import Font
    from openpyxl.workbook.defined_name import DefinedName
    from openpyxl.worksheet.table import Table, TableColumn

    wb = Workbook()
    ws = wb.active
    ws.title = MARK_SHEET
    ws["A1"] = MARK_CELL_A1
    ws["B1"] = "金额"
    ws["A2"] = MARK_CELL_B2
    ws["B2"] = 124000
    ws["A3"] = "网络"
    ws["B3"] = 18000
    ws["A4"] = MARK_TOTAL_LABEL
    ws["B4"] = "=SUM(B2:B3)"
    ws["A1"].font = Font(bold=True)
    ws.merge_cells("A5:B5")
    ws["A5"] = "口径：含税"
    second = wb.create_sheet("说明")
    second["A1"] = "第二张表：口径说明"
    hidden = wb.create_sheet("草稿")
    hidden["A1"] = "隐藏的草稿表"
    hidden.sheet_state = "hidden"
    wb.defined_names.add(
        DefinedName("总额", attr_text=f"'{MARK_SHEET}'!$B$4")
    )
    table = Table(displayName="预算表", ref="A1:B3")
    table.tableColumns.append(TableColumn(id=1, name=MARK_CELL_A1))
    table.tableColumns.append(TableColumn(id=2, name="金额"))
    ws.add_table(table)
    wb.properties.title = MARK_TITLE
    wb.properties.creator = MARK_AUTHOR
    wb.properties.description = "fixture produced by openpyxl"
    wb.save(str(path))


def write_mulrk_xlsx(path: Path) -> None:
    """一行连续的八个数字 —— LibreOffice 写 .xls 时把这种行程并成一条 MULRK

    为什么要专门造这一份：MULRK(0x00BD) 每格是 `{ixfe(2), rkmac(4)}`，值在**后**四字节。
    两边的读者都曾读早两个字节（把 ixfe 当成数），而 book.xls / formats.xls 里
    根本没有 MULRK —— 没被真件走到的代码，两份实现一起错也对账不出来。
    这份件转成 .xls 后确实只有一条 MULRK（54 字节正文 = rw + colFirst + 8×6 + colLast），
    值 1000.5…8000.5 与 LibreOffice 自己读回 .ods 的 A2:H2 逐格一致。
    """
    from openpyxl import Workbook

    wb = Workbook()
    ws = wb.active
    ws.title = "连续"
    ws["A1"] = "一行连续的数（LibreOffice 会写成一条 MULRK）"
    for column in range(1, 9):
        ws.cell(row=2, column=column, value=column * 1000 + 0.5)
    ws["A4"] = "隔开一行就不是一程"
    for column in range(1, 4):
        ws.cell(row=5, column=column, value=column * 7)
    wb.save(path)


def write_cell_notes_xlsx(path: Path) -> None:
    """带批注的表格：一格一条 `xl/comments*.xml` 里的 `<comment ref authorId>`。

    为什么要专门造这一份：批注不在 `sheet1.xml` 里，它在**另一个部件**里，
    要靠这张表自己的 `xl/worksheets/_rels/sheet1.xml.rels` 才能找到 ——
    少一跳就报成「这份表没有批注」。两个生产者把那个部件放在两个地方：
    openpyxl 写 `xl/comments/comment1.xml`（关系 Target 还是绝对路径 `/xl/...`，
    关系 Id 甚至不是 rId 而是字面量 `comments`），LibreOffice 写 `xl/comments1.xml`
    （Target 是 `../comments1.xml`）—— 两种都要走得到。
    同一批字再转一份 .ods：那里批注是 `office:annotation`，**坐在格子里面**，
    一锅端地取格子的字就会把注的文字当成这一格的内容（这一条与 .odt 里
    「批注与修订表那段不算正文」是同一条规矩）。
    """
    import datetime as dt

    from openpyxl import Workbook
    from openpyxl.comments import Comment

    wb = Workbook()
    ws = wb.active
    ws.title = MARK_SHEET
    ws["A1"], ws["B1"] = MARK_CELL_A1, "金额"
    ws["A2"], ws["B2"] = "服务器", 124000
    ws["A3"], ws["B3"] = "网络", 18000
    # 带时间戳的那一条：openpyxl 到底把时间写不写进 XML，是这份件要回答的问题之一
    ws["B2"].comment = Comment(MARK_COMMENT, "张三", dt.datetime(2026, 3, 5, 9, 8, 7))
    ws["A3"].comment = Comment(MARK_CELL_NOTE, "李四")
    ws["B3"].comment = Comment(MARK_CELL_NOTE_2, "李四")
    wb.save(path)


def write_cell_notes_many(path: Path) -> None:
    """给 .xls 那第四种存法当种子：两份表、四条注，每条各动一个变量。

    LibreOffice 的 .xls 导出器把一条注拆成三条记录（字、格子与作者、还有一条自报的
    序号），那三条里的每一位都要有件走过：作者名 ASCII 的与中文的（那一族里编码旗标
    与 BIFF8 的惯例相反）、字数 2 与 3、注的字带不带换行、格子拉到 AA100 让列名进
    两位数、以及注分落在两张表上（不按子流归位就会把第二张表的注挂到第一张上）。
    """
    from openpyxl import Workbook
    from openpyxl.comments import Comment

    wb = Workbook()
    first = wb.active
    first.title = MARK_SHEET
    first["A1"] = 1
    first["A1"].comment = Comment(MARK_NOTE_ASCII_TEXT, MARK_NOTE_ASCII_AUTHOR)
    first["C5"] = 2
    first["C5"].comment = Comment(MARK_NOTE_TWO_LINE, "张三")
    first["AA100"] = 3
    first["AA100"].comment = Comment(MARK_COMMENT, MARK_NOTE_LONG_AUTHOR)
    second = wb.create_sheet(MARK_NOTE_SHEET_2)
    second["B2"] = 4
    second["B2"].comment = Comment(MARK_CELL_NOTE, "李四")
    wb.save(path)


def write_formats_xlsx(path: Path) -> None:
    """openpyxl：一个格子的 `s=` 指向 styles.xml 的 cellXfs，那里才写着它是日期还是数。

    这份样本是专给「日期格识别」这条功能当证据的：
    * C1 / C2 是 openpyxl 自己带的日期与日期时间（它把它们写成**自定义**格式号 164/165，
      而不是内置的 14/22 —— 所以只查内置表会漏掉真件）；
    * C3/C4 是百分比与货币；C5 是带汉字字面量的自定义日期格式（`yyyy"年"m"月"d"日"`），
      字面量不剥掉就会把「月」这种字当成月份标记；
    * **C7 是文本 `12/23/2013`**：长得像日期、样式是 General，它不是日期；
    * C8 是布尔；C6 是常规数。
    """
    import datetime as dt
    from openpyxl import Workbook

    wb = Workbook()
    ws = wb.active
    ws.title = "格式"
    ws["A1"] = "标签"
    ws["B1"] = "日期"
    ws["C1"] = dt.date(2013, 12, 23)
    ws["C2"] = dt.datetime(2013, 12, 23, 15, 15, 0)
    ws["C3"] = 0.125
    ws["C3"].number_format = "0.0%"
    ws["C4"] = 124000
    ws["C4"].number_format = "¥#,##0.00"
    ws["C5"] = dt.date(2013, 12, 23)
    ws["C5"].number_format = 'yyyy"年"m"月"d"日"'
    ws["C6"] = 1234.5
    ws["C7"] = "12/23/2013"
    ws["C8"] = True
    second = wb.create_sheet("另一张")
    second["A1"] = dt.date(2026, 9, 23)
    wb.save(str(path))


def write_hidden_xlsx(path: Path) -> None:
    """openpyxl：隐藏行、隐藏列，还有藏在隐藏列里的字。

    这份样本是给「隐藏的才是重点」那半条功能当证据的：
    * 第 3、4 行整行隐藏（`<row hidden="1">`）—— 里面坐着 `合计` 那行；
    * C 列隐藏，D/E 两列也隐藏，而且 D2/E2 **有字**：只报「有几列隐藏」不够，
      还得看得见那些字仍然会被算进格子数；
    * 转成 .ods 之后隐藏换了存法 —— 行列只写一个样式名，`collapse` 在那个
      自动样式的 `table-row-properties` / `table-column-properties` 里；
    * 再让 LibreOffice 把 .ods 转回 .xlsx，隐藏列会被并成 `<col min="4" max="5">`
      这种跨列写法（openpyxl 一列一条），少展开一格就少报一列。
    """
    from openpyxl import Workbook

    wb = Workbook()
    ws = wb.active
    ws.title = "预算表"
    ws["A1"], ws["B1"], ws["C1"] = "科目", "金额", "备注"
    ws["A2"], ws["B2"] = "服务器", 124000
    ws["C2"] = "含税"
    ws["D2"] = "第一列批注"
    ws["E2"] = "第二列批注"
    ws["A3"], ws["B3"] = "网络", 18000
    ws["A4"], ws["B4"] = "合计", "=SUM(B2:B3)"
    ws["A5"] = "口径：含税"
    ws.row_dimensions[3].hidden = True
    ws.row_dimensions[4].hidden = True
    for letter in ("C", "D", "E"):
        ws.column_dimensions[letter].hidden = True
    wb.save(str(path))


def write_chart_xlsx(path: Path) -> None:
    """openpyxl：两张图挂在同一张表上，第三张表一张也没有。

    图是办公表格里最常见也最容易「只数得出部件、说不清内容」的东西，所以这份样本
    一次把这几个变量都摆开：
    * 两张图**类型不同**（柱形 `barChart` 与折线 `lineChart`），挂在同一张表 —— 一张表
      两张图才量得出「按表归位」不是按部件序号凑的；
    * 系列名从表头来（`titles_from_data=True`），于是系列的名字是一个**格子引用**
      （`数据!$B$1`），不是字符串 —— 而且那张表的名字是中文，引号与转义都要过一遍；
    * 类目是文本格（月份），数值是数字格：同一张图里两种引用并存；
    * 折线那张只喂一个系列，柱形那张喂两个 —— 系列数不等；
    * 标题写成字面量（`chart.title = "…"`），openpyxl 会写成 `c:rich` 的一段字，
      而 LibreOffice 重写同一份东西时可能改成 `c:strRef` —— 那正是要量的第二种写法；
    * 第三张表完全不挂图，用来钉「这张表没有图」是 0 而不是缺键。
    """
    from openpyxl import Workbook
    from openpyxl.chart import BarChart, LineChart, Reference

    wb = Workbook()
    ws = wb.active
    ws.title = "数据"
    ws.append(["月份", "收入", "支出"])
    ws.append(["一月", 10, 4])
    ws.append(["二月", 25, 9])
    cats = Reference(ws, min_col=1, min_row=2, max_row=3)

    bar = BarChart()
    bar.type = "col"
    bar.title = "逐月收支"
    bar.add_data(Reference(ws, min_col=2, min_row=1, max_col=3, max_row=3), titles_from_data=True)
    bar.set_categories(cats)
    ws.add_chart(bar, "E2")

    line = LineChart()
    line.title = "收入折线"
    line.add_data(Reference(ws, min_col=2, min_row=1, max_row=3), titles_from_data=True)
    line.set_categories(cats)
    ws.add_chart(line, "E20")

    wb.create_sheet("无图")
    wb.save(path)


def write_rules_xlsx(path: Path) -> None:
    """openpyxl：条件格式与数据验证 —— 「这个格子为什么长这样」与「为什么不让填」

    一次把两边都会碰到的变量摆开：
    * 四种规则类型：`cellIs`（大于 100 就涂红加粗，样式在 dxf 那一跳上）、
      `colorScale`（三色阶，不带 dxf）、`iconSet`（三向箭头）、`expression`（公式规则）
      —— 各自的属性不一样，带不带 dxf 下标也不一样；
    * 范围两种写法：一条 `sqref` 里塞两段区间（openpyxl 就是这么写的）与一段连续区间；
    * 数据验证三种：`list`（下拉，选项直接写在 `formula1` 那对引号里）、
      `whole`（介于 1 与 12，两个公式）、`custom`（一条公式），另带 prompt 与 error 两段字
      与两个开关；
    * 第二张表两样都没有 —— 「没有」要报 0，不是缺键。
    """
    from openpyxl import Workbook
    from openpyxl.formatting.rule import CellIsRule, ColorScaleRule, FormulaRule, IconSetRule
    from openpyxl.styles import Font
    from openpyxl.worksheet.datavalidation import DataValidation

    wb = Workbook()
    ws = wb.active
    ws.title = "规则"
    for row, (label, value) in enumerate(
        [("一月", 60), ("二月", 120), ("三月", 300), ("四月", 45), ("五月", 90)], start=2
    ):
        ws.cell(row=row, column=1, value=label)
        ws.cell(row=row, column=2, value=value)
    ws.conditional_formatting.add(
        "B2:B6",
        CellIsRule(operator="greaterThan", formula=["100"], font=Font(bold=True, color="FF9C0006")),
    )
    ws.conditional_formatting.add(
        "A2:A6 B2:B4",
        ColorScaleRule(
            start_type="min", start_color="FFFFFF",
            mid_type="percentile", mid_value=50, mid_color="FFEB84",
            end_type="max", end_color="F8696B",
        ),
    )
    ws.conditional_formatting.add(
        "B2:B6",
        IconSetRule("3Arrows", "percent", [0, 33, 67]),
    )
    ws.conditional_formatting.add(
        "A2:A6",
        FormulaRule(formula=['$B2>200'], font=Font(italic=True)),
    )
    picks = DataValidation(
        type="list",
        formula1='"红,黄,绿"',
        allow_blank=True,
        showErrorMessage=True,
        showInputMessage=True,
        errorTitle="不在选项里",
        error="只能选 红/黄/绿 三个之一",
        promptTitle="选一个",
        prompt="下拉里有三个颜色",
    )
    ws.add_data_validation(picks)
    picks.add("D2:D6")
    months = DataValidation(type="whole", operator="between", formula1="1", formula2="12")
    ws.add_data_validation(months)
    months.add("E2:E6")
    check = DataValidation(type="custom", formula1="=ISNUMBER(B2)", allow_blank=False)
    ws.add_data_validation(check)
    check.add("F2")

    wb.create_sheet("干净")
    wb.save(path)


def write_view_xlsx(path: Path) -> None:
    """openpyxl：窗口的状态与页眉页脚 —— 「冻了哪几行」与「打出来页眉上有什么」

    这两件事住在两个元素上，而且**两家对「没写」的处理完全不同**，所以一次把变量摆开：
    * 第一张表冻在 `B3`（`pane state="frozen"`）、网格线关掉、页签选中、缩放 150%，
      再给奇偶页两套抬头与页脚（含一个字面 `&&`）；
    * 第二张表用**拆分**而不是冻结（`state="split"`，Excel 里「拆分窗口」那一个开关）——
      实测 LibreOffice 的 xlsx 导出把这个 pane **整个丢掉**（同一份件冻住的那张表留着），
      所以这一张是「重写会掉东西」的证据，不是笔者的猜测；
    * 第三张表什么都不设：openpyxl 干脆不写 `headerFooter` 这个元素，
      而 LibreOffice 六个段落一个不落全写出来（空元素）—— 「没写」与「写了空的」
      在两份件里是两回事，键在不在必须分开。
    """
    from openpyxl import Workbook
    from openpyxl.worksheet.views import Pane

    wb = Workbook()
    one = wb.active
    one.title = "冻结"
    one["A1"] = "月份"
    one["B1"] = "收入"
    one["A2"] = "一月"
    one["B2"] = 10
    one.freeze_panes = "B3"
    one.sheet_view.showGridLines = False
    one.sheet_view.tabSelected = True
    one.sheet_view.zoomScale = 150
    one.oddHeader.left.text = "第 &A 页"
    one.oddHeader.center.text = "冻结那张"
    one.oddFooter.left.text = "打开 && 关闭"
    one.oddFooter.right.text = "第 &P 页，共 &N 页"
    one.HeaderFooter.differentOddEven = True
    one.evenHeader.center.text = "偶数页抬头"

    two = wb.create_sheet("拆分")
    two["A1"] = "字"
    two.sheet_view.pane = Pane(xSplit=1, ySplit=2, topLeftCell="B3", activePane="bottomRight", state="split")

    wb.create_sheet("默认")
    wb.save(path)


def write_size_xlsx(path: Path) -> None:
    """openpyxl：列宽、行高、筛选与表对象 —— 「这一列为什么显示不全」

    这四样都在同一张表上，而两家的数**互不相等**（重写一次就换一套换算），所以一次把变量摆开：
    * A 列宽 22.5、C 列宽 4 并且藏起来（同一份件里两种列状态）；
    * 第 2 行高 40、第 3 行高 8（第 1 行什么都不写 —— 「没说过话」的行是一种，说了话的另一种）；
    * 默认行高改成 18（这一条写在 `sheetFormatPr` 上，而「默认列宽」两家写的属性名都不一样）；
    * 一条筛选范围 A1:C3 带一个筛选列（值是甲），另外再挂一个**范围不同**的表对象 `A1:B3`
      —— 表对象自己带一套 `autoFilter`，与表上那一条不是一回事，两个范围都要交出来；
    * 表对象的列名是文件自己写的（openpyxl 拿范围第一行的字当列名，于是第二列叫 `10`）；
    * 第二张表什么都不设，用来钉「没写」的形状。
    """
    from openpyxl import Workbook
    from openpyxl.worksheet.table import Table, TableStyleInfo

    wb = Workbook()
    ws = wb.active
    ws.title = "尺寸"
    for index, one in enumerate([("一月", 10, "甲"), ("二月", 25, "乙"), ("三月", 30, "丙")], start=1):
        for col, value in enumerate(one, start=1):
            ws.cell(row=index, column=col, value=value)
    ws.column_dimensions["A"].width = 22.5
    ws.column_dimensions["C"].width = 4
    ws.column_dimensions["C"].hidden = True
    ws.row_dimensions[2].height = 40
    ws.row_dimensions[3].height = 8
    ws.sheet_format.defaultRowHeight = 18
    ws.auto_filter.ref = "A1:C3"
    ws.auto_filter.add_filter_column(0, ["甲"])
    table = Table(displayName="台账", ref="A1:B3")
    table.tableStyleInfo = TableStyleInfo(name="TableStyleMedium2", showRowStripes=True)
    ws.add_table(table)
    wb.create_sheet("素面")
    wb.save(path)


def write_epoch_xlsx(path: Path) -> None:
    """1904 基准的那套日期：openpyxl 写 `date1904="1"`，LibreOffice 重写写 `"true"`

    同一个序列数在两套基准下不是同一天（差 1462 天），所以「这一格是几号」完全取决于
    读不读 `workbookPr/@date1904`。五格各测一件事：一个日期（40169 → 2013-12-23）、
    一个普通数（1）、一个带时刻的（42370.12783564815 → 2020-01-02 03:04:05）、
    **序列号 60** 那一格（1904 基准下是 1904-03-01，而 1900 基准下那个号是 Excel
    闰年 bug 里不存在的 1900-02-29 —— 那个特例必须只在 1900 那一边生效）、
    还有一格长得像日期而**本来就是字**的（`12/23/2013`，openpyxl 把它写成 `t="inlineStr"`，
    LibreOffice 重写时换成共享字符串 `t="s"` —— 同一条字两种存法）。
    """
    import datetime

    from openpyxl import Workbook
    from openpyxl.utils.datetime import CALENDAR_MAC_1904

    wb = Workbook()
    wb.epoch = CALENDAR_MAC_1904
    ws = wb.active
    ws.title = "日期"
    ws["A1"] = datetime.date(2013, 12, 23)
    ws["A1"].number_format = "YYYY-MM-DD"
    ws["B1"] = 1
    ws["C1"] = datetime.datetime(2020, 1, 2, 3, 4, 5)
    ws["C1"].number_format = "YYYY-MM-DD HH:MM:SS"
    ws["D1"] = 60
    ws["D1"].number_format = "YYYY-MM-DD"
    ws["E1"] = "12/23/2013"
    wb.save(path)


def write_errors_xlsx(path: Path) -> None:
    """算错的格与算成文本的格：一次只改一个变量，各测一种「结果不是数」

    openpyxl 只把公式抄进去，不算，所以它自己写出来的那份里那些格是
    `<c r="B1"><f>1/0</f><v></v></c>` —— 连 `t` 都不写。真正写了 `t="e"` 与
    `<v>#DIV/0!</v>` 的是 LibreOffice 重算过的那一份（两份都收进仓库，见 main 里那一转）。
    四行各测一件事：除零、拼出来的文本（`t="str"`，且 LO 不走共享字符串表）、
    `NA()`、指不到东西的 `VLOOKUP`（LO 重算成 `#VALUE!`，公式里的 `FALSE` 还被补成 `FALSE()`）。
    另有一格布尔（`t="b"`，文件里写的是 `1` 不是 TRUE）与两格正常的数字结果做对照。
    """
    from openpyxl import Workbook

    wb = Workbook()
    ws = wb.active
    ws.title = "错误"
    ws["A1"] = 7
    ws["B1"] = "=1/0"
    ws["A2"] = 5
    ws["B2"] = '="甲"&"乙"'
    ws["B3"] = "=NA()"
    ws["B4"] = "=VLOOKUP(99,A1:A2,2,FALSE)"
    ws["B5"] = "=A1*2"
    ws["C5"] = "=A2*2"
    ws["D1"] = True
    wb.save(path)


def write_rich_xlsx(path: Path) -> None:
    """一个格子里的字分成几段、首尾那两个空格：两家生产者各写一种摆法

    openpyxl 3.1 的富文本走的是**行内串**（`t="inlineStr"` + `<is><r><rPr>…`），一条
    `sharedStrings.xml` 都不写；LibreOffice 重写同一份时把八次引用全搬进字符串表
    （`count="8" uniqueCount="7"` —— 那条「甲」被两个格子用了，两个数都是对的），
    并且给**每一个** `t` 补上 `xml:space="preserve"`。四件事各测一处：
    两段各有格式（A2）、一段有 `rPr` 一段整个没有（A6 —— 那是「这段没格式」与
    「那段格式是空的」的分别）、首尾空格（A3）与开头的制表符（A7，LO 还按字体
    fallback 把它切成两段），另有一格把粗体写在**格子上**而不是串里（B1）做对照。
    """
    from openpyxl import Workbook
    from openpyxl.cell.rich_text import CellRichText, TextBlock
    from openpyxl.cell.text import InlineFont
    from openpyxl.styles import Font

    wb = Workbook()
    ws = wb.active
    ws.title = "字"
    ws["A1"] = "甲"
    ws["A2"] = CellRichText(
        TextBlock(InlineFont(rFont="宋体", sz=11.0, b=True, color="FFC00000"), "重要"),
        TextBlock(InlineFont(rFont="宋体", sz=11.0), "普通"),
    )
    ws["A3"] = "  两头有空格  "
    ws["A4"] = "第一行\n第二行"
    ws["A5"] = "甲"
    ws["A6"] = CellRichText("整段一个格式", TextBlock(InlineFont(i=True), "斜体那截"))
    ws["B1"] = "整格加粗（格式在格子上不在串里）"
    ws["B1"].font = Font(bold=True)
    ws["A7"] = "\ttab 开头"
    wb.save(path)


def write_styled_xlsx(path: Path) -> None:
    """一个格子「长什么样」那一跳：格式在 `cellXfs` 之外的三张表里

    一次摆开六件事：粗体 + 深红 + 换字体（A1）、实心黄底（B1）、四边细线（C1）、
    斜体加下划线（D1）、右对齐 + 垂直居中 + 自动换行（A2）、点状网格底（D2），
    外加两种颜色写法（`indexed="64"` 与 `theme="1" tint="0.5"`）与一个百分比格式。
    openpyxl 只写它觉得要写的那几个 id 与 `applyAlignment`，而 LibreOffice 重写同一份时
    会把 `apply*` 一串旗标、`patternType="none"` 那个占位与九份字体全补出来 ——
    两副都收进仓库，因为「谁省略了什么」正是要看的（`s=` 也一样：一家不写默认那一个）。
    """
    from openpyxl import Workbook
    from openpyxl.styles import Alignment, Border, Color, Font, PatternFill, Side

    wb = Workbook()
    ws = wb.active
    ws.title = "样子"
    ws["A1"] = "加粗深红"
    ws["A1"].font = Font(bold=True, color="FFC00000", name="微软雅黑", sz=12.0)
    ws["B1"] = "黄底实心"
    ws["B1"].fill = PatternFill(fill_type="solid", fgColor="FFFFFF00")
    ws["C1"] = "四边细线"
    thin = Side(style="thin", color="FF000000")
    ws["C1"].border = Border(left=thin, right=thin, top=thin, bottom=thin)
    ws["D1"] = "斜体下划线"
    ws["D1"].font = Font(italic=True, underline="single")
    ws["A2"] = "长字换行右对齐"
    ws["A2"].alignment = Alignment(horizontal="right", vertical="center", wrap_text=True)
    ws["B2"] = "索引色"
    ws["B2"].font = Font(color=Color(indexed=64))
    ws["C2"] = 0.25
    ws["C2"].number_format = "0.00%"
    ws["D2"] = "点状网格底"
    ws["D2"].fill = PatternFill(fill_type="lightGrid", fgColor="FF00B050", bgColor="FFFFFFFF")
    ws["E2"] = "主题色带淡深"
    ws["E2"].font = Font(color=Color(theme=1, tint=0.5))
    ws["A3"] = "默认什么都不写"
    wb.save(path)


def write_dot_png(path: Path) -> None:
    """一张 40×24 的红点：图那一份账要的只是「有一张真图被嵌进去」"""
    from PIL import Image

    Image.new("RGB", (40, 24), (200, 30, 30)).save(path)


def write_images_docx(path: Path, dot: Path) -> None:
    """文档里那张图：尺寸有两处、替代文字在 `wp:docPr`、地址在关系表里

    python-docx 只会写 `wp:inline`；`wp:anchor`（浮在页上、带绕排那一种）由
    `poke_anchor` + LibreOffice 的导出得到，见 `images-float.docx`。
    `descr` 就是 Word「查看替代文字」里那一句 —— 有没有它是无障碍检查真正在问的事，
    因此单独交 `alt_written`。两处尺寸（`wp:extent` 与 `pic:spPr/a:xfrm/a:ext`）都交：
    LibreOffice 重写时把它们换成了另一个数（4cm → 4.001cm → `1440180`），而且把名字
    与那句替代文字**抄进了 `pic:cNvPr`**（python-docx 在那儿写的是原文件名 `dot.png`），
    另外补了一份 `a:picLocks` —— python-docx 只写 `a:graphicFrameLocks` 那一份。
    """
    from docx import Document
    from docx.shared import Cm

    doc = Document()
    doc.add_paragraph("图前的一段。")
    doc.add_picture(str(dot), width=Cm(4), height=Cm(2.4))
    run = doc.paragraphs[-1].runs[-1]
    drawing = run._element.find(
        ".//{http://schemas.openxmlformats.org/wordprocessingml/2006/main}drawing"
    )
    if drawing is not None:
        frame = drawing.find(
            ".//{http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing}inline"
        )
        data = None if frame is None else frame.find(
            ".//{http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing}docPr"
        )
        if data is not None:
            data.set("name", "图 1")
            data.set("descr", "一个红点")
    doc.add_paragraph("图后的一段。")
    doc.save(path)


def poke_anchor(src: Path, dst: Path) -> None:
    """把 `images.odt` 那一格的锚点改成 page，让 LibreOffice 有理由写出 `wp:anchor`

    两个生产者（python-docx 与 LibreOffice 的默认插入）都只往文字流里塞图
    （`wp:inline` / `text:anchor-type="as-char"`），手上没有一份「浮在页上、文字绕着排」
    的件，那一条分支就成了没人走过的路。这一份输入只改两个属性：锚点改 `page`、
    样式换成 `styles.xml` 里那份 `family=graphic` 而带 `style:wrap="dynamic"` 的
    `Graphics` —— 输出那份 docx 里每一个字节都是 LibreOffice 自己写的。
    """
    with zipfile.ZipFile(src) as zin, zipfile.ZipFile(dst, "w") as zout:
        for item in zin.infolist():
            data = zin.read(item.filename)
            if item.filename == "content.xml":
                text = data.decode("utf8")
                for want in ('text:anchor-type="as-char"', 'draw:style-name="fr1"'):
                    if want not in text:
                        raise SystemExit("⚠️  %s 里找不到 %s" % (src.name, want))
                text = text.replace('text:anchor-type="as-char"', 'text:anchor-type="page"')
                text = text.replace('draw:style-name="fr1"', 'draw:style-name="Graphics"')
                data = text.encode("utf8")
            zout.writestr(item, data)


def write_pictures_pptx(path: Path, dot: Path) -> None:
    """页上那两张图：一张给了替代文字，另一张什么都没给

    什么都没给那一张才是这条分支的理由：python-pptx 把**源文件名**写进了
    `p:cNvPr/@descr`（`descr="dot.png"`），于是文件上「有 alt」而那句话不是描述。
    尺寸一处（`a:xfrm/a:ext`）而位置（`a:off`）写在同一层；第二张不写宽高，
    由 python-pptx 按 72 DPI 从像素换算出 `508000`（40px）—— 那一句假设在文件里
    没有任何地方写出来，所以只交这个数，不替它解释。
    替代文字那格用的是私有 XML（python-pptx 没有公开 setter），写的是文件那一层。
    """
    from pptx import Presentation
    from pptx.util import Cm

    prs = Presentation()
    blank = prs.slide_layouts[6]
    first = prs.slides.add_slide(blank)
    pic = first.shapes.add_picture(str(dot), Cm(1), Cm(1), width=Cm(4), height=Cm(2.4))
    pic.name = "红点"
    pic._element.nvPicPr.cNvPr.set("descr", "一个红点")
    second = prs.slides.add_slide(blank)
    second.shapes.add_picture(str(dot), Cm(10), Cm(5))
    third = prs.slides.add_slide(blank)
    third.shapes.add_textbox(Cm(1), Cm(1), Cm(8), Cm(2)).text_frame.text = "没有图的一页"
    prs.save(path)


def write_styles_docx(path: Path) -> None:
    """python-docx：三段各点一个**字符样式**，其中一段还自己另加一个开关

    为什么存这一份：`w:rPr` 里除了直接格式，还可以只写一个样式号（`w:rStyle`），
    那句话就搬到 `word/styles.xml` 里那条 `w:type="character"` 的定义上 ——
    「这串字是粗的」这一问在文件里就有了**两个来处**（实测 Strong 的定义里写着 `<w:b/>`
    而那一段自己一个字都没点）。第二段故意让两处都说话：样式 `Emphasis` 说斜体、
    段上直接写 `<w:b/>` 说粗体，两处各说一半，合成一个「又粗又斜」就是替文件下结论。
    第三段点的是带颜色与小字号大写的 `Subtle Emphasis`（styleId 是 `SubtleEmphasis`，
    名字里那个空格是文件写的），看的是「样式名 ≠ 样式号」。
    第四段什么样式都不点 —— 与上一族同一件事：没这一格与这格是空的先分清楚。
    """
    from docx import Document

    doc = Document()
    one = doc.add_paragraph()
    one.add_run("只有样式说的：")
    one.add_run("强调的字").style = "Strong"
    two = doc.add_paragraph()
    two.add_run("样式说斜、段上自己说粗：")
    both = two.add_run("又粗又斜")
    both.style = "Emphasis"
    both.bold = True
    three = doc.add_paragraph()
    three.add_run("样式里还写着颜色与大写：")
    three.add_run("淡淡的").style = "Subtle Emphasis"
    doc.add_paragraph("这一段什么样式都不点")
    doc.save(path)


def write_placeholder_deck(path: Path) -> None:
    """四页，一次只改一个变量：占位符、占位符 + 文本框、空占位符、只有文本框

    要紧的是 python-pptx 给正文占位符写 `<p:ph idx="1"/>` —— **`type` 不写**。
    两家读者以前一边猜 `other`、一边猜 `title`（规范默认其实是 body），所以这份件
    存在的意义就是让那一格有「文件确实没说」的真凭据。
    """
    from pptx import Presentation
    from pptx.util import Inches

    deck = Presentation()
    one = deck.slides.add_slide(deck.slide_layouts[1])
    one.shapes.title.text = "预算评审"
    body = one.placeholders[1]
    body.text_frame.text = "先讲口径"
    body.text_frame.add_paragraph().text = "再讲数字"

    two = deck.slides.add_slide(deck.slide_layouts[1])
    two.shapes.title.text = "第二页"
    two.shapes.add_textbox(Inches(1), Inches(4), Inches(3), Inches(1)).text_frame.text = "这不是占位符"

    deck.slides.add_slide(deck.slide_layouts[1])  # 占位符在，字是空的

    four = deck.slides.add_slide(deck.slide_layouts[6])
    four.shapes.add_textbox(Inches(0.5), Inches(0.5), Inches(4), Inches(1)).text_frame.text = "只有一个文本框"

    deck.save(path)


def write_print_area_xlsx(path: Path) -> None:
    """四张表，一次只改一个变量：打印区域、区域 + 重复行、区域给成两段、只给重复列

    这一族的答案不在表上，而在 `xl/workbook.xml` 的两条保留名上（`_xlnm.Print_Area` /
    `_xlnm.Print_Titles`），归属是 `localSheetId` 那个**顺序号**。openpyxl 给 sheet 名带引号，
    LibreOffice 重写同一份时把引号全去掉而其它一字不差 —— 所以要两份生产者的件。
    """
    from openpyxl import Workbook

    book = Workbook()
    first = book.active
    first.title = "区域与标题"
    second = book.create_sheet("区域加标题")
    third = book.create_sheet("两段区域")
    fourth = book.create_sheet("什么都没给")
    for sheet in (first, second, third, fourth):
        sheet["A1"] = "项目"
        sheet["B1"] = "金额"
        sheet["C1"] = "备注"
        for row in range(2, 13):
            sheet.cell(row=row, column=1, value="条目%d" % row)
            sheet.cell(row=row, column=2, value=row * 100)
            sheet.cell(row=row, column=3, value="注%d" % row)
    first.print_area = "A1:C10"
    second.print_area = "A1:C10"
    second.print_title_rows = "1:1"
    third.print_area = ["A1:B6", "C8:C12"]
    fourth.print_title_cols = "B:B"
    book.save(path)


def write_tabs_docx(path: Path) -> None:
    """四条段各改一个变量的制表位：左无引导 / 右点引导 / 居中划引导 / 小数点对齐

    python-docx 这一问有**真的** API（`paragraph_format.tab_stops.add_tab_stop(位置, 对齐, 引导)`），
    不必绕 oxml。两条要紧的量出来的事：3cm 落成 `w:pos="1701"`（twip 是整数，落不下 1700.79），
    而「SPACES 那一档引导」在文件里是**整个属性不写** —— 所以读者交 null，不交 "none"。
    每段各写两个制表位（3cm 与 9cm）并按两次 Tab 键，好让「定义」与「字符」两本账分开数。
    """
    from docx import Document
    from docx.enum.text import WD_TAB_ALIGNMENT, WD_TAB_LEADER
    from docx.shared import Cm

    doc = Document()
    doc.add_paragraph("制表位那份账")
    cases = (
        ("左对齐无引导", WD_TAB_ALIGNMENT.LEFT, WD_TAB_LEADER.SPACES),
        ("右对齐点引导", WD_TAB_ALIGNMENT.RIGHT, WD_TAB_LEADER.DOTS),
        ("居中长划引导", WD_TAB_ALIGNMENT.CENTER, WD_TAB_LEADER.DASHES),
        ("小数点对齐", WD_TAB_ALIGNMENT.DECIMAL, WD_TAB_LEADER.SPACES),
    )
    for title, align, leader in cases:
        para = doc.add_paragraph()
        para.paragraph_format.tab_stops.add_tab_stop(Cm(3), align, leader)
        para.paragraph_format.tab_stops.add_tab_stop(
            Cm(9), WD_TAB_ALIGNMENT.LEFT, WD_TAB_LEADER.LINES
        )
        para.add_run(title + "\t第一段字\t右边")
    doc.save(path)



def write_comment_thread_docx(path: Path) -> None:
    """四条段、三条批注：前两条锚在**同一段**上，第三条另起一段，第四段没人锚

    python-docx 1.2 有 `Document.add_comment(那些 run, 文本, 作者, 缩写)`（**没有**回复那一条
    API —— `Comment` 上只有 `add_paragraph` / `add_table`，所以线程这一格只能等真凭据）。
    两处要紧的都在量出来的数里：`w:date` 带 Z，而 LibreOffice 转出的 .odt 那份 `dc:date`
    不带；同一份稿子重写后 `comments.xml` 里三条排成 1,0,2，而正文那九个锚点一字没动。
    """
    from docx import Document

    doc = Document()
    first = doc.add_paragraph("第一段：这条有两个人说过话")
    doc.add_comment(
        first.runs[0], text="第一条批注：请核对数字", author="刘奇", initials="LQ"
    )
    doc.add_comment(
        first.runs[0], text="第二条：同一个锚点上", author="审稿人", initials="SG"
    )
    second = doc.add_paragraph("第二段：这条只有作者")
    doc.add_comment(
        second.runs[0], text="第三条，另一个人写的", author="编辑", initials="JG"
    )
    doc.add_paragraph("第三段：没有批注")
    doc.save(path)


def write_table_header_docx(path: Path) -> None:
    """四张表，一次只改一个变量：只重复第一行 / 重复前两行 / 一个都不重复 / 只重复**中间**那一行

    OOXML 把这件事写成**行上**一枚没有值的 `w:trPr/w:tblHeader`（在场就是重复）。python-docx
    这一版**没有**暴露这个开关：`row.repeat_table_header = True` 是一个静默无效的属性，
    设完 `trPr` 还是 0 条（量的时候才发现），所以照仓库既有的办法走它的 oxml 层写元素。
    第四张故意标在第二行上 —— Word 允许，而 LibreOffice 重写时把这一枚整个丢掉。
    """
    from docx import Document
    from docx.oxml import OxmlElement

    doc = Document()
    doc.add_paragraph("表头重复那份账")
    for title, repeat in (("只重复第一行", [0]), ("重复前两行", [0, 1]),
                          ("一个都不重复", []), ("只重复中间一行", [1])):
        doc.add_heading(title, level=2)
        table = doc.add_table(rows=3, cols=3)
        for row in range(3):
            for col in range(3):
                table.cell(row, col).text = "%s r%d c%d" % (title[:2], row, col)
        for index in repeat:
            properties = table.rows[index]._tr.get_or_add_trPr()
            properties.append(OxmlElement("w:tblHeader"))
    doc.save(path)


def write_autofit_pptx(path: Path) -> None:
    """一个框三种「字与框谁迁就谁」，再加 PowerPoint 自己算出来的那两个缩放数

    `a:bodyPr` 里那一个孩子的**元素名**就是答案（`noAutofit` / `spAutoFit` / `normAutofit`），
    而 `normAutofit` 上的 `fontScale` / `lnSpcReduction` 是生产者算完才写下来的数 ——
    读者不该自己推，只把写的交出去。`wrap` 与四边内间距（`lIns`…）也在同一个元素上，
    两家写出来的数不一样（python-pptx 90000 / LibreOffice 91440 EMU，正是 0.0984 与 0.1 英寸）。
    """
    from pptx import Presentation
    from pptx.enum.text import MSO_AUTO_SIZE
    from pptx.oxml.ns import qn
    from pptx.util import Inches

    pres = Presentation()
    slide = pres.slides.add_slide(pres.slide_layouts[6])
    kinds = (
        (MSO_AUTO_SIZE.NONE, "不缩：这一框的字超出去就超出去"),
        (MSO_AUTO_SIZE.SHAPE_TO_FIT_TEXT, "框随字长"),
        (MSO_AUTO_SIZE.TEXT_TO_FIT_SHAPE, "字缩进框：这一段本来要两行，压成一行了"),
    )
    for at, (kind, words) in enumerate(kinds):
        box = slide.shapes.add_textbox(Inches(0.5), Inches(1.0 + at * 1.6), Inches(2.5), Inches(1.0))
        frame = box.text_frame
        frame.word_wrap = True
        frame.text = words
        frame.auto_size = kind
        if kind is MSO_AUTO_SIZE.TEXT_TO_FIT_SHAPE:
            # 生产者算完之后写在 normAutofit 上的两个数（读者不自己算）
            body = frame._txBody.find(qn("a:bodyPr"))
            fit = body.find(qn("a:normAutofit"))
            if fit is not None:
                fit.set("fontScale", "75000")
                fit.set("lnSpcReduction", "20000")
    second = pres.slides.add_slide(pres.slide_layouts[6])
    box = second.shapes.add_textbox(Inches(0.5), Inches(1.0), Inches(2.5), Inches(1.0))
    box.text_frame.text = "没人设过自动缩放的新框"
    # 量到的一条生产者脾气：python-pptx 新建的文本框**默认就写** `<a:spAutoFit/>`
    # 并把 wrap 设成 none —— 所以「这一框没点任何一种缩放」在这一家要用
    # `MSO_AUTO_SIZE.NONE` 显式说（第一框那样，写出来是 bodyPr 里根本没有那一个孩子），
    # 什么都不做反而是「框跟着字长」。
    pres.save(str(path))


def write_runs_docx(path: Path) -> None:
    """python-docx：一段只点一个字符属性 —— 「这几个字自己写了什么格式」

    为什么存这一份：字符格式在 OOXML 里住在**段里每一串字**自己身上（`w:r/w:rPr`），
    与段落格式（`w:pPr`）是两本账，而两家的写法正好相反：python-docx 不给没格式的那一串
    写 `w:rPr`，LibreOffice 重写时给**每一串**都写一个空的 `<w:rPr></w:rPr>` ——
    「有这一格而里面是空的」与「压根没有这一格」在这两份件里一边一种，
    合成一个布尔就看不见了。九个开关一次只点一个（粗、斜、下划线、删除线、上标、
    红、黄、字号、字体），值都取不会看错的（红 `C00000`、黄 `yellow`、
    9 磅写成半磅 `sz="18"`、宋体）；再加三种只有真件才有的形状：
    * **明确写不粗**（`w:b w:val="0"`）—— 与只写 `<w:b/>`（默认开）是两种拼法，
      读成「有 b 这个孩子所以是粗体」就把这句话读反了；
    * 一个 `rPr` 里**两个孩子**（`<w:b/><w:i/>`）—— 与「一段里两个格式」不是一件事；
    * **一段里三种字各一串**（点、不点、又点）—— 分段是按串分的，不是按段分的。
    """
    from docx import Document
    from docx.enum.text import WD_COLOR_INDEX
    from docx.shared import Pt, RGBColor

    doc = Document()
    doc.add_paragraph(MARK_RUN_BASE)
    for label in ("加粗", "斜体", "下划线", "删除线", "上标", "红色", "高亮", "小一号", "换字体"):
        para = doc.add_paragraph()
        para.add_run("这一段只点" + label + "：")
        body = para.add_run(MARK_RUN_TAIL)
        if label == "加粗":
            body.bold = True
        elif label == "斜体":
            body.italic = True
        elif label == "下划线":
            body.underline = True
        elif label == "删除线":
            body.font.strike = True
        elif label == "上标":
            body.font.superscript = True
        elif label == "红色":
            body.font.color.rgb = RGBColor(0xC0, 0x00, 0x00)
        elif label == "高亮":
            body.font.highlight_color = WD_COLOR_INDEX.YELLOW
        elif label == "小一号":
            body.font.size = Pt(9)
        else:
            body.font.name = "宋体"

    for label, setting in (("不粗", "bold"), ("不斜", "italic")):
        para = doc.add_paragraph()
        para.add_run("这一段明确写着" + label + "：")
        setattr(para.add_run(MARK_RUN_TAIL).font, setting, False)

    both = doc.add_paragraph()
    both.add_run("一串字里两个孩子：")
    twin = both.add_run(MARK_RUN_TAIL)
    twin.bold = True
    twin.italic = True

    mixed = doc.add_paragraph()
    mixed.add_run("一段里三种字：")
    mixed.add_run("这一串点粗。").bold = True
    mixed.add_run("这一串什么都不点。")
    plain = mixed.add_run("这一串又点斜。")
    plain.italic = True
    doc.add_paragraph(MARK_RUN_TAIL_END)
    doc.save(path)


def write_para_docx(path: Path) -> None:
    """python-docx：四种段落写法 + 一节两栏 —— 「这一段到底排成什么样」

    一次把三家会分家的地方都摆开（另两份件由 LibreOffice 导出得到）：
    * 第一段：两端对齐 + 左缩进 3 厘米 + 首行缩进 480 twips + 段前 6 磅段后 3 磅 + 1.5 倍行距；
    * 第二段：右对齐 + 右缩进 24 磅 + **悬挂缩进** + **固定** 18 磅行距（与「1.5 倍」在 docx 里
      是同一个 `w:line` 配两种 `w:lineRule`，在 ODF 里是同一个 `fo:line-height` 配两种单位）；
    * 第三段：什么都不设 —— 「没写」这一种必须有，否则读成 0 还是没读出来分不开；
    * 第四段：居中 + **按字数**缩进（docx 的第二种单位：`w:leftChars="200"` 是两个字，
      ODF 那一家换成了 `loext:margin-left="2ic"`，只看 `fo:` 会当成没缩进）；
    * 末尾另起一节分两栏（`w:cols w:num="2" w:space="425"`）：模板自带的第二节那份
      `w:cols` 只有 `w:space` 没有 `num`，正好是「一栏」的写法。
    """
    from docx import Document
    from docx.enum.text import WD_ALIGN_PARAGRAPH, WD_LINE_SPACING
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn
    from docx.shared import Cm, Pt, Twips

    doc = Document()
    one = doc.add_paragraph("第一段：两端对齐，左缩进三厘米，首行缩进两个字")
    one.alignment = WD_ALIGN_PARAGRAPH.JUSTIFY
    one.paragraph_format.left_indent = Cm(3)
    one.paragraph_format.first_line_indent = Twips(480)
    one.paragraph_format.space_before = Pt(6)
    one.paragraph_format.space_after = Pt(3)
    one.paragraph_format.line_spacing_rule = WD_LINE_SPACING.ONE_POINT_FIVE

    two = doc.add_paragraph("第二段：右对齐，悬挂缩进，固定行距 18 磅")
    two.alignment = WD_ALIGN_PARAGRAPH.RIGHT
    two.paragraph_format.right_indent = Pt(24)
    two.paragraph_format.first_line_indent = Pt(-18)
    two.paragraph_format.line_spacing = Pt(18)
    two.paragraph_format.line_spacing_rule = WD_LINE_SPACING.EXACTLY

    doc.add_paragraph("第三段：什么都不设")

    four = doc.add_paragraph("第四段：按字数缩进")
    four.alignment = WD_ALIGN_PARAGRAPH.CENTER
    node = OxmlElement("w:ind")
    node.set(qn("w:left"), "0")
    node.set(qn("w:leftChars"), "200")
    node.set(qn("w:firstLineChars"), "150")
    four._p.get_or_add_pPr().append(node)

    section = doc.add_section()
    # 模板自带一节一份 `w:cols`（只有 space，没有 num —— 那就是「一栏」的写法），
    # 所以这里改它而不是再塞一份：追加会造出同一个 sectPr 里两个 w:cols
    cols = section._sectPr.find(qn("w:cols"))
    if cols is None:
        cols = OxmlElement("w:cols")
        section._sectPr.append(cols)
    cols.set(qn("w:num"), "2")
    cols.set(qn("w:space"), "425")
    doc.add_paragraph("这一节排在两栏里")
    doc.save(path)


def write_shaded_docx(path: Path) -> None:
    """python-docx：一张 2×3 的表，四个格各带一种「格子自己的样子」

    为什么存这一份：表格最常见的三个问题（这格有没有底色、有没有边框、字贴在格子的
    上边还是下边）在 OOXML 里都写在**格子自己**的 `w:tcPr` 上，与表宽那三本账无关，
    所以三个记号各摆一处、并且留两格**什么都不设** —— 不然「没底色」与「这一族不报底色」
    分不开。三处都是 python-docx 没封装的写法（`shd` / `tcBorders` 只能手搓 `OxmlElement`，
    `vAlign` 有封装），值都取不会看错的：底色 `FFFF00`、双线 `sz="6"` 红色、`bottom`。
    """
    from docx import Document
    from docx.enum.table import WD_ALIGN_VERTICAL
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    doc = Document()
    table = doc.add_table(rows=2, cols=3)
    one = table.cell(0, 0)
    one.text = MARK_SHADE_FILL
    shd = OxmlElement("w:shd")
    shd.set(qn("w:val"), "clear")
    shd.set(qn("w:color"), "auto")
    shd.set(qn("w:fill"), "FFFF00")
    one._tc.get_or_add_tcPr().append(shd)

    two = table.cell(0, 1)
    two.text = MARK_SHADE_BORDER
    borders = OxmlElement("w:tcBorders")
    top = OxmlElement("w:top")
    top.set(qn("w:val"), "double")
    top.set(qn("w:sz"), "6")
    top.set(qn("w:space"), "0")
    top.set(qn("w:color"), "FF0000")
    borders.append(top)
    two._tc.get_or_add_tcPr().append(borders)

    three = table.cell(0, 2)
    three.text = MARK_SHADE_ALIGN
    three.vertical_alignment = WD_ALIGN_VERTICAL.BOTTOM

    table.cell(1, 0).text = MARK_SHADE_PLAIN
    table.cell(1, 1).text = "普通"
    table.cell(1, 2).text = "普通"
    doc.add_paragraph("表外的一段")
    doc.core_properties.title = MARK_SHADE_FILL
    doc.save(str(path))


def write_list_docx(path: Path) -> None:
    """python-docx：一份文档把「这一段是不是列表项、编号从哪来」的三条路各走一遍

    为什么存这一份：这三条路在 OOXML 里住的地方完全不同，只看段上是读不全的 ——
    * 走**样式**的那三段（两份 `List Number` 与一份 `List Bullet`）段上没有任何 `w:numPr`，
      编号来自样式定义里的 `w:numPr`（python-docx 写这三段时一个编号属性都不往段上放）；
    * 走**直接挂段上**的那两段：`w:numPr` 里 `w:ilvl` 与 `w:numId` 成对写，
      而模板给这九份 abstractNum 写的都是 `multiLevelType="singleLevel"`、
      每份只带一条 `w:lvl w:ilvl="0"` —— 于是那句 `ilvl="1"` 在定义里根本没有对应的那一条，
      「numId 解得到、级别解不到」这一种必须有真件撑着；
    * 最后一段点一个不存在的 `numId="77"`：这条要交「解不开」，不能交 0 也不能交默认。
    另外 `w:num` 与 `w:abstractNum` 的对应是**反的**（`numId 1 → abstractNumId 8`），
    而 abstract 里那一级的 `w:lvlText` 圆点不是 `•` 是 Symbol 字体的 `U+F0B7`，
    并且 `w:lvl` 里还有一条 `<w:pStyle w:val="ListNumber"/>` 反向指回样式表 ——
    编号与样式是一个环，读的人只能挑一条边走，挑的那条要写在账上。
    """
    from docx import Document
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    doc = Document()
    doc.add_paragraph(MARK_LIST_PLAIN)
    doc.add_paragraph(MARK_LIST_NUM_1, style="List Number")
    doc.add_paragraph(MARK_LIST_NUM_2, style="List Number")
    doc.add_paragraph(MARK_LIST_BULLET, style="List Bullet")

    def numbered(text: str, num_id: str, ilvl: str):
        one = doc.add_paragraph(text)
        num_pr = OxmlElement("w:numPr")
        level = OxmlElement("w:ilvl")
        level.set(qn("w:val"), ilvl)
        ident = OxmlElement("w:numId")
        ident.set(qn("w:val"), num_id)
        num_pr.append(level)
        num_pr.append(ident)
        one._p.get_or_add_pPr().insert(0, num_pr)
        return one

    numbered(MARK_LIST_DEEP_1, "3", "0")
    numbered(MARK_LIST_DEEP_2, "3", "1")
    numbered(MARK_LIST_DANGLING, "77", "0")
    doc.core_properties.title = MARK_LIST_NUM_1
    doc.save(str(path))


def write_pptx_links(path: Path) -> None:
    """python-pptx：一页三条链接 + 一页一条也没有，一次只改一个变量

    链接在 OOXML 的演示稿里**不住在字里**：那个 run 只写一个 `a:hlinkClick/@r:id`，
    真正的地址在这一页自己的关系表里（与图、与表对象同一类两跳的找法）。
    三条各测一件事：一条站外 http 且显示的字与地址不同、一条 `mailto:`、
    一条整个 run 的字就是地址本身（那种最容易只留一个字）。第二页一个字都链不到，
    所以那一页要交 0，不是缺键。
    """
    from pptx import Presentation
    from pptx.util import Inches

    pres = Presentation()
    slide = pres.slides.add_slide(pres.slide_layouts[5])
    slide.shapes.title.text = MARK_LINK_PAGE
    box = slide.shapes.add_textbox(Inches(1), Inches(2), Inches(7), Inches(2))
    frame = box.text_frame
    frame.word_wrap = True

    def line(text_of: str, url: str | None) -> None:
        one = frame.add_paragraph() if frame.paragraphs[0].runs else frame.paragraphs[0]
        had = one.add_run()
        had.text = text_of
        if url:
            had.hyperlink.address = url

    line("第三季度的说明", "https://example.com/budget")
    line("（口径见附页，这一段没链）", None)
    line("发邮件问预算", "mailto:liuqi@example.com")
    line("https://example.com/raw", "https://example.com/raw")
    quiet = pres.slides.add_slide(pres.slide_layouts[5])
    quiet.shapes.title.text = "第二页"
    plain = quiet.shapes.add_textbox(Inches(1), Inches(2), Inches(6), Inches(1)).text_frame
    plain.paragraphs[0].add_run().text = "这一页一条链接也没有"
    pres.save(path)


def write_pptx_hidden(path: Path) -> None:
    """python-pptx：一页看得见、一页「放映时隐藏」—— 三家把同一件事写在三个地方

    OOXML 是一个 `p:sld@show="0"`（python-pptx 没有开关，但包是它写的，这里只设
    PowerPoint 界面上那个「隐藏幻灯片」会设的唯一一个属性）；ODF 不写在页上，而是
    写在页点名的那份 family=drawing-page 的自动样式里
    （`style:drawing-page-properties/@presentation:visibility="hidden"` —— 实测两页的
    `draw:page` 属性表差别只有 `draw:style-name`，藏不藏要跳一跳才知道）。
    两页都有标题与字，读的人别把藏起来那页当没有这页。
    """
    from pptx import Presentation
    from pptx.util import Inches

    pres = Presentation()
    first = pres.slides.add_slide(pres.slide_layouts[5])
    first.shapes.title.text = "第一页：看得见"
    second = pres.slides.add_slide(pres.slide_layouts[5])
    second.shapes.title.text = "第二页：放映时藏起来"
    second._element.set("show", "0")
    box = second.shapes.add_textbox(Inches(1), Inches(3), Inches(5), Inches(1))
    box.text_frame.paragraphs[0].add_run().text = "藏起来那页也有字"
    pres.save(path)


def write_pptx_charts(path: Path) -> None:
    """python-pptx：同一页两张图（柱形与饼图），第二页一张也没有。

    与 xlsx 那两份图件配成一对：同一套 `c:ser` 的读法要能在两种宿主里走通，而 pptx
    这一家另有两件事要量：图的引用指向的是**内嵌的那张工作簿**（`ppt/embeddings/*.xlsx`
    里那张表的 `Sheet1!$B$1`，不是演示文稿自己的表），以及 LibreOffice 重写这一族时
    往 `ppt/charts/` 里多塞 style 与 colors 部件 —— 按目录数图就会多数。
    """
    from pptx import Presentation
    from pptx.chart.data import CategoryChartData
    from pptx.enum.chart import XL_CHART_TYPE
    from pptx.util import Inches, Pt

    deck = Presentation()
    slide = deck.slides.add_slide(deck.slide_layouts[6])
    box = slide.shapes.add_textbox(Inches(1), Inches(0.4), Inches(6), Inches(0.8))
    box.text_frame.text = PPT_CHART_HEAD
    box.text_frame.paragraphs[0].font.size = Pt(28)
    box.text_frame.paragraphs[0].font.bold = True

    bars = CategoryChartData()
    bars.categories = list(PPT_CHART_CATS)
    bars.add_series(PPT_CHART_SER_1, (10, 25))
    bars.add_series(PPT_CHART_SER_2, (4, 9))
    slide.shapes.add_chart(XL_CHART_TYPE.COLUMN_CLUSTERED, Inches(1), Inches(1.6),
                           Inches(5), Inches(3.4), bars)

    pie = CategoryChartData()
    pie.categories = list(PPT_CHART_PIE_CATS)
    pie.add_series(PPT_CHART_PIE_SER, (124000, 18000))
    slide.shapes.add_chart(XL_CHART_TYPE.PIE, Inches(6.4), Inches(1.6),
                           Inches(3), Inches(3.4), pie)

    plain = deck.slides.add_slide(deck.slide_layouts[6])
    plain.shapes.add_textbox(Inches(1), Inches(1), Inches(6), Inches(1)).text_frame.text = (
        PPT_CHART_PLAIN
    )
    deck.save(str(path))


def write_pptx(path: Path, art: Path) -> None:
    """python-pptx：两页、标题+正文占位符、备注、图片、表格、切换与母版"""
    from pptx import Presentation
    from pptx.util import Inches

    pres = Presentation()
    first = pres.slides.add_slide(pres.slide_layouts[1])
    first.shapes.title.text = MARK_SLIDE_TITLE
    first.placeholders[1].text = f"{MARK_SLIDE_BODY}\n第二条要点"
    first.notes_slide.notes_text_frame.text = MARK_NOTES
    first.shapes.add_picture(str(art), Inches(6), Inches(2), Inches(1), Inches(1))
    second = pres.slides.add_slide(pres.slide_layouts[5])
    second.shapes.title.text = "第二页：数字"
    table = second.shapes.add_table(2, 2, Inches(1), Inches(2), Inches(4), Inches(1)).table
    table.cell(0, 0).text = MARK_CELL_A1
    table.cell(0, 1).text = "金额"
    table.cell(1, 0).text = MARK_CELL_B2
    table.cell(1, 1).text = "124000"
    core = pres.core_properties
    core.title = MARK_TITLE
    core.author = MARK_AUTHOR
    core.comments = "fixture produced by python-pptx"
    core.keywords = MARK_KEYWORD
    pres.save(str(path))


def write_pptx_tables(path: Path) -> None:
    """python-pptx：一页一张 3×3 的表，**一次只改一个变量**

    合并（横的与竖的各一处）、只给第二行设行高、只给第一列设列宽、只给一个格子设
    垂直对齐与左右边距、只给一个格子设填充色、还有一个格子写两段。
    这一族的合并与 docx 不一样：被合掉的那一格**照样在场**（`hMerge` / `vMerge`，字是空的），
    起点那格写 `gridSpan` / `rowSpan` —— 于是「几个格」「几个有字的格」与「跨度之和」是三本账。
    """
    from pptx import Presentation
    from pptx.dml.color import RGBColor
    from pptx.enum.text import MSO_ANCHOR
    from pptx.util import Emu, Inches

    pres = Presentation()
    slide = pres.slides.add_slide(pres.slide_layouts[5])
    slide.shapes.title.text = MARK_TABLE_PAGE
    table = slide.shapes.add_table(3, 3, Inches(1), Inches(2), Inches(6), Inches(2)).table
    words = {
        (0, 0): MARK_CELL_A1,
        (0, 1): "金额",  # 与 (0,0) 横向合并掉
        (0, 2): "备注",
        (1, 0): MARK_CELL_B2,
        (1, 1): "124000",
        (1, 2): "含税",  # 与 (2,2) 纵向合并掉
        (2, 0): "网络\n设备",  # 一格两段
        (2, 1): "8000",
        (2, 2): "",
    }
    for (row, col), text in words.items():
        if text:
            table.cell(row, col).text = text
    table.cell(0, 0).merge(table.cell(0, 1))
    table.cell(1, 2).merge(table.cell(2, 2))
    table.rows[1].height = Emu(914400)
    table.columns[0].width = Emu(2743200)
    table.cell(1, 0).vertical_anchor = MSO_ANCHOR.BOTTOM
    table.cell(1, 0).margin_left = Emu(91440)
    table.cell(1, 0).margin_right = Emu(45720)
    table.cell(2, 1).fill.solid()
    table.cell(2, 1).fill.fore_color.rgb = RGBColor(0xFF, 0xFF, 0x00)
    core = pres.core_properties
    core.title = MARK_TITLE
    core.author = MARK_AUTHOR
    pres.save(str(path))


def add_macro_part(path: Path, out: Path) -> None:
    """把 docx 复制成一份带 `word/vbaProject.bin` 的 docm —— 宏检测要有样本

    这是**合成**的（没有真 VBA 工程，也没有编译器能在这里造一个），所以它只证明
    「包里出现 vbaProject.bin / 声明了宏内容类型」这条检测成立，不证明宏内容可解。
    这一点在 `office-objects` 的 about 文本里要写明白，别让它冒充真宏样本。
    """
    macro = bytes.fromhex("cf11" + "e0a1b11ae1" + "00" * 40)  # 看着像 OLE 的占位字节
    with zipfile.ZipFile(path) as box:
        types = box.read("[Content_Types].xml").decode("utf-8")
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    types = types.replace(
        "</Types>",
        '<Override PartName="/word/vbaProject.bin" ContentType='
        '"application/vnd.ms-office.vbaProject"/>'
        '<Override PartName="/word/vbaSignature.xml" ContentType='
        '"application/vnd.ms-office.vbaSign"/></Types>',
    )
    parts["[Content_Types].xml"] = types.encode("utf-8")
    parts["word/vbaProject.bin"] = macro
    parts["word/vbaSignature.xml"] = b"<vbaSignature/>"
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as box:
        for name, data in parts.items():
            box.writestr(name, data)


def write_csv(path: Path) -> None:
    """CSV 是给 LO 转 xls 的备用输入（openpyxl 缺席时才用）"""
    rows = [
        [MARK_CELL_A1, "金额"],
        [MARK_CELL_B2, "124000"],
        ["网络", "18000"],
        [MARK_TOTAL_LABEL, "=SUM(B2:B3)"],
    ]
    text = "\r\n".join(",".join(one) for one in rows) + "\r\n"
    path.write_text(text, encoding="utf-8-sig")


def write_odp(path: Path) -> None:
    """手工写一个最小合法 ODP（只作为 LO 转 pptx / ppt 的输入，不给 Rust 当 fixture）"""
    from xml.sax.saxutils import escape

    manifest = f"""<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0">
 <manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.presentation"/>
 <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
 <manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>
 <manifest:file-entry manifest:full-path="Meta.xml" manifest:media-type="text/xml"/>
</manifest:manifest>
"""
    styles = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<office:document-styles '
        'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
        'office:version="1.2"/>'
    )
    meta = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<office:document-meta '
        'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
        'xmlns:dc="http://purl.org/dc/elements/1.1/" '
        'xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" office:version="1.2">'
        f"<office:meta><dc:title>{escape(MARK_TITLE)}</dc:title>"
        f"<dc:creator>{escape(MARK_AUTHOR)}</dc:creator>"
        f"<meta:generator>libreoffice-fixture</meta:generator></office:meta>"
        "</office:document-meta>"
    )
    draw_page = (
        '<draw:page draw:name="page1" draw:style-name="dp1" draw:master-page-name="mp1">'
        f'<draw:frame draw:style-name="ft1'
        '" text:anchor-type="shape" svg:width="20cm" svg:height="4cm" svg:x="2cm" svg:y="2cm">'
        f"<draw:text-box><text:p>{escape(MARK_SLIDE_TITLE)}</text:p></draw:text-box></draw:frame>"
        f'<draw:frame draw:style-name="ft2" text:anchor-type="shape" svg:width="20cm" '
        f'svg:height="8cm" svg:x="2cm" svg:y="7cm">'
        f"<draw:text-box><text:p>{escape(MARK_SLIDE_BODY)}</text:p></draw:text-box></draw:frame>"
        "</draw:page>"
    )
    content = f"""<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
 xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0"
 xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
 xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"
 xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"
 xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"
 office:version="1.2">
 <office:automatic-styles>
  <style:style style:name="dp1" style:family="presentation"/>
  <style:style style:name="ft1" style:family="presentation"/>
  <style:style style:name="ft2" style:family="presentation"/>
 </office:automatic-styles>
 <office:body>
  <office:presentation>
   {draw_page}
   <presentation:notes xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0">
    <text:p>{escape(MARK_NOTES)}</text:p>
   </presentation:notes>
  </office:presentation>
 </office:body>
</office:document-content>
"""
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as box:
        box.writestr("mimetype", "application/vnd.oasis.opendocument.presentation")
        box.writestr("META-INF/manifest.xml", manifest)
        box.writestr("content.xml", content)
        box.writestr("styles.xml", styles)
        box.writestr("Meta.xml", meta)


def write_odt(path: Path) -> None:
    """手工 ODT：ODF 文本的 fixture（结构由本脚本自证，内容是这些锚点串）"""
    from xml.sax.saxutils import escape

    content = f"""<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
 xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2">
 <office:body><office:text>
  <text:h text:outline-level="1">{escape(MARK_HEADING)}</text:h>
  <text:p>{escape(MARK_BODY)}</text:p>
  <text:p>第二段：含税口径，单位元。</text:p>
 </office:text></office:body>
</office:document-content>
"""
    meta = f"""<?xml version="1.0" encoding="UTF-8"?>
<office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
 xmlns:dc="http://purl.org/dc/elements/1.1/"
 xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" office:version="1.2">
 <office:meta>
  <dc:title>{escape(MARK_TITLE)}</dc:title>
  <dc:creator>{escape(MARK_AUTHOR)}</dc:creator>
  <dc:language>zh-CN</dc:language>
  <meta:initial-creator>{escape(MARK_AUTHOR)}</meta:initial-creator>
  <meta:generator>lyco-fixture</meta:generator>
 </office:meta>
</office:document-meta>
"""
    manifest = """<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0">
 <manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/>
 <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
 <manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>
 <manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>
</manifest:manifest>
"""
    styles = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<office:document-styles '
        'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
        'office:version="1.2"/>'
    )
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as box:
        box.writestr("mimetype", "application/vnd.oasis.opendocument.text")
        box.writestr("META-INF/manifest.xml", manifest)
        box.writestr("content.xml", content)
        box.writestr("styles.xml", styles)
        box.writestr("meta.xml", meta)


def write_risk_pdf(path: Path) -> None:
    """手搓一份「会自己动」的 PDF：表单、文档级 JavaScript、附件、Launch 动作，
    外加 MediaBox / Rotate 写在 /Pages 上让页去继承 —— 这五种形状 LibreOffice
    都不肯写（它导出的 PDF 没有脚本、没有表单、每页自带 MediaBox）。

    手搓的风险是「我以为规范是这么写的」，所以这份写完立刻用 pdfinfo 验：
    它报 Form: AcroForm、JavaScript: yes、Pages: 1、Page size 612 x 792、
    Page rot: 90，五个形状就都被第三方读者认了（rot 90 与尺寸正是继承来的）。
    字典少一个 `>` 会让后面的解析全歪且歪得看不出所以然，所以每个对象先自数括号。
    """
    import zlib

    js = b"app.alert('from the document')"
    objects: dict[int, bytes] = {
        1: b"<</Type/Catalog/Pages 2 0 R/AcroForm 12 0 R"
        b"/Names<</JavaScript 13 0 R/EmbeddedFiles 14 0 R>>"
        b"/OpenAction<</S/GoTo/D 3 0 R>>/Lang(en-US)>>",
        2: b"<</Type/Pages/Kids[3 0 R]/Count 1/MediaBox[0 0 612 792]/Rotate 90>>",
        # 页自己不写 MediaBox 与 Rotate：这两项从 /Pages 继承
        3: b"<</Type/Page/Parent 2 0 R"
        b"/Annots[10 0 R 11 0 R]/Contents 4 0 R/Resources<</Font<</F1 5 0 R>>>>"
        b"/AA<</E<</S/JavaScript/JS 16 0 R>>>>>>",
        5: b"<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>",
        6: b"<</Title(Risk fixture)/Author(nobody)/Producer(hand-built)>>",
        7: b"<</Type/Filespec/F(badge.exe)/UF(badge.exe)/EF<</F 8 0 R>>>>",
        8: b"<</Type/EmbeddedFile/Length 10>>\nstream\nMZ........\nendstream",
        10: b"<</Type/Annot/Subtype/Link/Rect[10 700 200 740]"
        b"/A<</S/Launch/F(winword.exe)/P<</O/Open>>>>>>",
        11: b"<</Type/Annot/Subtype/Link/Rect[10 640 200 680]"
        b"/A<</S/URI/URI(https://example.invalid/doc)>>>>",
        12: b"<</Fields[15 0 R]/DR<</Font<</F1 5 0 R>>>>>>",
        13: b"<</Names[(EmbeddedJS) 9 0 R]>>",
        14: b"<</Names[(badge.exe) 7 0 R]>>",
        15: b"<</Type/Annot/Subtype/Widget/FT/Tx/T(name)/V(x)/Rect[0 0 1 1]"
        b"/P 3 0 R>>",
        16: b"<</S/JavaScript/JS(%s)>>" % js,
    }
    body = bytearray(b"BT /F1 12 Tf 72 720 Td (Risk surface fixture) Tj ET")
    packed = zlib.compress(bytes(body), 9)
    objects[4] = (
        b"<</Filter/FlateDecode/Length %d>>\nstream\n" % len(packed) + packed + b"\nendstream"
    )
    stream_js = zlib.compress(js, 9)
    objects[9] = (
        b"<</Filter/FlateDecode/Length %d>>\nstream\n" % len(stream_js)
        + stream_js
        + b"\nendstream"
    )

    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xdc\n")  # 头 + 规范建议的二进制注释行
    offsets: dict[int, int] = {}
    for num in sorted(objects):
        head = objects[num].split(b"\nstream\n", 1)[0]
        assert head.count(b"<<") == head.count(b">>"), (num, head)
        offsets[num] = len(out)
        out += b"%d 0 obj\n" % num + objects[num] + b"\nendobj\n"
    top = max(offsets) + 1
    out += b"xref\n0 %d\n" % top + b"0000000000 65535 f \n"
    for i in range(1, top):
        out += (b"%010d 00000 n \n" % offsets[i]) if i in offsets else b"0000000000 65535 f \n"
    xref_at = len(out)
    out += b"trailer\n<</Size %d/Root 1 0 R/Info 6 0 R>>\nstartxref\n%d\n%%%%EOF\n" % (top, xref_at)
    path.write_bytes(bytes(out))


def write_forms_hier_pdf(source: Path, path: Path) -> None:
    """在 LibreOffice 导的 `notes.pdf` 上挂一套**分层**表单，另存为 `forms-hier.pdf`。

    为什么要有这一份：手上有真生产者的件只到「一层、自己写 /FT、没写 /Ff」为止（`risk.pdf`
    就是），所以 `form` 那一份账里继承、分层、成对候选那几条分支全停在「数过了，没有」。
    这一份把它们一条条摊开：/FT 与 /Ff 只写在祖父上（孩子两级都靠继承）、/Kids 三层、
    `/Opt` 的两种合法写法各一份、一个只写空串 /V 的字段（与「整个不写」是两件事）、
    一个 /V 写成数组的多选字段，并把两个控件同时挂在页的 /Annots 上 —— 字段树与注记
    共用那几条引用，只从 /Fields 走才不会把一条数两遍。

    它**不是**任何编辑器导出的：由 pikepdf（qpdf）写出，README 里要说清这一点。
    写完用 pypdf 独立读回来数过一遍（第三个读者，不参与 CI 对账），见 fixture README。
    """
    import pikepdf

    def text(value: str):
        # 带 FE FF：PDF 文本串没有 BOM 就按 PDFDocEncoding 读，两个字节的中国字会变成
        # 两个拉丁字母 —— 两个读者都照规范读，所以写的时候必须把 BOM 带上
        return pikepdf.String(bytes([0xFE, 0xFF]) + value.encode("utf-16-be"))

    with pikepdf.open(source) as pdf:
        new = pdf.make_indirect
        # 第三层：Person.Address.City —— 只有 /V，类型与开关都靠往上继承
        city = new(
            pikepdf.Dictionary(
                Type=pikepdf.Name.Annot,
                Subtype=pikepdf.Name.Widget,
                T=text("City"),
                V=text("杭州"),
                Rect=pikepdf.Array([0, 0, 100, 20]),
            )
        )
        # 第二层：Person.Address —— 有 /T 与 /Kids，自己一个开关都不写
        address = new(
            pikepdf.Dictionary(
                T=text("Address"),
                Kids=pikepdf.Array([city]),
            )
        )
        # 根上第一条的另一个孩子：自己写 /FT /Tx 与 /Ff，把父上的那两个盖掉
        first = new(
            pikepdf.Dictionary(
                Type=pikepdf.Name.Annot,
                Subtype=pikepdf.Name.Widget,
                T=text("First"),
                FT=pikepdf.Name.Tx,
                Ff=pikepdf.Integer(1),
                V=text("李"),
                DV=text("李"),
                MaxLen=pikepdf.Integer(4),
                Rect=pikepdf.Array([0, 0, 80, 16]),
            )
        )
        person = new(
            pikepdf.Dictionary(
                T=text("Person"),
                FT=pikepdf.Name.Tx,
                Ff=pikepdf.Integer(4),
                Kids=pikepdf.Array([first, address]),
            )
        )
        # 选择框，成对写法：[导出值 显示值] —— 显示值是中国字，导出值是 ASCII
        level = new(
            pikepdf.Dictionary(
                T=text("Level"),
                FT=pikepdf.Name.Ch,
                Opt=pikepdf.Array(
                    [
                        pikepdf.Array([pikepdf.String("1"), text("一")]),
                        pikepdf.Array([pikepdf.String("2"), text("二")]),
                    ]
                ),
                V=pikepdf.String("1"),
                Kids=pikepdf.Array([]),
            )
        )
        # 选择框，摊平写法：一个数组里就是三个显示值，导出值与它们同一串
        # 多选位（第 22 位，524288）下 /V 也是一个数组 —— 这一条也顺便量出「值是数组时
        # 这里交 null」这一族行为
        flags = new(
            pikepdf.Dictionary(
                T=text("Flags"),
                FT=pikepdf.Name.Ch,
                Ff=pikepdf.Integer(524288),
                Opt=pikepdf.Array([text("甲"), text("乙"), text("丙")]),
                V=pikepdf.Array([text("甲"), text("丙")]),
                Kids=pikepdf.Array([]),
            )
        )
        # 勾选框那两条：值在字段上（/V /Yes），**显示状态在控件上**（/AS），
        # 可用的那几种状态写在 /AP 的 /N 字典的键上 —— 三处分开，才说得清「打没打上」
        # 第二条故意不写 /V：文件没说值，只说控件当前是 Off
        agreed = new(
            pikepdf.Dictionary(
                Type=pikepdf.Name.Annot,
                Subtype=pikepdf.Name.Widget,
                T=text("Agreed"),
                FT=pikepdf.Name.Btn,
                Ff=pikepdf.Integer(65536),
                V=pikepdf.Name.Yes,
                AS=pikepdf.Name.Yes,
                AP=pikepdf.Dictionary(
                    N=pikepdf.Dictionary(
                        Yes=new(pikepdf.Stream(pdf, b"q Q n")),
                        Off=new(pikepdf.Stream(pdf, b"q Q n")),
                    )
                ),
                Rect=pikepdf.Array([0, 0, 12, 12]),
            )
        )
        extra = new(
            pikepdf.Dictionary(
                Type=pikepdf.Name.Annot,
                Subtype=pikepdf.Name.Widget,
                T=text("Extra"),
                FT=pikepdf.Name.Btn,
                Ff=pikepdf.Integer(65536),
                AS=pikepdf.Name.Off,
                AP=pikepdf.Dictionary(
                    N=pikepdf.Dictionary(
                        Yes=new(pikepdf.Stream(pdf, b"q Q n")),
                        Off=new(pikepdf.Stream(pdf, b"q Q n")),
                    )
                ),
                Rect=pikepdf.Array([0, 0, 12, 12]),
            )
        )
        # 单选那一族：父上写 /FT /Btn 与 /V，两个孩子各是一个控件、各写自己的 /AS；
        # 其中一个与父上的 /V 对得上，另一个不对 —— 那份不一致要能看出来，不替它圆
        pick = new(
            pikepdf.Dictionary(
                T=text("Pick"),
                FT=pikepdf.Name.Btn,
                Ff=pikepdf.Integer(131072),
                V=pikepdf.Name.One,
                Kids=pikepdf.Array([]),
            )
        )
        on_one = new(
            pikepdf.Dictionary(
                Type=pikepdf.Name.Annot,
                Subtype=pikepdf.Name.Widget,
                T=text("On"),
                AS=pikepdf.Name.One,
                AP=pikepdf.Dictionary(
                    N=pikepdf.Dictionary(
                        One=new(pikepdf.Stream(pdf, b"q Q n")),
                        Two=new(pikepdf.Stream(pdf, b"q Q n")),
                    )
                ),
                Rect=pikepdf.Array([0, 0, 12, 12]),
            )
        )
        on_two = new(
            pikepdf.Dictionary(
                Type=pikepdf.Name.Annot,
                Subtype=pikepdf.Name.Widget,
                T=text("Two"),
                AS=pikepdf.Name.Two,
                Rect=pikepdf.Array([0, 0, 12, 12]),
            )
        )
        for kid in (on_one, on_two):
            kid.Parent = pick
            pick.Kids.append(kid)
        # 孤儿：根上第三条，没有 /FT，也没有任何孩子与控件；/V 写了，但写的是空串
        orphan = new(
            pikepdf.Dictionary(
                T=text("Ghost"),
                V=pikepdf.String(""),
            )
        )
        # 真表单两头都写：孩子的 /Parent 回填上
        for parent, kids in ((person, [first, address]), (address, [city])):
            for kid in kids:
                kid.Parent = parent
        page = pdf.Root.Pages.Kids[0]
        annots = page.get("/Annots")
        if annots is None:
            page["/Annots"] = pikepdf.Array()
            annots = page["/Annots"]
        for widget in (first, city, agreed, extra, on_one, on_two):
            annots.append(widget)
        pdf.Root.AcroForm = new(
            pikepdf.Dictionary(
                Fields=pikepdf.Array([person, level, flags, agreed, extra, pick, orphan]),
                DA=pikepdf.String("/Helv 0 Tf 0 g "),
                NeedAppearances=True,
                SigFlags=pikepdf.Integer(1),
            )
        )
        pdf.save(path)


FOOTNOTE_RTF = r"""{\rtf1\ansi\ansicpg1252\deff0{\fonttbl{\f0 Calibri;}}
\pard Quarterly budget note.\par
This sentence carries a footnote{\footnote\fs16 Footnote: the numbers are gross.} and keeps going.\par
A second line carries a second note{\footnote\fs16 Second footnote: see the budget policy.}\par
Last line.\par
}
"""


def write_footnote_rtf(path: Path) -> None:
    """写一份带两条脚注的 RTF，只为让 LibreOffice 把它导入成一份**真 OOXML**。

    python-docx 没有加脚注的 API（1.2 也没有），所以 `word/footnotes.xml` 那一条分支
    一直没件真文件可走。交出去的 docx 完全由 LibreOffice 写出：连分隔符与续分符
    （`w:type="separator"` / `"continuationSeparator"`）也是它自己加的 —— 那正好是
    「数脚注不能只数 `w:footnote` 元素」的证据。
    写法要按 `{\footnote ...}` 这一族：`\footnote{...}` 那种 LibreOffice 的导入器会
    把字串行（第一版就把手上的字体名当成了脚注正文）。
    """
    path.write_text(FOOTNOTE_RTF, encoding="ascii")


ENDNOTE_NS = (
    'xmlns:o="urn:schemas-microsoft-com:office:office" '
    'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" '
    'xmlns:v="urn:schemas-microsoft-com:vml" '
    'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
    'xmlns:w10="urn:schemas-microsoft-com:office:word" '
    'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
    'xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" '
    'xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" '
    'xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup" '
    'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" '
    'xmlns:wp14="http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing" '
    'xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" '
    'xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml" '
    'mc:Ignorable="w14 wp14 w15"'
)

ENDNOTE_PART = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
    f"<w:endnotes {ENDNOTE_NS}>"
    '<w:endnote w:id="0" w:type="separator"><w:p><w:pPr><w:rPr><w:sz w:val="12"/></w:rPr></w:pPr>'
    "<w:r></w:r></w:p></w:endnote>"
    '<w:endnote w:id="1" w:type="continuationSeparator">'
    "<w:p><w:pPr><w:rPr><w:sz w:val=\"12\"/></w:rPr></w:pPr><w:r></w:r></w:p></w:endnote>"
    '<w:endnote w:id="2">'
    '<w:p><w:pPr><w:pStyle w:val="EndnoteText"/><w:bidi w:val="0"/><w:rPr></w:rPr></w:pPr>'
    '<w:r><w:rPr><w:rStyle w:val="Style14"/></w:rPr><w:endnoteRef/></w:r>'
    '<w:r><w:rPr><w:sz w:val="16"/></w:rPr>'
    f"<w:t>{MARK_ENDNOTE}</w:t></w:r>"
    "</w:p></w:endnote>"
    "</w:endnotes>"
)


def write_endnote_seed(src: Path, dst: Path) -> None:
    """把一条尾注注进 notes-foot.docx，只为让 LibreOffice 照着再写一份。

    尾注这一条分支一直没有真件：Writer 没有「尾注」这个概念，RTF 的
    `\\endnote` 在导入时就被摊进正文（所以 RTF → docx 那一条路上根本不会有
    `word/endnotes.xml`）。但它的 **docx 导出器**会写这个部件 —— 实测过一次：
    注进去的文件交进去，吐出来的包里 `word/endnotes.xml` 还在，而且两条分隔符
    被它改写成自己那套写法（`<w:separator/>` / `<w:continuationSeparator/>`，
    注样式也叫它自己的 `Style15`）。
    所以手搓的只有「这里有一条尾注」这一个意图与那句字，部件的字节仍是生产者写的。
    """
    src_zip = zipfile.ZipFile(src)
    doc = src_zip.read("word/document.xml").decode("utf-8")
    rels = src_zip.read("word/_rels/document.xml.rels").decode("utf-8")
    types = src_zip.read("[Content_Types].xml").decode("utf-8")

    anchor = '<w:footnoteReference w:id="2"/></w:r>'
    if doc.count(anchor) != 1:
        sys.exit(f"尾注种子锚点不唯一：{doc.count(anchor)} —— 别改 notes-foot.docx 的那条脚注")
    doc = doc.replace(
        anchor,
        anchor
        + '<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr>'
        '<w:endnoteReference w:id="2"/></w:r>',
        1,
    )

    new_id = max(int(one) for one in re.findall(r'Id="rId(\d+)"', rels)) + 1
    rels = rels.replace(
        "</Relationships>",
        f'<Relationship Id="rId{new_id}" '
        'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" '
        'Target="endnotes.xml"/></Relationships>',
    )
    types = types.replace(
        "</Types>",
        '<Override PartName="/word/endnotes.xml" '
        'ContentType="application/vnd.openxmlformats-officedocument'
        '.wordprocessingml.endnotes+xml"/></Types>',
    )

    dst.parent.mkdir(parents=True, exist_ok=True)
    replaced = {
        "word/document.xml": doc,
        "word/_rels/document.xml.rels": rels,
        "[Content_Types].xml": types,
    }
    with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as out:
        for item in src_zip.infolist():
            text = replaced.get(item.filename)
            out.writestr(
                item.filename,
                text.encode("utf-8") if text is not None else src_zip.read(item.filename),
            )
        out.writestr("word/endnotes.xml", ENDNOTE_PART)
    src_zip.close()


def write_toc_seed(src: Path, dst: Path) -> None:
    """把一段目录注进 notes.docx，只为让 LibreOffice 照着再写一份。

    常见一问是「这份文档有没有目录、收了几级」。python-docx 没有目录的 API，
    所以走 `write_endnote_seed` 那条已经验过的路：注进去的内容由 LibreOffice 的
    导出器重写 —— 实测它会保留 `<w:docPartGallery w:val="Table of Contents"/>`
    与那条 `TOC \\o "1-2" \\h` 的域指令（连引号都替它转义成 `&quot;`），
    再转一份 .odt 时目录换成 `text:table-of-content`（名字「目录1」、
    `text:table-of-content-source text:outline-level="2"`）。
    两家的「收了几级」写法根本不同：一个写在域指令的 `\\o` 里，一个写在 source 的
    属性上 —— 所以读的时候不强行统一。
    """
    src_zip = zipfile.ZipFile(src)
    doc = src_zip.read("word/document.xml").decode("utf-8")
    before, sep, after = doc.partition("<w:body>")
    if not sep:
        sys.exit(f"{src.name} 里找不到 <w:body>，注不进目录")
    toc = (
        '<w:sdt><w:sdtPr><w:id w:val="12345678"/><w:docPartObj>'
        '<w:docPartGallery w:val="Table of Contents"/><w:docPartUnique/>'
        "</w:docPartObj></w:sdtPr><w:sdtContent>"
        '<w:p><w:pPr><w:pStyle w:val="TOCHeading"/></w:pPr><w:r><w:t>目录</w:t></w:r></w:p>'
        "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>"
        "<w:r><w:instrText xml:space=\"preserve\"> TOC \\o \"1-2\" \\h </w:instrText></w:r>"
        "<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>"
        "<w:r><w:t>一级标题：预算口径</w:t></w:r>"
        "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>"
        "</w:sdtContent></w:sdt>"
    )
    dst.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as out:
        for item in src_zip.infolist():
            data = before + "<w:body>" + toc + after if item.filename == "word/document.xml" else None
            out.writestr(
                item.filename,
                data.encode("utf-8") if data is not None else src_zip.read(item.filename),
            )
    src_zip.close()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--force", action="store_true", help="重跑前先清掉输出目录")
    args = ap.parse_args()
    if args.force and OUT.exists():
        shutil.rmtree(OUT)
    OUT.mkdir(parents=True, exist_ok=True)
    SCRATCH.mkdir(parents=True, exist_ok=True)
    exe = need_soffice()

    docx = OUT / "notes.docx"
    art = tiny_png(SCRATCH / "dot.png")
    write_docx(docx, art)
    add_custom_props(docx)
    xlsx = OUT / "book.xlsx"
    write_xlsx(xlsx)
    write_formats_xlsx(OUT / "formats.xlsx")
    write_mulrk_xlsx(OUT / "mulrk.xlsx")
    # 表格批注那两跳：openpyxl 写一份（批注部件在 xl/comments/comment1.xml），
    # LibreOffice 转 .ods 一份（批注坐在格子里面），两个生产者两种存法
    write_cell_notes_xlsx(OUT / "cell-notes.xlsx")
    # 第四种存法（.xls 的记录流）的种子，转出的 .xls 在下面那个遗留格式循环里
    write_cell_notes_many(OUT / "cell-notes-many.xlsx")
    pptx = OUT / "deck.pptx"
    write_pptx(pptx, art)
    add_macro_part(docx, OUT / "notes.docm")

    english = OUT / "notes-en.docx"
    write_english_docx(english)

    # 字体那份账：一张字体表 + 五种点法（表里没有的名、主题里的那一个、只点东亚那一路）。
    # 再过一遍 LibreOffice：它把 OOXML 那张表搬成 style:font-face，而「一个名两个属性」
    # 在那里成了「一个 style:name 配一个带引号的 svg:font-family」
    fonts = OUT / "fonts.docx"
    write_fonts_docx(fonts)
    convert(exe, fonts, "odt", SCRATCH)
    if (SCRATCH / "fonts.odt").exists():
        shutil.copyfile(SCRATCH / "fonts.odt", OUT / "fonts.odt")
    convert(exe, fonts, "docx", SCRATCH)
    if (SCRATCH / "fonts.docx").exists():
        shutil.copyfile(SCRATCH / "fonts.docx", OUT / "fonts-lo.docx")

    headers = OUT / "notes-hf.docx"
    write_header_docx(headers)

    # 表这一份要两张：同一批行与格子在 RTF 那条流里数得出（\row 与 \cell 的条数），
    # 数不出一张还是两张 —— 两份件的对照就是这条结论的出处，见 write_tables_docx
    twotables = OUT / "tables.docx"
    write_tables_docx(twotables)

    # 那张纸的第二尺寸：A4 纵向 + 一节横排。Letter 是 python-docx 模板的默认值，
    # 换一个尺寸与方向才量得出换算不是凑上的（见 write_paper_a4_docx）
    paper = OUT / "paper-a4.docx"
    write_paper_a4_docx(paper)

    # 合并格那两份：同一张表在 OOXML 与 ODF 里格子数不一样，这条口径只有这份件能给
    merged = OUT / "tables-merged.docx"
    write_merged_tables_docx(merged)

    # 批注那三份：两条注 + 一个中文作者名。RTF 那一族把注写在流里，列表怎么配、
    # 日期是什么历法，一份只有一条注的件判不出来（见 write_comments_docx）
    commented = OUT / "comments.docx"
    write_comments_docx(commented)

    # 修订这一份账：python-docx 注入四种改动，再让 LibreOffice 转一次。两份都留：
    # LibreOffice 会把一次编辑拆成几个 run（数字与单位各一条），又会丢掉段落标记那一条，
    # 而它自己导出的 ODF 把一次编辑写回一个 changed-region —— 这条对照是合并规则的唯一出处
    revisions = OUT / "revisions.docx"
    write_revisions_docx(revisions)
    convert(exe, revisions, "docx", SCRATCH)
    lo_rev = SCRATCH / "revisions.docx"
    if lo_rev.exists():
        shutil.copyfile(lo_rev, OUT / "revisions-lo.docx")
        convert(exe, OUT / "revisions-lo.docx", "odt", SCRATCH)
        odt_rev = SCRATCH / "revisions-lo.odt"
        if odt_rev.exists():
            shutil.copyfile(odt_rev, OUT / "revisions.odt")
        else:
            print("⚠️  没拿到 revisions.odt")
    else:
        print("⚠️  没拿到 revisions-lo.docx")

    # 保护这份账：docx 的编辑限制写在 settings.xml，xlsx 的写在 workbook 与每张表上。
    # 手注入的那两份留着（它们带 LibreOffice 导出时会丢的东西），LO 重写的三份是主样本
    protected = OUT / "protected.docx"
    write_protected_docx(docx, protected)
    convert(exe, protected, "docx", SCRATCH)
    if (SCRATCH / "protected.docx").exists():
        shutil.copyfile(SCRATCH / "protected.docx", OUT / "protected-lo.docx")
    convert(exe, protected, "odt", SCRATCH)
    if (SCRATCH / "protected.odt").exists():
        shutil.copyfile(SCRATCH / "protected.odt", OUT / "protected.odt")
    else:
        print("⚠️  没拿到 protected.odt")

    locked = OUT / "locked-sheet.xlsx"
    write_locked_sheet_xlsx(xlsx, locked)
    convert(exe, locked, "xlsx", SCRATCH)
    if (SCRATCH / "locked-sheet.xlsx").exists():
        shutil.copyfile(SCRATCH / "locked-sheet.xlsx", OUT / "locked-sheet-lo.xlsx")
    convert(exe, locked, "ods", SCRATCH)
    if (SCRATCH / "locked-sheet.ods").exists():
        shutil.copyfile(SCRATCH / "locked-sheet.ods", OUT / "locked-sheet.ods")
    else:
        print("⚠️  没拿到 locked-sheet.ods")

    # .xls 那一层：同一句话的第三种写法（记录住在被锁那张表自己的子流里）。
    # 两份是一组对照 —— 锁从第一张挪到第二张，三条记录跟着挪窝
    convert(exe, locked, "xls", SCRATCH)
    if (SCRATCH / "locked-sheet.xls").exists():
        shutil.copyfile(SCRATCH / "locked-sheet.xls", OUT / "locked-sheet.xls")
    else:
        print("⚠️  没拿到 locked-sheet.xls")
    second_locked = OUT / "locked-second.xlsx"
    write_locked_second_xlsx(xlsx, second_locked)
    convert(exe, second_locked, "xls", SCRATCH)
    if (SCRATCH / "locked-second.xls").exists():
        shutil.copyfile(SCRATCH / "locked-second.xls", OUT / "locked-second.xls")
    else:
        print("⚠️  没拿到 locked-second.xls")

    # 隐藏行/列那一档：openpyxl 写 xlsx，LibreOffice 转 ods，再转回 xlsx（跨列写法）
    hidden = OUT / "hidden.xlsx"
    write_hidden_xlsx(hidden)
    convert(exe, hidden, "ods", SCRATCH)
    if (SCRATCH / "hidden.ods").exists():
        shutil.copyfile(SCRATCH / "hidden.ods", SCRATCH / "hidden-copy.ods")
        shutil.copyfile(SCRATCH / "hidden.ods", OUT / "hidden.ods")
        convert(exe, SCRATCH / "hidden-copy.ods", "xlsx", SCRATCH / "roundtrip")
        back = SCRATCH / "roundtrip" / "hidden-copy.xlsx"
        if back.exists():
            shutil.copyfile(back, OUT / "hidden-lo.xlsx")
        else:
            print("⚠️  没拿到 hidden-lo.xlsx（.ods → .xlsx 那一转）")
    else:
        print("⚠️  没拿到 hidden.ods")

    # 图那一份账：openpyxl 把两张图挂在同一张表上（字面标题 + 格子引用当系列名）；
    # 转 .ods 再转回 .xlsx 的那一份是第二个生产者 —— LibreOffice 重写整个 chart 部件，
    # 缓存值、标题的写法、系列的对齐都可能是另一种，两份都留件才量得出差别
    chart = OUT / "chart.xlsx"
    write_chart_xlsx(chart)
    convert(exe, chart, "ods", SCRATCH)
    if (SCRATCH / "chart.ods").exists():
        shutil.copyfile(SCRATCH / "chart.ods", OUT / "chart.ods")
        shutil.copyfile(SCRATCH / "chart.ods", SCRATCH / "chart-copy.ods")
        convert(exe, SCRATCH / "chart-copy.ods", "xlsx", SCRATCH / "chart-back")
        back = SCRATCH / "chart-back" / "chart-copy.xlsx"
        if back.exists():
            shutil.copyfile(back, OUT / "chart-lo.xlsx")
        else:
            print("⚠️  没拿到 chart-lo.xlsx（.ods → .xlsx 那一转）")
    else:
        print("⚠️  没拿到 chart.ods")

    # 条件格式与数据验证那一份：openpyxl 写四种规则与三种验证；LibreOffice 经 .ods 转回
    # 的那一份是第二个生产者 —— dxf 的下标、规则的属性与验证的公式都可能换写法
    rules = OUT / "rules.xlsx"
    write_rules_xlsx(rules)
    convert(exe, rules, "ods", SCRATCH)
    if (SCRATCH / "rules.ods").exists():
        shutil.copyfile(SCRATCH / "rules.ods", SCRATCH / "rules-copy.ods")
        convert(exe, SCRATCH / "rules-copy.ods", "xlsx", SCRATCH / "rules-back")
        back = SCRATCH / "rules-back" / "rules-copy.xlsx"
        if back.exists():
            shutil.copyfile(back, OUT / "rules-lo.xlsx")
        else:
            print("⚠️  没拿到 rules-lo.xlsx（.ods → .xlsx 那一转）")
    else:
        print("⚠️  没拿到 rules.ods")

    # 窗口状态与页眉页脚那一份：直接 xlsx → xlsx 让 LibreOffice 重写同一个格式，
    # 才量得出「同一件事换了一家写」差在哪（pane 的属性全被补出来、split 那个整个没了、
    # 六段抬头脚全部写成空元素、每段字前面多一个 `&"Calibri"`）
    view = OUT / "view.xlsx"
    write_view_xlsx(view)
    convert(exe, view, "xlsx", SCRATCH / "view-back")
    made = SCRATCH / "view-back" / "view.xlsx"
    if made.exists():
        shutil.copyfile(made, OUT / "view-lo.xlsx")
    else:
        print("⚠️  没拿到 view-lo.xlsx（xlsx → xlsx 那一转）")

    # 列宽行高与筛选/表对象那一份：同一个格式重写同一个格式，才量得出「换一家换算就换一套数」
    size = OUT / "size.xlsx"
    write_size_xlsx(size)
    convert(exe, size, "xlsx", SCRATCH / "size-back")
    made = SCRATCH / "size-back" / "size.xlsx"
    if made.exists():
        shutil.copyfile(made, OUT / "size-lo.xlsx")
    else:
        print("⚠️  没拿到 size-lo.xlsx（xlsx → xlsx 那一转）")

    # 「结果不是数」那三副：openpyxl 写的那份没有缓存值（`t` 都不写），所以除零长什么样
    # 只有让 LibreOffice 重算一遍才看得见；同一份再转一次 .ods，看 ODF 怎么摆同一个错误
    errors = OUT / "errors.xlsx"
    write_errors_xlsx(errors)
    convert(exe, errors, "xlsx", SCRATCH / "errors-back")
    made = SCRATCH / "errors-back" / "errors.xlsx"
    if made.exists():
        shutil.copyfile(made, OUT / "errors-lo.xlsx")
    else:
        print("⚠️  没拿到 errors-lo.xlsx（xlsx → xlsx 那一转）")
    convert(exe, errors, "ods", SCRATCH / "errors-ods")
    made = SCRATCH / "errors-ods" / "errors.ods"
    if made.exists():
        shutil.copyfile(made, OUT / "errors.ods")
    else:
        print("⚠️  没拿到 errors.ods")

    # 1904 基准那两件：date1904 一个键决定整套日期差四年，而手上从来没有一份真写过它的件
    epoch = OUT / "epoch.xlsx"
    write_epoch_xlsx(epoch)
    convert(exe, epoch, "xlsx", SCRATCH / "epoch-back")
    made = SCRATCH / "epoch-back" / "epoch.xlsx"
    if made.exists():
        shutil.copyfile(made, OUT / "epoch-lo.xlsx")
    else:
        print("⚠️  没拿到 epoch-lo.xlsx（xlsx → xlsx 那一转）")

    # 一个格子的字分成几段那三副：openpyxl 只写行内串（一条 sharedStrings 都没有），
    # LibreOffice 重写时全搬进字符串表，同一份 ods 再看 ODF 怎么摆
    rich = OUT / "rich.xlsx"
    write_rich_xlsx(rich)
    convert(exe, rich, "xlsx", SCRATCH / "rich-back")
    made = SCRATCH / "rich-back" / "rich.xlsx"
    if made.exists():
        shutil.copyfile(made, OUT / "rich-lo.xlsx")
    else:
        print("⚠️  没拿到 rich-lo.xlsx（xlsx → xlsx 那一转）")
    convert(exe, rich, "ods", SCRATCH / "rich-ods")
    made = SCRATCH / "rich-ods" / "rich.ods"
    if made.exists():
        shutil.copyfile(made, OUT / "rich.ods")
    else:
        print("⚠️  没拿到 rich.ods")

    # 「长相」那一跳的两副：格式在 cellXfs 之外的三张表里，两家补的东西差很多
    styled = OUT / "styled.xlsx"
    write_styled_xlsx(styled)
    convert(exe, styled, "xlsx", SCRATCH / "styled-back")
    made = SCRATCH / "styled-back" / "styled.xlsx"
    if made.exists():
        shutil.copyfile(made, OUT / "styled-lo.xlsx")
    else:
        print("⚠️  没拿到 styled-lo.xlsx（xlsx → xlsx 那一转）")

    # 文档里那张图那四副：docx 由 python-docx 写，odt / rtf 由 LibreOffice 导出，
    # 再把那份 odt 转回 docx —— 同一条 LO 的两副 OOXML（一处 inline、一处带 dist* 与 effectExtent）
    dot = SCRATCH / "dot.png"
    write_dot_png(dot)
    images = OUT / "images.docx"
    write_images_docx(images, dot)
    convert(exe, images, "odt", SCRATCH / "img-odt")
    made = SCRATCH / "img-odt" / "images.odt"
    if made.exists():
        shutil.copyfile(made, OUT / "images.odt")
    else:
        print("⚠️  没拿到 images.odt")
    convert(exe, images, "rtf", SCRATCH / "img-rtf")
    made = SCRATCH / "img-rtf" / "images.rtf"
    if made.exists():
        shutil.copyfile(made, OUT / "images.rtf")
    else:
        print("⚠️  没拿到 images.rtf")
    if (OUT / "images.odt").exists():
        convert(exe, OUT / "images.odt", "docx", SCRATCH / "img-back")
        made = SCRATCH / "img-back" / "images.docx"
        if made.exists():
            shutil.copyfile(made, OUT / "images-lo.docx")
        else:
            print("⚠️  没拿到 images-lo.docx（odt → docx 那一转）")
    # 「浮在页上、文字绕着排」那一种摆法：手上没有一个生产者自己会写出来，
    # 所以把上面那份 odt 的锚点改一页，再让 LibreOffice 导出 docx（输出全是它写的）
    if (OUT / "images.odt").exists():
        poked = SCRATCH / "images-float.odt"
        poke_anchor(OUT / "images.odt", poked)
        convert(exe, poked, "docx", SCRATCH / "img-float")
        made = SCRATCH / "img-float" / "images-float.docx"
        if not made.exists():
            print("⚠️  没拿到 images-float.docx（锚点那一转）")
        else:
            shutil.copyfile(made, OUT / "images-float.docx")
            # 那一种摆法换到 ODF 里是另一个词（实测 char 而不是 as-char），所以要有一份它自己的件
            convert(exe, made, "odt", SCRATCH / "img-float-back")
            again = SCRATCH / "img-float-back" / "images-float.odt"
            if again.exists():
                shutil.copyfile(again, OUT / "images-float.odt")
            else:
                print("⚠️  没拿到 images-float.odt（anchor 那一族转回 ODF）")

    # 页上那两张图：pptx 由 python-pptx 写，odp 由 LibreOffice 导出，再转回一份 pptx
    # （第三份正是那条来回：LO 的 OOXML 会丢掉 a:picLocks 并把尺寸换成另一个 EMU 数）
    deck_pics = OUT / "deck-pictures.pptx"
    write_pictures_pptx(deck_pics, dot)
    convert(exe, deck_pics, "odp", SCRATCH / "pic-odp")
    made = SCRATCH / "pic-odp" / "deck-pictures.odp"
    if not made.exists():
        print("⚠️  没拿到 deck-pictures.odp")
    else:
        shutil.copyfile(made, OUT / "deck-pictures.odp")
        convert(exe, made, "pptx", SCRATCH / "pic-back")
        again = SCRATCH / "pic-back" / "deck-pictures.pptx"
        if again.exists():
            shutil.copyfile(again, OUT / "deck-pictures-lo.pptx")
        else:
            print("⚠️  没拿到 deck-pictures-lo.pptx（odp → pptx 那一转）")

    # 段落格式与分栏那三件套：docx 由 python-docx 写，odt / rtf 都由 LibreOffice 导出
    para = OUT / "para.docx"
    write_para_docx(para)
    convert(exe, para, "odt", SCRATCH)
    if (SCRATCH / "para.odt").exists():
        shutil.copyfile(SCRATCH / "para.odt", OUT / "para.odt")
    else:
        print("⚠️  没拿到 para.odt")
    convert(exe, para, "rtf", SCRATCH)
    if (SCRATCH / "para.rtf").exists():
        shutil.copyfile(SCRATCH / "para.rtf", OUT / "para.rtf")
    else:
        print("⚠️  没拿到 para.rtf")

    # 字符格式那四件套：docx 由 python-docx 写，odt / rtf 由 LibreOffice 导出，
    # 另留一份 LibreOffice 重写的 docx（就是它给每一串字都补一个空 rPr 的那一份）
    styled_text = OUT / "styled-text.docx"
    write_runs_docx(styled_text)
    convert(exe, styled_text, "odt", SCRATCH / "st-odt")
    made = SCRATCH / "st-odt" / "styled-text.odt"
    if made.exists():
        shutil.copyfile(made, OUT / "styled-text.odt")
    else:
        print("⚠️  没拿到 styled-text.odt")
    convert(exe, styled_text, "rtf", SCRATCH / "st-rtf")
    made = SCRATCH / "st-rtf" / "styled-text.rtf"
    if made.exists():
        shutil.copyfile(made, OUT / "styled-text.rtf")
    else:
        print("⚠️  没拿到 styled-text.rtf")
    convert(exe, styled_text, "docx", SCRATCH / "st-back")
    made = SCRATCH / "st-back" / "styled-text.docx"
    if made.exists():
        shutil.copyfile(made, OUT / "styled-text-lo.docx")
    else:
        print("⚠️  没拿到 styled-text-lo.docx（docx → docx 那一转）")

    # 字符样式那四件套：样式号在一处、样式自己说的话在另一处（word/styles.xml）
    charstyles = OUT / "charstyles.docx"
    write_styles_docx(charstyles)
    convert(exe, charstyles, "odt", SCRATCH / "cs-odt")
    made = SCRATCH / "cs-odt" / "charstyles.odt"
    if made.exists():
        shutil.copyfile(made, OUT / "charstyles.odt")
    else:
        print("⚠️  没拿到 charstyles.odt")
    convert(exe, charstyles, "rtf", SCRATCH / "cs-rtf")
    made = SCRATCH / "cs-rtf" / "charstyles.rtf"
    if made.exists():
        shutil.copyfile(made, OUT / "charstyles.rtf")
    else:
        print("⚠️  没拿到 charstyles.rtf")
    convert(exe, charstyles, "docx", SCRATCH / "cs-back")
    made = SCRATCH / "cs-back" / "charstyles.docx"
    if made.exists():
        shutil.copyfile(made, OUT / "charstyles-lo.docx")
    else:
        print("⚠️  没拿到 charstyles-lo.docx（docx → docx 那一转）")

    # 域与站内跳转：一条 SEQ 编号、一条 DATE、正文与页脚各一条 PAGE，
    # 加一个指着真书签的站内跳转与一个指着不存在的名的那一个
    fields = OUT / "fields.docx"
    write_fields_docx(fields)
    # 目标名要按 soffice 实际写出的那个取：docx → docx 输出的还是 fields.docx，
    # 「-lo」这个后缀是**我们**给它的名字，不是 LibreOffice 起的
    for fmt, born, out_name in (
        ("odt", "fields.odt", "fields.odt"),
        ("rtf", "fields.rtf", "fields.rtf"),
        ("docx", "fields.docx", "fields-lo.docx"),
    ):
        convert(exe, fields, fmt, SCRATCH / ("fd-" + fmt))
        made = SCRATCH / ("fd-" + fmt) / born
        if made.exists():
            shutil.copyfile(made, OUT / out_name)
        else:
            print("⚠️  没拿到 %s（%s 那一转）" % (out_name, fmt))

    # 一个框三种「字与框谁迁就谁」：python-pptx 写 pptx，LibreOffice 转 odp（第三种词表）
    write_autofit_pptx(OUT / "deck-autofit.pptx")
    convert(exe, OUT / "deck-autofit.pptx", "odp", SCRATCH)
    if (SCRATCH / "deck-autofit.odp").exists():
        shutil.copyfile(SCRATCH / "deck-autofit.odp", OUT / "deck-autofit.odp")
    else:
        print("⚠️  没拿到 deck-autofit.odp")

    # 打印区域那三份：openpyxl 写 xlsx，同格式重写一份（引号整层没了）、再转一份 ods
    area = OUT / "print-area.xlsx"
    write_print_area_xlsx(area)
    convert(exe, area, "xlsx", SCRATCH / "print-area-back")
    made_area = SCRATCH / "print-area-back" / "print-area.xlsx"
    if made_area.exists():
        shutil.copyfile(made_area, OUT / "print-area-lo.xlsx")
    else:
        print("⚠️  没拿到 print-area-lo.xlsx（xlsx → xlsx 那一转）")
    convert(exe, area, "ods", SCRATCH)
    if (SCRATCH / "print-area.ods").exists():
        shutil.copyfile(SCRATCH / "print-area.ods", OUT / "print-area.ods")
    else:
        print("⚠️  没拿到 print-area.ods")

    # 占位符那三份：python-pptx 写 pptx，同格式重写一份（正文那格被写成空元素）、再转一份 odp
    phdeck = OUT / "deck-ph.pptx"
    write_placeholder_deck(phdeck)
    convert(exe, phdeck, "pptx", SCRATCH / "deck-ph-back")
    made_ph = SCRATCH / "deck-ph-back" / "deck-ph.pptx"
    if made_ph.exists():
        shutil.copyfile(made_ph, OUT / "deck-ph-lo.pptx")
    else:
        print("⚠️  没拿到 deck-ph-lo.pptx（pptx → pptx 那一转）")
    convert(exe, phdeck, "odp", SCRATCH)
    if (SCRATCH / "deck-ph.odp").exists():
        shutil.copyfile(SCRATCH / "deck-ph.odp", OUT / "deck-ph.odp")
    else:
        print("⚠️  没拿到 deck-ph.odp")

    # 表样式那三份：python-docx 写 docx，同格式重写一份（重算缓存）、再转一份 odt（只剩一个名字）
    styled = OUT / "table-style.docx"
    write_table_style_docx(styled)
    convert(exe, styled, "docx", SCRATCH / "table-style-back")
    made_styled = SCRATCH / "table-style-back" / "table-style.docx"
    if made_styled.exists():
        shutil.copyfile(made_styled, OUT / "table-style-lo.docx")
    else:
        print("⚠️  没拿到 table-style-lo.docx（docx → docx 那一转）")
    convert(exe, styled, "odt", SCRATCH)
    if (SCRATCH / "table-style.odt").exists():
        shutil.copyfile(SCRATCH / "table-style.odt", OUT / "table-style.odt")
    else:
        print("⚠️  没拿到 table-style.odt")

    # 分页开关那三份：python-docx 写 docx，同格式重写一份（丢 pageBreakBefore）、再转一份 odt
    keep = OUT / "keep.docx"
    write_keep_docx(keep)
    convert(exe, keep, "docx", SCRATCH / "keep-back")
    made_keep = SCRATCH / "keep-back" / "keep.docx"
    if made_keep.exists():
        shutil.copyfile(made_keep, OUT / "keep-lo.docx")
    else:
        print("⚠️  没拿到 keep-lo.docx（docx → docx 那一转）")
    convert(exe, keep, "odt", SCRATCH)
    if (SCRATCH / "keep.odt").exists():
        shutil.copyfile(SCRATCH / "keep.odt", OUT / "keep.odt")
    else:
        print("⚠️  没拿到 keep.odt")

    # 行距那三份：python-docx 写 docx，同格式重写一份（补 before/after）、再转一份 odt（换单位）
    line = OUT / "line.docx"
    write_line_docx(line)
    convert(exe, line, "docx", SCRATCH / "line-back")
    made_line = SCRATCH / "line-back" / "line.docx"
    if made_line.exists():
        shutil.copyfile(made_line, OUT / "line-lo.docx")
    else:
        print("⚠️  没拿到 line-lo.docx（docx → docx 那一转）")
    convert(exe, line, "odt", SCRATCH)
    if (SCRATCH / "line.odt").exists():
        shutil.copyfile(SCRATCH / "line.odt", OUT / "line.odt")
    else:
        print("⚠️  没拿到 line.odt")

    # 段边框与底纹那三份：OxmlElement 写 docx，同格式重写一份（丢空壳）、再转一份 odt（换形状）
    boxed = OUT / "pborder.docx"
    write_border_docx(boxed)
    convert(exe, boxed, "docx", SCRATCH / "pborder-back")
    made_boxed = SCRATCH / "pborder-back" / "pborder.docx"
    if made_boxed.exists():
        shutil.copyfile(made_boxed, OUT / "pborder-lo.docx")
    else:
        print("⚠️  没拿到 pborder-lo.docx（docx → docx 那一转）")
    convert(exe, boxed, "odt", SCRATCH)
    if (SCRATCH / "pborder.odt").exists():
        shutil.copyfile(SCRATCH / "pborder.odt", OUT / "pborder.odt")
    else:
        print("⚠️  没拿到 pborder.odt")

    # 文本框那三份：zipfile 写 odt，LibreOffice 转 docx（同一句话写两份）、再重写一份 odt（丢坐标）
    tbox = OUT / "tbox.odt"
    write_tbox_odt(tbox)
    convert(exe, tbox, "docx", SCRATCH / "tbox-docx")
    made_tbox = SCRATCH / "tbox-docx" / "tbox.docx"
    if made_tbox.exists():
        shutil.copyfile(made_tbox, OUT / "tbox.docx")
    else:
        print("⚠️  没拿到 tbox.docx（odt → docx 那一转）")
    convert(exe, tbox, "odt", SCRATCH / "tbox-lo")
    made_tbox_lo = SCRATCH / "tbox-lo" / "tbox.odt"
    if made_tbox_lo.exists():
        shutil.copyfile(made_tbox_lo, OUT / "tbox-lo.odt")
    else:
        print("⚠️  没拿到 tbox-lo.odt（odt 重写那一转）")

    # 书签配对那三份：python-docx + OxmlElement 写 docx，同格式重写一份（删断的）、再转一份 odt
    bkmk = OUT / "bkmks.docx"
    write_bookmark_docx(bkmk)
    convert(exe, bkmk, "docx", SCRATCH / "bkmks-back")
    made_bkmk = SCRATCH / "bkmks-back" / "bkmks.docx"
    if made_bkmk.exists():
        shutil.copyfile(made_bkmk, OUT / "bkmks-lo.docx")
    else:
        print("⚠️  没拿到 bkmks-lo.docx（docx → docx 那一转）")
    convert(exe, bkmk, "odt", SCRATCH)
    if (SCRATCH / "bkmks.odt").exists():
        shutil.copyfile(SCRATCH / "bkmks.odt", OUT / "bkmks.odt")
    else:
        print("⚠️  没拿到 bkmks.odt")

    # 批注那三份：python-docx 写 docx，同格式重写一份（部件换先后）、再转一份 odt（两处合一处）
    noted = OUT / "doc-comments.docx"
    write_comment_thread_docx(noted)
    convert(exe, noted, "docx", SCRATCH / "doc-comments-back")
    made_noted = SCRATCH / "doc-comments-back" / "doc-comments.docx"
    if made_noted.exists():
        shutil.copyfile(made_noted, OUT / "doc-comments-lo.docx")
    else:
        print("⚠️  没拿到 doc-comments-lo.docx（docx → docx 那一转）")
    convert(exe, noted, "odt", SCRATCH)
    if (SCRATCH / "doc-comments.odt").exists():
        shutil.copyfile(SCRATCH / "doc-comments.odt", OUT / "doc-comments.odt")
    else:
        print("⚠️  没拿到 doc-comments.odt")

    # 制表位那三份：python-docx 写 docx，LibreOffice 转 odt（位置换成带单位的串）与 rtf（`\tx`）
    tabbed = OUT / "tabs.docx"
    write_tabs_docx(tabbed)
    convert(exe, tabbed, "odt", SCRATCH)
    if (SCRATCH / "tabs.odt").exists():
        shutil.copyfile(SCRATCH / "tabs.odt", OUT / "tabs.odt")
    else:
        print("⚠️  没拿到 tabs.odt")
    convert(exe, tabbed, "rtf", SCRATCH)
    if (SCRATCH / "tabs.rtf").exists():
        shutil.copyfile(SCRATCH / "tabs.rtf", OUT / "tabs.rtf")
    else:
        print("⚠️  没拿到 tabs.rtf")

    # 表头重复那三份：python-docx 写 docx，同格式重写一份（丢掉标在中间那行的那枚）、再转一份 odt
    repeat = OUT / "table-header.docx"
    write_table_header_docx(repeat)
    convert(exe, repeat, "docx", SCRATCH / "table-header-back")
    made_repeat = SCRATCH / "table-header-back" / "table-header.docx"
    if made_repeat.exists():
        shutil.copyfile(made_repeat, OUT / "table-header-lo.docx")
    else:
        print("⚠️  没拿到 table-header-lo.docx（docx → docx 那一转）")
    convert(exe, repeat, "odt", SCRATCH)
    if (SCRATCH / "table-header.odt").exists():
        shutil.copyfile(SCRATCH / "table-header.odt", OUT / "table-header.odt")
    else:
        print("⚠️  没拿到 table-header.odt")

    # 分节的页眉页脚六格：两节 + titlePg + evenAndOddHeaders + 一条指着不存在关系的号
    write_sections_docx(OUT / "sections.docx")

    # 格子底色/边框/对齐：shaded.docx 由 python-docx 写，shaded-lo.docx 是同一个格式重写
    shaded = OUT / "shaded.docx"
    write_shaded_docx(shaded)
    convert(exe, shaded, "odt", SCRATCH)
    if (SCRATCH / "shaded.odt").exists():
        shutil.copyfile(SCRATCH / "shaded.odt", OUT / "shaded.odt")
    else:
        print("⚠️  没拿到 shaded.odt")
    convert(exe, shaded, "docx", SCRATCH / "shaded-back")
    made_shaded = SCRATCH / "shaded-back" / "shaded.docx"
    if made_shaded.exists():
        shutil.copyfile(made_shaded, OUT / "shaded-lo.docx")
    else:
        print("⚠️  没拿到 shaded-lo.docx（docx → docx 那一转）")

    # 表宽那两份：tables-lo.docx 是「同一个格式重写」（LibreOffice 把 auto/0 换成实数）
    tab = OUT / "tables.docx"
    convert(exe, tab, "docx", SCRATCH / "tables-back")
    made_tab = SCRATCH / "tables-back" / "tables.docx"
    if made_tab.exists():
        shutil.copyfile(made_tab, OUT / "tables-lo.docx")
    else:
        print("⚠️  没拿到 tables-lo.docx（docx → docx 那一转）")

    # 列表与编号那三件套：docx 由 python-docx 写，odt 由 LibreOffice 导出，
    # lists-lo.docx 是「同一个格式重写」那一份（LibreOffice 读进自己的模型再写回 OOXML）
    lists = OUT / "lists.docx"
    write_list_docx(lists)
    convert(exe, lists, "odt", SCRATCH)
    if (SCRATCH / "lists.odt").exists():
        shutil.copyfile(SCRATCH / "lists.odt", OUT / "lists.odt")
    else:
        print("⚠️  没拿到 lists.odt")
    # RTF 那一族也照一份转出来：`\ilvl` / `\ls` 与那一句 `{\listtext…}` 标签的出处
    convert(exe, lists, "rtf", SCRATCH)
    if (SCRATCH / "lists.rtf").exists():
        shutil.copyfile(SCRATCH / "lists.rtf", OUT / "lists.rtf")
    else:
        print("⚠️  没拿到 lists.rtf")
    convert(exe, lists, "docx", SCRATCH / "lists-back")
    made = SCRATCH / "lists-back" / "lists.docx"
    if made.exists():
        shutil.copyfile(made, OUT / "lists-lo.docx")
    else:
        print("⚠️  没拿到 lists-lo.docx（docx → docx 那一转）")

    # 演示稿的第二生产者与图那两份：同一份 pptx 让 LibreOffice 转 odp 再转回来，
    # 版式与母版的条数、段落被拆成几个 run、`sldSz` 上那个 type 属性都会变
    decklo = OUT / "deck-lo.pptx"
    convert(exe, pptx, "odp", SCRATCH)
    if (SCRATCH / "deck.odp").exists():
        shutil.copyfile(SCRATCH / "deck.odp", SCRATCH / "deck-copy.odp")
        convert(exe, SCRATCH / "deck-copy.odp", "pptx", SCRATCH / "deck-back")
        back = SCRATCH / "deck-back" / "deck-copy.pptx"
        if back.exists():
            shutil.copyfile(back, decklo)
        else:
            print("⚠️  没拿到 deck-lo.pptx（.odp → .pptx 那一转）")
    else:
        print("⚠️  没拿到 deck.odp（第二轮的中间件）")

    # 页上那张表那两件：一张 3×3、一次只改一个变量（横合、竖合、行高、列宽、一个格子的
    # 对齐与边距、一个格子的填充色、一格两段），再让 LibreOffice 转 odp 转回来 ——
    # 两家对同一张表说的数不一样（行高 609600 与 609480、`tblPr` 一个带三个开关一个空着），
    # 而合并那四个字两家倒是一字不差
    tables = OUT / "deck-tables.pptx"
    write_pptx_tables(tables)
    tableslo = OUT / "deck-tables-lo.pptx"
    convert(exe, tables, "odp", SCRATCH / "tables-odp")
    middle = SCRATCH / "tables-odp" / "tbl-middle.odp"
    src_middle = SCRATCH / "tables-odp" / "deck-tables.odp"
    if src_middle.exists():
        shutil.copyfile(src_middle, middle)
        convert(exe, middle, "pptx", SCRATCH / "tables-back")
        back = SCRATCH / "tables-back" / "tbl-middle.pptx"
        if back.exists():
            shutil.copyfile(back, tableslo)
            # 中间那份 odp 也留着：它是同一张表的第三种写法（列宽换成 cm）
            shutil.copyfile(src_middle, OUT / "deck-tables.odp")
        else:
            print("⚠️  没拿到 deck-tables-lo.pptx（.odp → .pptx 那一转）")
    else:
        print("⚠️  没拿到 deck-tables.odp（表那一转的中间件）")

    # 页上那几条链接那三件：python-pptx 写一份，LibreOffice 转 odp 再转回 pptx ——
    # 同一条链接两家写的地方不一样（一家 `a:hlinkClick` 指一个关系 id，一家在字上直接
    # 挂 `text:a xlink:href`），所以两跳的找法与地址的写法都要各自量
    links = OUT / "deck-links.pptx"
    write_pptx_links(links)
    convert(exe, links, "odp", SCRATCH / "links-odp")
    links_middle = SCRATCH / "links-odp" / "deck-links.odp"
    if links_middle.exists():
        keep = SCRATCH / "links-odp" / "lnk-middle.odp"
        shutil.copyfile(links_middle, keep)
        convert(exe, keep, "pptx", SCRATCH / "links-back")
        back = SCRATCH / "links-back" / "lnk-middle.pptx"
        if back.exists():
            shutil.copyfile(back, OUT / "deck-links-lo.pptx")
            shutil.copyfile(links_middle, OUT / "deck-links.odp")
        else:
            print("⚠️  没拿到 deck-links-lo.pptx（.odp → .pptx 那一转）")
    else:
        print("⚠️  没拿到 deck-links.odp（链接那一转的中间件）")

    charts = OUT / "deck-chart.pptx"
    write_pptx_charts(charts)
    convert(exe, charts, "odp", SCRATCH)
    if (SCRATCH / "deck-chart.odp").exists():
        # ODP 这一份也要留：图住在 `Object N/` 目录里是 ODF 那一族自己的存法，
        # 与 pptx 那份配成一对才有「同一批图、两种链接法」的对照
        shutil.copyfile(SCRATCH / "deck-chart.odp", OUT / "deck-chart.odp")
        shutil.copyfile(SCRATCH / "deck-chart.odp", SCRATCH / "deck-chart-copy.odp")
        convert(exe, SCRATCH / "deck-chart-copy.odp", "pptx", SCRATCH / "chart-back")
        back = SCRATCH / "chart-back" / "deck-chart-copy.pptx"
        if back.exists():
            shutil.copyfile(back, OUT / "deck-chart-lo.pptx")
        else:
            print("⚠️  没拿到 deck-chart-lo.pptx")
    else:
        print("⚠️  没拿到 deck-chart.odp")

    # 脚注那一条分支：python-docx 给不出 word/footnotes.xml，让 LibreOffice 从 RTF 导入再写出
    foot_rtf = SCRATCH / "notes-foot.rtf"
    write_footnote_rtf(foot_rtf)
    convert(exe, foot_rtf, "docx", SCRATCH)
    if (SCRATCH / "notes-foot.docx").exists():
        shutil.copyfile(SCRATCH / "notes-foot.docx", OUT / "notes-foot.docx")
    else:
        print("⚠️  没拿到 notes-foot.docx")

    # 尾注那一条分支：Writer 没有尾注概念，只有它的 docx 导出器会写这个部件，
    # 所以先注进一份 docx，再让它照抄一遍（见 write_endnote_seed）
    end_seed = SCRATCH / "end" / "notes-end.docx"
    write_endnote_seed(OUT / "notes-foot.docx", end_seed)
    convert(exe, end_seed, "docx", SCRATCH / "end-out")
    if (SCRATCH / "end-out" / "notes-end.docx").exists():
        shutil.copyfile(SCRATCH / "end-out" / "notes-end.docx", OUT / "notes-end.docx")
        # ODF 那一支同样第一次有真尾注可走：Writer 没有尾注概念，但它的 ODT
        # 导出器写 `text:note-class="endnote"`（编号还换成罗马数字 `i`）
        convert(exe, OUT / "notes-end.docx", "odt", SCRATCH / "end-odt")
        if (SCRATCH / "end-odt" / "notes-end.odt").exists():
            shutil.copyfile(SCRATCH / "end-odt" / "notes-end.odt", OUT / "notes-end.odt")
        else:
            print("⚠️  没拿到 notes-end.odt")
    else:
        print("⚠️  没拿到 notes-end.docx")

    # 目录那一条分支：python-docx 给不出目录，同样「注进去再让它照抄」（见 write_toc_seed）
    toc_seed = SCRATCH / "toc" / "toc-seed.docx"
    write_toc_seed(OUT / "notes.docx", toc_seed)
    convert(exe, toc_seed, "docx", SCRATCH / "toc-out")
    if (SCRATCH / "toc-out" / "toc-seed.docx").exists():
        shutil.copyfile(SCRATCH / "toc-out" / "toc-seed.docx", OUT / "toc.docx")
    else:
        print("⚠️  没拿到 toc.docx")

    # 真 ODF 写入者是 LibreOffice：从 OOXML 转过去，比手搓的 content.xml 有说服力
    for src, fmt in (
        (docx, "odt"),
        (xlsx, "ods"),
        (pptx, "odp"),
        (OUT / "formats.xlsx", "ods"),
        (OUT / "cell-notes.xlsx", "ods"),
        (OUT / "toc.docx", "odt"),
        (twotables, "odt"),
        # 那张纸的第二尺寸：ODF 的 `fo:page-width` 与 OOXML 的 twips 是两家写法
        (paper, "odt"),
        (merged, "odt"),
        # 批注那一族也走 ODF 一副：注是嵌在正文段里的 office:annotation
        (commented, "odt"),
    ):
        convert(exe, src, fmt, SCRATCH)
    for name in (
        "notes.odt",
        "book.ods",
        "deck.odp",
        "formats.ods",
        "cell-notes.ods",
        "toc.odt",
        "tables.odt",
        "paper-a4.odt",
        "tables-merged.odt",
        "comments.odt",
    ):
        src = SCRATCH / name
        if src.exists():
            shutil.copyfile(src, OUT / name)
        else:
            print(f"⚠️  没拿到 {name}")

    # 同一批字的第三种写法：LibreOffice 自己导出的 xlsx。它把批注部件放在
    # `xl/comments1.xml`（openpyxl 放 `xl/comments/comment1.xml`），关系 Target
    # 也从绝对 `/xl/...` 变成相对 `../comments1.xml` —— 两条路都得走得到才算读过批注
    if (OUT / "cell-notes.ods").exists():
        convert(exe, OUT / "cell-notes.ods", "xlsx", SCRATCH / "notes-back")
        back = SCRATCH / "notes-back" / "cell-notes.xlsx"
        if back.exists():
            shutil.copyfile(back, OUT / "cell-notes-lo.xlsx")
        else:
            print("⚠️  没拿到 cell-notes-lo.xlsx（.ods → .xlsx 那一转）")

    # 遗留二进制格式：这些就是 MS-CFB 复合文档
    for src, fmt in (
        (docx, "doc"),
        (english, "doc"),
        (xlsx, "xls"),
        (pptx, "ppt"),
        (docx, "rtf"),
        # 数字格式那一族也转一份 .xls：BIFF 把格式坐在 XF → FORMAT 那一跳上，
        # 这一份是那条路的真件样本（见 formats.ods 那一条）
        (OUT / "formats.xlsx", "xls"),
        (OUT / "mulrk.xlsx", "xls"),
        # 隐藏行/列那一族也转一份 .xls：BIFF 把这两件事写在 ROW 与 COLINFO 的字段位上，
        # 这一份是那条路的真件样本（ground truth 是 openpyxl 写的 hidden.xlsx）
        (OUT / "hidden.xlsx", "xls"),
        # 批注的第四种存法：LibreOffice 把一条注拆成三条 BIFF 记录（字、格子与作者、
        # 一个自报的序号），这一份是那条路的真件样本（种子是 cell-notes-many.xlsx）
        (OUT / "cell-notes-many.xlsx", "xls"),
        # 页眉页脚那两份再转两个格式：ODF 的页眉坐在 master-page 的样式里，
        # RTF 的坐在 \header / \footer 目标里 —— 两边都是同一批字的另一种存法
        (headers, "rtf"),
        (headers, "odt"),
        # 脚注与尾注的 RTF 存法：两条都是 `{\*\footnote …}` 群，尾注只多一个 `\ftnalt`；
        # 分隔符另走 `{\*\ftnsep …}` —— 与 OOXML 那两条分隔符是同一件事的第三种写法
        (OUT / "notes-end.docx", "rtf"),
        (twotables, "rtf"),
        # 那张纸的第二尺寸也要 RTF 那一副：`\paperw` / `\landscape` 是第三种写法
        (paper, "rtf"),
        # 目录的第三种写法：RTF 把同一串指令写在 `{\*\fldinst { TOC \\o "1-2" \\h}}` 里，
        # 开关前面的反斜杠在文件里必须成对写（解掉那一对才与 docx 的 instrText 一样）
        (OUT / "toc.docx", "rtf"),
        # 批注的第三种存法：作者与正文分在注的前后两格，中文作者名在 RTF 里被写成
        # 两个问号（同一批字的 docx 那边照抄「刘奇」）—— 两种答案都要留着
        (commented, "rtf"),
    ):
        convert(exe, src, fmt, SCRATCH)
    for name in (
        "notes.doc",
        "notes-en.doc",
        "book.xls",
        "formats.xls",
        "mulrk.xls",
        "hidden.xls",
        "cell-notes-many.xls",
        "deck.ppt",
        "notes.rtf",
        "notes-hf.rtf",
        "notes-end.rtf",
        "tables.rtf",
        "paper-a4.rtf",
        "notes-hf.odt",
        "comments.rtf",
    ):
        src = SCRATCH / name
        if src.exists():
            shutil.copyfile(src, OUT / name)
        else:
            print(f"⚠️  没拿到 {name}（LibreOffice 版本可能不支持该目标格式）")

    # 「放映时隐藏这一页」那三副：python-pptx 写 `show="0"`，LibreOffice 重写同一个格式
    # 留着它，而它转出去的 .odp 把同一句话搬进了页点名的那份 drawing-page 样式里
    hidden_deck = OUT / "deck-hidden.pptx"
    write_pptx_hidden(hidden_deck)
    convert(exe, hidden_deck, "odp", SCRATCH / "hidden-odp")
    made = SCRATCH / "hidden-odp" / "deck-hidden.odp"
    if made.exists():
        shutil.copyfile(made, OUT / "deck-hidden.odp")
        convert(exe, made, "pptx", SCRATCH / "hidden-back")
        again = SCRATCH / "hidden-back" / "deck-hidden.pptx"
        if again.exists():
            shutil.copyfile(again, OUT / "deck-hidden-lo.pptx")
        else:
            print("⚠️  没拿到 deck-hidden-lo.pptx（.odp → .pptx 那一转）")
    else:
        print("⚠️  没拿到 deck-hidden.odp（隐藏那一转的中间件）")

    # ── PDF：这一族的三条路各要一个真件 ─────────────────────────────
    # 1) LibreOffice 导出（Writer 与 Impress 各一份：页面尺寸、/Lang、字体数都不同）
    write_risk_pdf(OUT / "risk.pdf")
    for src in (docx, pptx):
        convert(exe, src, "pdf", SCRATCH)
    for name in ("notes.pdf", "deck.pdf"):
        src = SCRATCH / name
        if src.exists():
            shutil.copyfile(src, OUT / name)
        else:
            print(f"⚠️  没拿到 {name}（LibreOffice 的 pdf 导出）")
    # 批注导进 PDF：LibreOffice 的 headless 默认那一转**把 docx 的批注整个丢掉**
    # （上面那份 notes.pdf 就是证据：一条 /Text 都没有，只剩一条链接）。要显式给
    # filter 选项才带得出来，而那一条 /Text 还顺手配了一条 /Popup —— 两个都在
    # 这一页的 /Annots 数组里，所以「几条注记」与「几条批注」是两个数
    convert(
        exe,
        docx,
        'pdf:writer_pdf_Export:{"ExportAnnotations":{"type":"boolean","value":"true"}}',
        SCRATCH / "annots",
    )
    made = SCRATCH / "annots" / "notes.pdf"
    if made.exists():
        shutil.copyfile(made, OUT / "pdf-comments.pdf")
    else:
        print("⚠️  没拿到 pdf-comments.pdf（带 ExportAnnotations 的那一转）")
    # 2) qpdf（pikepdf 带的）再存一次，造出 LibreOffice 不写的两种形状：
    #    `object_stream_mode=generate` 把大部分对象搬进 /Type /ObjStm 并写出
    #    /Type /XRef —— 那种文件里根本没有 `trailer` 这个词，/Info 只住在那个流字典里；
    #    `Encryption(R=6)` 是 AES-256 真加密，pdfinfo 不给口令直接拒绝打开。
    #    这两份都经 pdfinfo / pdffonts 第三方读者核对过（见 fixture README）
    try:
        import pikepdf
    except Exception:  # noqa: BLE001 - 没有就让主脚本照常跑完其余 fixture
        print("⚠️  没有 pikepdf（python -m pip install pikepdf）：跳过 objstm.pdf / locked.pdf")
    else:
        source = OUT / "notes.pdf"
        if source.exists():
            with pikepdf.open(source) as box:
                box.save(
                    OUT / "objstm.pdf",
                    force_version="1.5",
                    object_stream_mode=pikepdf.ObjectStreamMode.generate,
                )
            with pikepdf.open(source) as box:
                box.save(
                    OUT / "locked.pdf",
                    encryption=pikepdf.Encryption(user="lbin-test", owner="lbin-owner", R=6),
                )
            # 3) 还有一份**只设 owner 口令**的：用户能打开，但 /P 里那些位就生效了
            #    （locked.pdf 连用户口令都要，工具根本进不去，权限位读不到）。
            #    pdfinfo 能把它读成 `Encrypted: yes (print:no copy:no change:yes
            #    addNotes:no algorithm:AES-256)` —— 那就是 /P 各位的第三方译法。
            with pikepdf.open(source) as box:
                box.save(
                    OUT / "perms.pdf",
                    encryption=pikepdf.Encryption(
                        user="",
                        owner="lbin-owner",
                        R=6,
                        allow=pikepdf.Permissions(
                            print_lowres=False,
                            print_highres=False,
                            modify_assembly=False,
                            modify_annotation=False,
                            modify_form=False,
                            extract=False,
                            accessibility=True,
                        ),
                    ),
                )
            print("  objstm.pdf / locked.pdf 由 qpdf 写出（口令 lbin-test，只为测加密检测）")
            print("  perms.pdf 只设 owner 口令：/P 的位生效，pdfinfo 能读出来对账")
            # 4) 表单那一份账要的三种形状（继承、分层、成对候选）没有编辑器肯写：
            #    在 notes.pdf 上挂一套分层字段，另存一份（详见函数说明）
            write_forms_hier_pdf(source, OUT / "forms-hier.pdf")
            print("  forms-hier.pdf 由 pikepdf 挂上三层字段：这一份不是编辑器导的")

    print("fixture 清单（每个文件的生产者见函数注释）：")
    for one in sorted(OUT.iterdir()):
        if one.is_file():
            print(f"  {one.name:16} {one.stat().st_size:>9,} bytes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
