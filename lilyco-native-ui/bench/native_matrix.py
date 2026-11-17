#!/usr/bin/env python3
"""NSH/GTK 原生全变体功能矩阵、状态交互与窗口截图证据。"""
import json, os, re, subprocess, sys, time
from pathlib import Path
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import native_bench as nb
BIN = os.environ.get("NATIVE_BIN", "/home/lilyco/.cache/lilyco-build/target/release/lilyco-native-ui")
SPEC = str(HERE / "fixtures/all-13-spec.json")
OUT = Path(os.environ.get("MATRIX_OUT", str(HERE / "results/native-all13-2026-11-15.json")))
TREE = str(HERE / "atspi_tree.py")
X11 = str(HERE / "bin/x11bench")
SHOT = Path("/tmp/native-all13-window.png")
LOG = Path("/tmp/native-all13-matrix.log")

def tree(mode, pid, tag, extra=None):
    out = Path(f"/tmp/native-matrix-{tag}.json")
    cmd = [sys.executable, "-I", TREE, mode, str(pid), str(out)] + list(extra or [])
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=40)
    if r.returncode: raise RuntimeError(f"tree {tag} rc={r.returncode} {r.stderr[:300]}")
    return json.loads(out.read_text()), r.stdout.strip(), out

def label(d, prefix):
    return next((str(n.get("name") or "") for n in d["nodes"] if str(n.get("name") or "").startswith(prefix)), "")

def has(d, pred):
    return any(pred(n) for n in d["nodes"])

def name(n): return str(n.get("name") or "")
def text(n): return str(n.get("text") or "")
def role(n): return str(n.get("role") or "")
def acts(n): return list(n.get("actions") or [])

def matrix(d):
    nodes = d["nodes"]
    ti = next((i for i, n in enumerate(nodes) if role(n) == "text" and text(n) == "hello"), -1)
    panels = nodes[ti + 1:ti + 3] if ti >= 0 else []
    return {
        "标题栏": has(d, lambda n: role(n) == "frame" and "Lilyco 原生全变体烟测" in name(n)),
        "标题": has(d, lambda n: "十三变体" in name(n)),
        "文本": has(d, lambda n: "纯 Rust / NSH GTK 原生视图" in name(n)),
        "输入框": has(d, lambda n: name(n) == "名称" and role(n) == "label") and has(d, lambda n: role(n) == "panel" and name(n) == ""),
        "数字框": has(d, lambda n: name(n) == "数量" and role(n) == "label") and has(d, lambda n: role(n) == "panel" and name(n) == ""),
        "选择框": has(d, lambda n: role(n) == "combo box" and "nsh.activate" in acts(n)) and has(d, lambda n: name(n) == "png" and role(n) == "label"),
        "复选框": has(d, lambda n: role(n) == "check box" and name(n) == "启用" and "nsh.activate" in acts(n)),
        "文本域": has(d, lambda n: role(n) == "text" and text(n) == "hello" and n.get("editable") is True),
        "图片/分隔线面板": len(panels) >= 2 and all(role(n) == "panel" and name(n) == "" for n in panels),
        "进度条": has(d, lambda n: role(n) == "progress bar" and abs(float(n.get("value") or -1) - 42.0) < 0.01),
        "链接": has(d, lambda n: role(n) == "button" and name(n) == "NSH 官网" and "nsh.activate" in acts(n)),
        "计算器": has(d, lambda n: "计算器 · 科学" in name(n)) and has(d, lambda n: role(n) == "button" and name(n) == "计算"),
        "按钮": has(d, lambda n: role(n) == "button" and name(n) == "执行" and "nsh.activate" in acts(n)),
        "状态栏初始": "状态: （空）" in label(d, "状态:"),
    }

