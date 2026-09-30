"""把两条静态闸门做实：
1) probe 里每一处 fixture("名字") 都必须是真的件名 —— 拼错的名字在 CI 里是 KeyError，
   会把整条 cross-check（60 分钟以上）连坐掉；
2) check() 的调用形状（3 或 4 个参数）。
"""
import ast
import io
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / 'scripts' / 'acceptance'
PROBE = SCRIPTS / 'office_probe.py'
READER = SCRIPTS / 'office_reader.py'
LEGACY = SCRIPTS / 'lyco_legacy.py'
FIXDIR = ROOT / 'lilyco-binfmt' / 'tests' / 'fixtures' / 'office'

LOW, HIGH = 3, 4


def texts():
    for path in (PROBE, READER, LEGACY):
        if path.exists():
            yield path, io.open(path, encoding='utf-8').read()


def main() -> int:
    have = set(one.name for one in FIXDIR.iterdir() if one.is_file())
    problems = []
    total = 0
    for path, text in texts():
        try:
            tree = ast.parse(text)
        except SyntaxError as why:
            print(f"FAIL {path.name} 语法不过：{why}")
            return 1
        for node in ast.walk(tree):
            if not isinstance(node, ast.Call):
                continue
            name = getattr(node.func, 'id', '')
            if name == 'check':
                total += 1
                if node.keywords or not (LOW <= len(node.args) <= HIGH):
                    head = node.args[0] if node.args else None
                    label = head.value[:44] if isinstance(head, ast.Constant) else '?'
                    problems.append((path.name, node.lineno,
                                     f'check() 给了 {len(node.args)} 个参数（要 3 或 4）：{label}'))
            elif name == 'fixture' and path is PROBE:
                total += 1
                if node.args and isinstance(node.args[0], ast.Constant) \
                        and isinstance(node.args[0].value, str):
                    want = node.args[0].value
                    if want not in have:
                        problems.append((path.name, node.lineno,
                                         f'fixture("{want[:40]}") 这个件名不在语料里'))
        # 循环里的件名表也要核：for name in ("a.docx", ...) 里拼错同样是 KeyError
        for node in ast.walk(tree):
            if path is PROBE and isinstance(node, (ast.Tuple, ast.List)):
                for one in node.elts:
                    if isinstance(one, ast.Constant) and isinstance(one.value, str) \
                            and '.' in one.value and one.value.rsplit('.', 1)[-1] in {
                                'docx', 'xlsx', 'pptx', 'odt', 'ods', 'odp', 'rtf', 'doc',
                                'xls', 'ppt', 'pdf', 'xlsm', 'docm', 'dotx'}:
                        total += 1
                        if ("*" not in one.value and "%" not in one.value and one.value.startswith(".") is False
                                and one.value not in have):
                            problems.append((path.name, one.lineno,
                                             f'字面量件名 "{one.value[:40]}" 不在语料里'))
    if problems:
        for where, lineno, why in problems:
            print(f"FAIL {where}:{lineno} {why}")
        return 1
    print(f"OK probe 闸门：{total} 处调用与件名都成立（check 参数形状 + fixture 件名都在语料里）")
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
