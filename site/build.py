#!/usr/bin/env python3
"""把 docs/ui 的 markdown 构建成 GitHub Pages 静态站。

不引入任何第三方依赖：只做够用的 markdown 子集（标题 / 代码块 / 表格 /
列表 / 引用 / 段落 / 行内代码 / 链接 / 粗体 / 分隔线），并改写 .md 链接。

用法:
    python3 site/build.py        # 产物在 site/dist
"""

from __future__ import annotations

import html
import pathlib
import re
import shutil
import sys

SITE = pathlib.Path(__file__).resolve().parent
REPO = SITE.parent
SRC = REPO / "docs" / "ui"
DIST = SITE / "dist"
DOCS_OUT = DIST / "docs"

PH = "\u0000"  # 行内占位符前缀
CURRENT_BASE: pathlib.Path | None = None  # 当前正在渲染的 md（相对 docs/ui）
GH_BLOB = "https://github.com/lilyco-42/lilyco/blob/main/"

# ── 零依赖语法高亮（构建期完成，页面不跑 JS） ──────────────

RUST_KW = (
    r"\b(?:as|async|await|break|const|continue|crate|dyn|else|enum|extern|false|fn|for|if|"
    r"impl|in|let|loop|match|mod|move|mut|pub|ref|return|self|Self|static|struct|super|trait|"
    r"true|type|unsafe|use|where|while|Some|None|Ok|Err|String|Vec|Option|Result)\b"
)


def highlight(code: str, lang: str) -> str:
    lang = (lang or "").strip().lower()
    if lang in ("rust", "rs"):
        pat = re.compile(
            r"(?P<c>//[^\n]*)"
            r'|(?P<s>r#?"(?:[^"\\]|\\.)*"#?)'
            r"|(?P<a>#!?\[[^\n]*?\])"
            r"|(?P<m>\b[a-z_][a-z0-9_]*!)"
            r"|(?P<k>" + RUST_KW + r")"
            r"|(?P<n>\b(?:0x[0-9a-fA-F_]+|\d[\d_]*\.?\d*)\b)",
            re.M,
        )
        cls = {"c": "tok-c", "s": "tok-s", "a": "tok-a", "m": "tok-m", "k": "tok-k", "n": "tok-n"}
    elif lang == "toml":
        pat = re.compile(
            r"(?P<c>#[^\n]*)"
            r"|(?P<h>^[ \t]*\[[^\]\n]+\])"
            r'|(?P<s>"(?:[^"\\]|\\.)*"|\'[^\']*\')'
            r"|(?P<k>^[ \t]*[A-Za-z0-9_.-]+(?=[ \t]*=))"
            r"|(?P<n>\b\d[\d_.]*\b)",
            re.M,
        )
        cls = {"c": "tok-c", "h": "tok-k", "s": "tok-s", "k": "tok-a", "n": "tok-n"}
    elif lang == "json":
        pat = re.compile(r'(?P<s>"(?:[^"\\]|\\.)*")|(?P<k>\b(?:true|false|null)\b)|(?P<n>-?\b\d[\d.eE+-]*\b)')
        cls = {"s": "tok-s", "k": "tok-k", "n": "tok-n"}
    elif lang in ("bash", "sh", "shell", "console"):
        pat = re.compile(
            r"(?P<c>#[^\n]*)"
            r'|(?P<s>"(?:[^"\\]|\\.)*"|\'[^\']*\')'
            r"|(?P<k>\b(?:cargo|cd|git|gh|curl|python3|node|npm|echo|export|mkdir|test|grep|find|strip|filecount)\b)"
            r"|(?P<n>[ \t]--?[A-Za-z][\w-]*)"
        )
        cls = {"c": "tok-c", "s": "tok-s", "k": "tok-k", "n": "tok-n"}
    else:
        return html.escape(code)

    out: list[str] = []
    pos = 0
    for m in pat.finditer(code):
        out.append(html.escape(code[pos:m.start()]))
        kind = next(k for k in cls if m.groupdict().get(k))
        out.append(f'<span class="{cls[kind]}">{html.escape(m.group(kind))}</span>')
        pos = m.end()
    out.append(html.escape(code[pos:]))
    return "".join(out)


