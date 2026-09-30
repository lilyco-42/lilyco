"""开工前的静态闸门：把 office_probe.py 里 `check(...)` 的调用形状先核一遍。

为什么要有这一份：cross-check 那一步要跑 60 分钟以上，而一条 `check()` 少给一个参数
（`TypeError: check() missing 1 required`）会让整步在**最后**才炸，前面所有账都白跑。
这一份只读源码、不读 fixture，几秒钟就能判。
"""
import ast
import io
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / 'scripts' / 'acceptance' / 'office_probe.py'
READER = ROOT / 'scripts' / 'acceptance' / 'office_reader.py'
LEGACY = ROOT / 'scripts' / 'acceptance' / 'lyco_legacy.py'

# 闸门一的口径来自 probe 自己的定义：check(name, got, want, hint="")
LOW = 3
HIGH = 4


def arity_problems(src: str, where: str):
    tree = ast.parse(src)
    bad = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        if getattr(node.func, 'id', '') != 'check':
            continue
        if node.keywords:
            bad.append((node.lineno, 'keyword', ''))
            continue
        if not (LOW <= len(node.args) <= HIGH):
            head = node.args[0] if node.args else None
            label = head.value[:48] if isinstance(head, ast.Constant) else '?'
            bad.append((node.lineno, len(node.args), label))
    return bad


def main() -> int:
    problems = []
    for path in (PROBE, READER, LEGACY):
        if not path.exists():
            continue
        text = io.open(path, encoding='utf-8').read()
        try:
            found = arity_problems(text, path.name)
        except SyntaxError as why:
            print(f"FAIL {path.name} 语法不过：{why}")
            return 1
        for lineno, count, label in found:
            problems.append((path.name, lineno, count, label))
    if problems:
        for name, lineno, count, label in problems:
            print(f"FAIL {name}:{lineno} check() 给了 {count} 个参数（要 {LOW} 或 {HIGH} 个）：{label}")
        return 1
    total = 0
    for path in (PROBE, READER, LEGACY):
        if path.exists():
            tree = ast.parse(io.open(path, encoding='utf-8').read())
            total += sum(1 for node in ast.walk(tree)
                         if isinstance(node, ast.Call) and getattr(node.func, 'id', '') == 'check')
    print(f"OK probe 闸门：{total} 处 check 调用参数齐全，三份脚本语法都过")
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
