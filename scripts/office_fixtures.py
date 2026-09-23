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
    pptx = OUT / "deck.pptx"
    write_pptx(pptx, art)
    add_macro_part(docx, OUT / "notes.docm")

    # 真 ODF 写入者是 LibreOffice：从 OOXML 转过去，比手搓的 content.xml 有说服力
    for src, fmt in ((docx, "odt"), (xlsx, "ods"), (pptx, "odp")):
        convert(exe, src, fmt, SCRATCH)
    for name in ("notes.odt", "book.ods", "deck.odp"):
        src = SCRATCH / name
        if src.exists():
            shutil.copyfile(src, OUT / name)
        else:
            print(f"⚠️  没拿到 {name}")

    # 遗留二进制格式：这些就是 MS-CFB 复合文档
    for src, fmt in ((docx, "doc"), (xlsx, "xls"), (pptx, "ppt"), (docx, "rtf")):
        convert(exe, src, fmt, SCRATCH)
    for name in ("notes.doc", "book.xls", "deck.ppt", "notes.rtf"):
        src = SCRATCH / name
        if src.exists():
            shutil.copyfile(src, OUT / name)
        else:
            print(f"⚠️  没拿到 {name}（LibreOffice 版本可能不支持该目标格式）")

    print("fixture 清单（每个文件的生产者见函数注释）：")
    for one in sorted(OUT.iterdir()):
        if one.is_file():
            print(f"  {one.name:16} {one.stat().st_size:>9,} bytes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
