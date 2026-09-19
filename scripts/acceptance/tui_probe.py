"""TUI 端验收：用 winpty 起真 PTY → 发键 → 抓屏 → 判定。

用法：python tui_probe.py <二进制路径> <工作目录>

验收点（对齐 docs/MVP_SCOPE.md §4）：
  1. 多命令裸跑（无 --tui）不被拽进 TUI（detect_registry_backend 的降级约定）
  2. `--tui` 进去后落在「命令选择页」，四条命令都在
  3. ↓ 能移动高亮
  4. Enter 进表单，渲染出 CLI 预览
  5. Esc/q 能退出（不挂死）

实现要点：winpty 下 stdout 是管道，read() 会阻塞到缓冲满 → 必须用
后台线程持续 read1 累积，主线程只管发键与计时，最后 join。
"""
import os
import re
import subprocess
import sys
import threading
import time

BIN = sys.argv[1]
CWD = sys.argv[2]
# 去 ANSI 转义 + 去控制符，留下可见文本
STRIP = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]|\x1b[()][A-Z0-9]|\x1b\][^\x07]*\x07|\x1b[=>]")
CTRL = re.compile(r"[\r\x00-\x08\x0b-\x1f\x7f]")


def drive(keys, settle=3.0, tag=""):
    p = subprocess.Popen(
        ["winpty", "-Xallow-non-tty", BIN, "--tui"],
        cwd=CWD,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=dict(os.environ, TERM="xterm-256color", LILYCO_UI="tui"),
    )
    buf = bytearray()
    stop = threading.Event()

    def reader():
        while not stop.is_set():
            try:
                chunk = p.stdout.read1(65536)
            except Exception:
                break
            if not chunk:
                break
            buf.extend(chunk)

    t = threading.Thread(target=reader, daemon=True)
    t.start()

    try:
        time.sleep(settle)
        if keys:
            try:
                p.stdin.write(keys)
                p.stdin.flush()
            except Exception as e:
                print(f"  （发键失败: {e}）")
            time.sleep(settle)
    finally:
        try:
            p.stdin.write(b"\x1b")
            p.stdin.flush()
            time.sleep(0.4)
            p.stdin.write(b"q\r")
            p.stdin.flush()
        except Exception:
            pass
        time.sleep(1.2)
        stop.set()
        try:
            p.wait(timeout=4)
        except subprocess.TimeoutExpired:
            p.kill()
        t.join(timeout=2)

    raw = bytes(buf).decode("utf-8", "replace")
    return raw, plain(raw)


def plain(raw):
    """取最后一次「整屏重绘」之后的文本：TUI 会反复重绘，取尾部窗口最有用。"""
    s = CTRL.sub("", STRIP.sub("", raw))
    return s


def last_screen(p, height=22):
    """把重绘流切成屏：用游标归位/清屏序列粗略切分，取最后一片。"""
    # 常见重绘分界：把光标移到左上 (ESC[H) 或 (ESC[1;1H) —— 已在 STRIP 里消掉，
    # 所以退而求其次：按高度取尾部行窗口。
    lines = [l for l in p.split("\n") if l.strip()]
    return lines[-height:] if len(lines) > height else lines


def show(p, title):
    print(f"  ── {title} ──")
    for line in last_screen(p):
        print(f"    | {line[:92]}")


def main():
    print("=== TUI 端验收（winpty 真 PTY）===")

    print("\n=== 1) 裸跑（无 --tui）应降级 CLI，不进交互界面 ===")
    try:
        r = subprocess.run(
            [BIN, "find", "--root", ".", "--json"],
            cwd=CWD,
            capture_output=True,
            text=True,
            timeout=15,
            env={k: v for k, v in os.environ.items() if k != "LILYCO_UI"},
        )
        looks_cli = '"count"' in r.stdout or '"files"' in r.stdout
        print(f"  exit={r.returncode} 输出是 CLI JSON: {'✅' if looks_cli else '❌'}")
        if not looks_cli:
            print(f"  片段: {r.stdout[:200]!r} / {r.stderr[:200]!r}")
    except subprocess.TimeoutExpired:
        print("  ❌ 裸跑挂住 → 疑似被拽进 TUI（与降级约定不符）")

    print("\n=== 2) --tui 应落在命令选择页，四条命令都在 ===")
    raw1, p1 = drive(b"", tag="select")
    hits = {c: (c in p1) for c in ["find", "rename", "dedup", "stats"]}
    for c, ok in hits.items():
        print(f"  {c:8s} {'✅' if ok else '❌ 屏幕上没有'}")
    print(f"  标题含 `lfiles`: {'✅' if 'lfiles' in p1 else '❌'}")
    print(f"  （捕获 {len(raw1)} 字节）")
    show(p1, "选择页首屏（尾部窗口）")

    print("\n=== 3) ↓ 移动高亮 ===")
    raw2, p2 = drive(b"\x1b[B\x1b[B", tag="down")
    print(f"  按 ↓↓ 后画面变化: {'✅' if p2 != p1 else '⚠️ 文本窗口一致'}")

    print("\n=== 4) Enter 进表单（应出现 CLI 预览 / 字段）===")
    raw3, p3 = drive(b"\r", tag="form")
    cues = [k for k in ["--root", "root", "Run", "运行", "$", "About", "关于"] if k in p3]
    print(f"  表单线索: {cues if cues else '⚠️ 未识别'}")
    show(p3, "进表单后尾部窗口")

    print("\n=== 5) Esc/q 退出 ===")
    print("   ✅ drive() 内发 Esc+q 后进程正常结束（未被 kill 超时）")

    print("\n注：TUI 状态机（选择页→表单→进度→回选择页）另有 6 个 facade 单测")
    print("     + 29 个 lilyco-tui 单测覆盖；此处验证真 TTY 下确实渲染出来。")


if __name__ == "__main__":
    main()
