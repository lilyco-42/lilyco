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

    print("fixture 清单（每个文件的生产者见函数注释）：")
    for one in sorted(OUT.iterdir()):
        if one.is_file():
            print(f"  {one.name:16} {one.stat().st_size:>9,} bytes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