# ── 行内渲染 ────────────────────────────────────────────────

def inline(text: str) -> str:
    text = text.replace("\\|", PH + "PIPE")
    codes: list[str] = []

    def keep_code(m: re.Match[str]) -> str:
        codes.append(f"<code>{html.escape(m.group(1))}</code>")
        return f"{PH}CODE{len(codes) - 1}"

    text = re.sub(r"`([^`]+)`", keep_code, text)
    text = html.escape(text, quote=False)

    def link(m: re.Match[str]) -> str:
        label, url = m.group(1), m.group(2)
        if url.startswith(("http://", "https://", "mailto:")):
            return f'<a href="{html.escape(url)}">{label}</a>'
        if url.endswith(".md") or ".md#" in url:
            path_part, _, frag = url.partition("#")
            resolved = (SRC / (CURRENT_BASE or pathlib.Path("."))).parent.joinpath(path_part).resolve()
            inside = resolved == SRC.resolve() or SRC.resolve() in resolved.parents
            if not inside:
                try:
                    rel = resolved.relative_to(REPO).as_posix()
                except ValueError:
                    rel = path_part
                return f'<a href="{GH_BLOB}{html.escape(rel)}">{label}</a>'
            url = path_part.replace(".md", ".html") + (f"#{frag}" if frag else "")
        return f'<a href="{html.escape(url)}">{label}</a>'

    text = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", link, text)
    text = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", text)
    text = re.sub(r"(?<![\w*])\*([^*\n]+)\*(?![\w*])", r"<em>\1</em>", text)

    for i, c in enumerate(codes):
        text = text.replace(f"{PH}CODE{i}", c)
    return text.replace(PH + "PIPE", "|")


# ── 块级渲染 ────────────────────────────────────────────────

def split_row(line: str) -> list[str]:
    line = line.strip().strip("|")
    cells = re.split(r"(?<!\\)\|", line)
    return [c.strip().replace("\\|", "|") for c in cells]


def is_table_sep(line: str) -> bool:
    return bool(re.match(r"^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)+\|?\s*$", line))


def md_to_html(text: str) -> str:
    lines = text.splitlines()
    out: list[str] = []
    i = 0
    n = len(lines)

    while i < n:
        line = lines[i]

        if line.startswith("```"):
            lang = line[3:].strip()
            i += 1
            buf: list[str] = []
            while i < n and not lines[i].startswith("```"):
                buf.append(lines[i])
                i += 1
            i += 1
            cls = f' class="language-{html.escape(lang)}"' if lang else ""
            out.append(f"<pre><code{cls}>" + highlight("\n".join(buf), lang) + "</code></pre>")
            continue

        if not line.strip():
            i += 1
            continue

        if line.startswith("---") and line.strip("-").strip() == "":
            out.append("<hr>")
            i += 1
            continue

        m = re.match(r"^(#{1,6})\s+(.*)$", line)
        if m:
            level = len(m.group(1))
            out.append(f"<h{level}>{inline(m.group(2))}</h{level}>")
            i += 1
            continue

        if line.lstrip().startswith("|") and i + 1 < n and is_table_sep(lines[i + 1]):
            head = split_row(line)
            i += 2
            rows: list[list[str]] = []
            while i < n and lines[i].lstrip().startswith("|"):
                rows.append(split_row(lines[i]))
                i += 1
            html_rows = ["<table><thead><tr>" + "".join(f"<th>{inline(c)}</th>" for c in head) + "</tr></thead><tbody>"]
            for row in rows:
                html_rows.append("<tr>" + "".join(f"<td>{inline(c)}</td>" for c in row) + "</tr>")
            html_rows.append("</tbody></table>")
            out.append("".join(html_rows))
            continue

        if line.lstrip().startswith(">"):
            buf = []
            while i < n and lines[i].lstrip().startswith(">"):
                buf.append(lines[i].lstrip()[1:].strip())
                i += 1
            out.append(f"<blockquote>{inline(' '.join(buf))}</blockquote>")
            continue

        if re.match(r"^\s*[-*+]\s+", line):
            items = []
            while i < n and re.match(r"^\s*[-*+]\s+", lines[i]):
                items.append(re.sub(r"^\s*[-*+]\s+", "", lines[i]))
                i += 1
            out.append("<ul>" + "".join(f"<li>{inline(x)}</li>" for x in items) + "</ul>")
            continue

        if re.match(r"^\s*\d+\.\s+", line):
            items = []
            while i < n and re.match(r"^\s*\d+\.\s+", lines[i]):
                items.append(re.sub(r"^\s*\d+\.\s+", "", lines[i]))
                i += 1
            out.append("<ol>" + "".join(f"<li>{inline(x)}</li>" for x in items) + "</ol>")
            continue

        buf = [line.strip()]
        i += 1
        while i < n and lines[i].strip() and not re.match(
            r"^(#{1,6}\s|```|\s*[-*+]\s|\s*\d+\.\s|>|\||---)", lines[i]
        ):
            buf.append(lines[i].strip())
            i += 1
        out.append(f"<p>{inline(' '.join(buf))}</p>")

    return "\n".join(out)


