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
MARK_BODY = "第三季度服务器预算为十二万四千元"
MARK_HEADING = "一级标题：预算口径"
MARK_SHEET = "预算表"
MARK_CELL_A1 = "科目"
MARK_CELL_B2 = "服务器"
MARK_TOTAL_LABEL = "合计"
MARK_SLIDE_TITLE = "预算评审"
MARK_SLIDE_BODY = "新增两台 64 核应用服务器"
MARK_NOTES = "评审时先讲口径再讲数字"
MARK_COMMENT = "这里要补上不含税口径"
MARK_AUTHOR = "liuqi"
MARK_COMPANY = "lilyco"
MARK_KEYWORD = "budget,quarterly"
# 尾注那句：notes-foot.docx 只有脚注，notes-end.docx 在这句上才走得到 `endnote` 那一支
MARK_ENDNOTE = "Endnote: the totals exclude the carry-over."


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
    pptx = OUT / "deck.pptx"
    write_pptx(pptx, art)
    add_macro_part(docx, OUT / "notes.docm")

    english = OUT / "notes-en.docx"
    write_english_docx(english)

    headers = OUT / "notes-hf.docx"
    write_header_docx(headers)

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

    # 真 ODF 写入者是 LibreOffice：从 OOXML 转过去，比手搓的 content.xml 有说服力
    for src, fmt in ((docx, "odt"), (xlsx, "ods"), (pptx, "odp"), (OUT / "formats.xlsx", "ods")):
        convert(exe, src, fmt, SCRATCH)
    for name in ("notes.odt", "book.ods", "deck.odp", "formats.ods"):
        src = SCRATCH / name
        if src.exists():
            shutil.copyfile(src, OUT / name)
        else:
            print(f"⚠️  没拿到 {name}")

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
        # 页眉页脚那两份再转两个格式：ODF 的页眉坐在 master-page 的样式里，
        # RTF 的坐在 \header / \footer 目标里 —— 两边都是同一批字的另一种存法
        (headers, "rtf"),
        (headers, "odt"),
    ):
        convert(exe, src, fmt, SCRATCH)
    for name in (
        "notes.doc",
        "notes-en.doc",
        "book.xls",
        "formats.xls",
        "mulrk.xls",
        "hidden.xls",
        "deck.ppt",
        "notes.rtf",
        "notes-hf.rtf",
        "notes-hf.odt",
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
