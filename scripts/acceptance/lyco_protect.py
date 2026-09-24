"""「这份文件还能动吗」的第二读者：只用标准库 ElementTree。

与 `lilyco-binfmt/src/protect.rs` 是同一套判断的两份实现。这里刻意只交出
**probe 要比的那几项**（结论、类型、有没有校验值、每张表一条），字段名与 Rust 那边
一致，省得两边各自发明一套形状。

两边都必须认两种布尔拼法：openpyxl 写 `sheet="1" formatCells="0"`，
LibreOffice 重写同一份东西时写 `sheet="true" formatCells="false"`。
"""

from __future__ import annotations

import xml.etree.ElementTree as ET

DOCX_SWITCHES = (
    "objects", "scenarios", "formatCells", "formatColumns", "formatRows",
    "insertColumns", "insertRows", "insertHyperlinks", "deleteColumns",
    "deleteRows", "selectLockedCells", "sort", "autoFilter", "pivotTables",
    "selectUnlockedCells",
)
ODF_ITEMS = ("ProtectForm", "ProtectBookmarks", "ProtectFields", "ProtectBook")


def local(tag: str) -> str:
    return tag.split("}")[1] if "}" in tag else tag


def attr(el, name: str) -> str | None:
    for key, value in el.attrib.items():
        if local(key) == name:
            return value
    return None


def on_off(raw: str | None) -> bool | None:
    """`1`/`true` 与 `0`/`false` 都认；认不出来给 None（不猜）"""
    if raw is None:
        return None
    text = raw.strip()
    if text in ("1", "true"):
        return True
    if text in ("0", "false"):
        return False
    return None


def docx_protection(settings: ET.Element | None) -> dict:
    root = (
        next((one for one in settings.iter() if local(one.tag) == "documentProtection"), None)
        if settings is not None
        else None
    )
    if root is None:
        return {"element": False, "protected": False}
    enforced = on_off(attr(root, "enforcement"))
    if enforced is None:
        # 规范里 `w:enforcement` 缺省是 on：元素写了却没说关，就是开着
        enforced = True
    return {
        "element": True,
        "protected": enforced,
        "enforcement": enforced,
        "edit": attr(root, "edit"),
        "password": bool(attr(root, "hash")),
        "algorithm": attr(root, "cryptAlgorithmType"),
        "spin_count": attr(root, "cryptSpinCount"),
    }


def odt_protection(settings: ET.Element | None) -> dict:
    items: dict = {}
    if settings is not None:
        for one in settings.iter():
            if local(one.tag) != "config-item":
                continue
            name = attr(one, "name")
            if name in ODF_ITEMS:
                items[name] = (one.text or "").strip() == "true"
    return {"items": items, "protected": any(items.values())}


def xlsx_protection(workbook: ET.Element, sheets: list) -> dict:
    hit = next(
        (one for one in workbook.iter() if local(one.tag) == "workbookProtection"), None
    )
    if hit is None:
        book = {"element": False}
    else:
        book = {
            "element": True,
            "lock_structure": on_off(attr(hit, "lockStructure")),
            "lock_windows": on_off(attr(hit, "lockWindows")),
            "book_password": bool(attr(hit, "password")),
        }
    out = []
    for name, root in sheets:
        one = next((kid for kid in root.iter() if local(kid.tag) == "sheetProtection"), None)
        if one is None:
            out.append({"name": name, "element": False, "protected": False})
            continue
        written = {}
        for key in DOCX_SWITCHES:
            value = on_off(attr(one, key))
            if value is not None:
                written[key] = value
        sheet_on = on_off(attr(one, "sheet"))
        out.append(
            {
                "name": name,
                "element": True,
                "protected": sheet_on if sheet_on is not None else True,
                "password": bool(attr(one, "password")),
                "written": written,
            }
        )
    return {"workbook": book, "sheets": out}


def ods_protection(content: ET.Element) -> dict:
    out = []
    for one in content.iter():
        if local(one.tag) != "table" or attr(one, "name") is None:
            continue
        digest = attr(one, "protection-key-digest-algorithm")
        out.append(
            {
                "name": attr(one, "name"),
                "protected": on_off(attr(one, "protected")) or False,
                "password": bool(attr(one, "protection-key")),
                "digest": digest.rsplit("/", 1)[-1] if digest else None,
            }
        )
    return {"sheets": out, "workbook": {"element": False}}
