#!/usr/bin/env python3
"""Enforce the project's nesting budget.

Two rules are checked for every Rust source file:

1. at most ``MAX_DEPTH`` nested blocks inside any ``fn`` body;
2. at most ``MAX_INDENT`` columns of leading indentation on any line.

Exit code 1 with a list of violations, 0 when everything complies.
"""

from __future__ import annotations

import sys
from pathlib import Path

MAX_DEPTH = 4
MAX_INDENT = 20


def strip_noise(line: str) -> str:
    """Drop comments and string literals so braces inside them are ignored."""
    out = []
    i = 0
    in_str = False
    while i < len(line):
        ch = line[i]
        if in_str:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                in_str = False
            out.append(" ")
            i += 1
            continue
        if ch == '"':
            in_str = True
            out.append(" ")
            i += 1
            continue
        if ch == "/" and i + 1 < len(line) and line[i + 1] == "/":
            break
        out.append(ch)
        i += 1
    return "".join(out)


def check_file(path: Path):
    problems = []
    depth = 0
    fn_bodies = []
    pending_fn = False
    for lineno, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        indent = len(raw) - len(raw.lstrip(" "))
        if indent > MAX_INDENT:
            problems.append((lineno, f"indent {indent} exceeds {MAX_INDENT}"))
        code = strip_noise(raw)
        if "fn " in code:
            pending_fn = True
        for ch in code:
            if ch == "{":
                depth += 1
                if pending_fn:
                    fn_bodies.append(depth)
                    pending_fn = False
            elif ch == "}":
                if fn_bodies and fn_bodies[-1] == depth:
                    fn_bodies.pop()
                depth -= 1
        if fn_bodies:
            nested = depth - fn_bodies[-1]
            if nested > MAX_DEPTH:
                problems.append((lineno, f"block depth {nested} exceeds {MAX_DEPTH}"))
    return problems


def main(argv):
    roots = [Path(a) for a in argv[1:]] or [Path("src")]
    files = []
    for root in roots:
        if root.is_dir():
            files.extend(sorted(root.rglob("*.rs")))
        elif root.suffix == ".rs":
            files.append(root)
    failures = 0
    for path in files:
        for lineno, message in check_file(path):
            failures += 1
            print(f"{path}:{lineno}: {message}")
    if failures:
        print(f"\n{failures} nesting violation(s) across {len(files)} file(s)")
        return 1
    print(f"nesting ok: {len(files)} file(s) within depth {MAX_DEPTH} and indent {MAX_INDENT}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