# ── 页面元数据与导航 ────────────────────────────────────────

def title_of(path: pathlib.Path) -> str:
    for line in path.read_text(encoding="utf-8").splitlines():
        m = re.match(r"^#\s+(.*)$", line)
        if m:
            return re.sub(r"[*`]", "", m.group(1)).strip()
    return path.stem


def build_nav(pages: list[pathlib.Path], current: pathlib.Path) -> str:
    groups: list[tuple[str, list[pathlib.Path]]] = [
        ("总览", [p for p in pages if p.parent == SRC]),
        ("教程", [p for p in pages if p.parent.name == "tutorial"]),
        ("组件与设计", [p for p in pages if p.parent.name == "components"]),
        ("附录", [p for p in pages if p.parent.name == "appendix"]),
    ]
    cur_rel = current.relative_to(SRC)
    root = pathlib.Path(*([".."] * (len(cur_rel.parts) - 1))) if len(cur_rel.parts) > 1 else pathlib.Path(".")

    def href(p: pathlib.Path) -> str:
        rel = p.relative_to(SRC).with_suffix(".html")
        return (root / rel).as_posix()

    parts = []
    for name, group in groups:
        if not group:
            continue
        items = []
        for p in group:
            active = ' class="active"' if p == current else ""
            items.append(f'<a href="{href(p)}"{active}>{html.escape(title_of(p))}</a>')
        parts.append(f'<div class="grp"><div class="grp-title">{name}</div>{"".join(items)}</div>')
    return "".join(parts)


def main() -> int:
    if not SRC.is_dir():
        print(f"缺少源目录: {SRC}", file=sys.stderr)
        return 1

    if DIST.exists():
        shutil.rmtree(DIST)
    DOCS_OUT.mkdir(parents=True)

    pages = sorted(SRC.rglob("*.md"))
    template = (SITE / "template.html").read_text(encoding="utf-8")

    for page in pages:
        global CURRENT_BASE
        rel = page.relative_to(SRC)
        CURRENT_BASE = rel
        depth = len(rel.parts) - 1
        root = "/".join([".."] * depth) if depth else "."
        body = md_to_html(page.read_text(encoding="utf-8"))
        out_html = (
            template.replace("{{TITLE}}", html.escape(title_of(page)))
            .replace("{{ROOT}}", root)
            .replace("{{NAV}}", build_nav(pages, page))
            .replace("{{BODY}}", body)
        )
        target = DOCS_OUT / rel.with_suffix(".html")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(out_html, encoding="utf-8")

    shutil.copytree(SITE / "assets", DIST / "assets")
    shutil.copy2(SITE / "index.html", DIST / "index.html")
    (DIST / ".nojekyll").write_text("", encoding="utf-8")

    print(f"构建完成：{len(pages)} 篇文档 → {DIST}")
    return 0


if __name__ == "__main__":
    sys.exit(main())