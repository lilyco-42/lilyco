"""三条新 lane（3a5e 透视表 / 3bg 分页符 / 3bh 网格与默认制表位）的本地空跑。

lbin 那一侧由第二读者替身回答：这样跑出来的 DIFF 就是「钉的形状与数据对不上」那一类，
而 NameError / 下标越界 / 参数少一个 这类会在 exec 时直接炸出来 —— 都在几秒内，
而不是 60 分钟之后由 CI 告知。
"""
import json
import sys
import textwrap
from pathlib import Path

ROOT = Path("D:/Code/lilyco")
sys.path.insert(0, str(ROOT / "scripts/acceptance"))
import office_reader as R  # noqa: E402

FIXDIR = ROOT / "lilyco-binfmt/tests/fixtures/office"
NAMES = sorted(one.name for one in FIXDIR.iterdir() if one.is_file() and not one.name.startswith("."))
FILES = {}
for name in NAMES:
    try:
        FILES[name] = R.facts(FIXDIR / name)
    except Exception as bad:  # noqa: BLE001
        FILES[name] = {"error": str(bad)[:120]}

probe = (ROOT / "scripts/acceptance/office_probe.py").read_text(encoding="utf-8")
DIG_SRC = probe[probe.index("def dig(payload"):probe.index("def check(name")]

SLICES = []
for marker, stop_marker in (('print("=== 3a5e)', 'print("=== 3a5b 续)'),
                            ('print("=== 3bg)', 'print("=== 3bh)'),
                            ('print("=== 3bh)', 'print("=== 3bi)'),
                            ('print("=== 3bi)', 'print("=== 3c0)'),
                            ('print("=== 3c0)', 'print("=== 3c1)'),
                            ('print("=== 3c1)', 'print("=== 3c2)'),
                            ('print("=== 3c2)', 'print("=== 3c3)'),
                            ('print("=== 3c3)', 'print("=== 3c4)'),
                            ('print("=== 3c4)', '    failed = [one for one in RESULTS')):
    start = probe.index(marker)
    stop = probe.index(stop_marker, start)
    SLICES.append(textwrap.dedent(probe[start - 4:stop]))

RESULTS = []


def record(label, ok, detail=""):
    RESULTS.append((label, ok, detail))


def check(label, got, want, hint=""):
    ok = got == want
    record(label, ok, "" if ok else f"替身={json.dumps(got, ensure_ascii=False)[:300]} "
                                    f"钉的={json.dumps(want, ensure_ascii=False)[:300]}")


def fixture(name):
    return str(name)


class Table:
    def glob(self, pattern):
        tail = "." + pattern.split(".")[-1]
        return [Path(one) for one in NAMES if one.endswith(tail)]


def payload(cmd, name):
    book = FILES.get(name) or {}
    if cmd == "office-doc":
        had = book.get("ooxml")
        if isinstance(had, dict):
            return {"structure": had}
        return {"structure": (book.get("odt") or book.get("rtf") or {})}
    if cmd == "office-sheet":
        return book.get("ooxml") or book.get("ods") or book.get("biff") or {}
    return book.get("ooxml") or book.get("odf") or {}


def lbin(cmd, path, *flags):
    name = Path(str(path)).name
    had = payload(cmd, name)
    return had if isinstance(had, dict) else {}


def no_theme_key(cmd, name, key):
    # 与 probe 里那一份同一条口径：整份输出深度找键（office-doc 的账本住在 structure 下面）
    stack = [payload(cmd, name)]
    while stack:
        one = stack.pop()
        if isinstance(one, dict):
            if key in one:
                return True
            stack.extend(one.values())
        elif isinstance(one, list):
            stack.extend(one)
    return False


NS = {"json": json, "check": check, "record": record, "RESULTS": RESULTS, "lbin": lbin,
      "fixture": fixture, "FIXTURES": Table(), "files": FILES, "Path": Path,
      "no_theme_key": no_theme_key}
exec(DIG_SRC, NS)
for lane in SLICES:
    exec(lane, NS)

bad = [one for one in RESULTS if not one[1]]
print(f"=== 空跑合计 {len(RESULTS)} 项，不一致 {len(bad)} 项 ===")
for name, _, detail in bad[:14]:
    print(f"  DIFF {name[:60]}: {detail[:300]}")