def main():
    base = nb.xids()
    f = LOG.open("w")
    env = dict(os.environ); env.update({"GDK_BACKEND": "x11", "LILYCO_BENCH": "1"})
    p = subprocess.Popen([BIN, SPEC], env=env, stdout=f, stderr=subprocess.STDOUT)
    try:
        wid = nb.wait_new(base, pid=p.pid)
        time.sleep(1.4)
        if wid: subprocess.run([X11, "activate", wid], capture_output=True, text=True, timeout=10)
        d0, s0, _ = tree("dump", p.pid, "initial")
        mat = matrix(d0)
        # 文本域写入 + 计算器按钮：验证 values 信号与结果文本更新。
        d1, s1, _ = tree("run", p.pid, "calc", ["", "matrix-ok", "计算"])
        state1, result1 = label(d1, "状态:"), label(d1, "结果:")
        # 复选框：验证 nsh.activate 触发 on_change 写回 values。
        d2, s2, _ = tree("run", p.pid, "checkbox", ["__NO_SUCH__", "", "启用"])
        state2 = label(d2, "状态:")
        # 业务按钮：验证 notice 信号与动作文字。
        d3, s3, _ = tree("run", p.pid, "button", ["__NO_SUCH__", "", "执行"])
        notice = label(d3, "动作 run")
        shot = {"path": str(SHOT), "captured": False}
        if wid:
            r = subprocess.run(["import", "-window", wid, str(SHOT)], capture_output=True, text=True, timeout=20)
            if r.returncode == 0 and SHOT.exists():
                try:
                    from PIL import Image
                    im = Image.open(SHOT).convert("RGB")
                    shot.update({"captured": True, "bytes": SHOT.stat().st_size,
                                 "width": im.width, "height": im.height,
                                 "unique_colors": len(im.getcolors(maxcolors=2000000) or [])})
                except Exception as e:
                    shot["error"] = str(e)
        time.sleep(0.3)
    finally:
        p.terminate()
        try: p.wait(timeout=3)
        except subprocess.TimeoutExpired: p.kill(); p.wait()
        f.close()
    log = LOG.read_text(errors="replace")
    interactions = {
        "textarea_and_calculator": {
            "events": d1["events"], "state": state1, "result": result1,
            "pass": any(e.get("kind") == "set_text" and e.get("ok") for e in d1["events"])
                    and any(e.get("kind") == "activate" and e.get("ok") for e in d1["events"])
                    and "matrix-ok" in state1 and "v_calc=" in state1 and "表达式无效" in result1,
            "note": "AT-SPI 不暴露计算器表达式 entry；此处按空表达式验证错误路径与状态写回，有效四则运算由 Rust 单测覆盖。",
        },
        "checkbox": {"events": d2["events"], "state": state2,
                     "pass": any(e.get("kind") == "activate" and e.get("ok") for e in d2["events"]) and "v_check=" in state2},
        "button_notice": {"events": d3["events"], "notice": notice,
                          "pass": bool(notice) and "动作 run 已触发" in notice and "v_textarea=matrix-ok" in notice},
    }
    result = {
        "backend": "NSH/GTK4 (Rust AppSource::Rust, no WebView/QuickJS)",
        "binary": BIN, "spec": SPEC, "pid": p.pid, "window_id": wid,
        "tree_nodes": d0["node_count"],
        "matrix": mat, "matrixPass": sum(1 for v in mat.values() if v), "matrixTotal": len(mat),
        "tree_stdout": [s0, s1, s2, s3],
        "interactions": interactions, "screenshot": shot,
        "log": {
            "press_markers": re.findall(r"LILYCO_NATIVE_PRESS (\d+) (\S+)", log),
            "perf_events": len(re.findall(r"nsh::perf\] event ", log)),
            "press_events": len(re.findall(r"nsh::perf\] event Press ", log)),
            "change_events": len(re.findall(r"nsh::perf\] event ChangeText", log)),
            "warnings": len(re.findall(r"Gtk-WARNING", log)),
            "criticals": len(re.findall(r"(?:GLib|Gtk)-CRITICAL", log)),
            "panics": len(re.findall(r"panicked", log, re.I)),
        },
    }
    OUT.parent.mkdir(exist_ok=True)
    OUT.write_text(json.dumps(result, ensure_ascii=False, indent=2))
    print(json.dumps({k: v for k, v in result.items() if k != "tree_stdout"}, ensure_ascii=False, indent=2))
if __name__ == "__main__":
    main()
