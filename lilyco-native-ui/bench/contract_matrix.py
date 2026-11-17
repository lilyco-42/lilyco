#!/usr/bin/env python3
"""契约层与异常用例：/api/validate、/api/render、示例路由、HTML 修复点。"""
import json, sys, urllib.error, urllib.request
from pathlib import Path
HERE = Path(__file__).resolve().parent
OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("/tmp/contract-matrix.json")
BASE = "http://127.0.0.1:9090"
ALL13 = json.loads((HERE / "fixtures/all-13-spec.json").read_text())

def req(method, path, payload=None, raw=None):
    data = None if payload is None and raw is None else (raw if raw is not None else json.dumps(payload, ensure_ascii=False)).encode()
    r = urllib.request.Request(BASE + path, data=data, method=method,
                               headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(r, timeout=15) as resp:
            body = resp.read().decode(errors="replace")
            return {"status": resp.status, "body": body}
    except urllib.error.HTTPError as e:
        return {"status": e.code, "body": e.read().decode(errors="replace")}

def spec(title="T", elements=None):
    return {"窗口": {"标题": title, "大小": "大", "元素": elements if elements is not None else [{"类型": "文本", "内容": "x"}]}}

def check(case, expect, fn):
    try:
        ok = bool(fn())
    except Exception as e:
        ok = False
        case["error"] = str(e)
    case["pass"] = ok
    case["expect"] = expect
    return case

cases = []
r = req("GET", "/api/example"); b = json.loads(r["body"])
cases.append(check({"name": "example_default", "status": r["status"]}, "10 元素合法示例",
                   lambda: r["status"] == 200 and b["窗口"]["标题"] == "图片压缩工具" and len(b["窗口"]["元素"]) == 10))
r = req("GET", "/api/example/calculator"); b = json.loads(r["body"])
cases.append(check({"name": "example_calculator", "status": r["status"]}, "计算器示例路由已挂载",
                   lambda: r["status"] == 200 and any(e["类型"] == "计算器" for e in b["窗口"]["元素"])))
r = req("POST", "/api/validate", ALL13); b = json.loads(r["body"])
cases.append(check({"name": "validate_all13", "status": r["status"], "body": b}, "valid=true, errors=[]",
                   lambda: r["status"] == 200 and b["valid"] is True and b["errors"] == []))
r = req("POST", "/api/render", ALL13)
h = r["body"]
cases.append(check({"name": "render_all13", "status": r["status"], "bytes": len(h.encode())}, "固定 SRI + classic preset + 13 变体 case",
                   lambda: r["status"] == 200 and h.count('integrity="sha384-') == 3
                   and "lilyco-react" in h and 'runtime: "classic"' in h and "case '计算器'" in h
                   and "case '按钮'" in h and 'data-presets="lilyco-react"' in h))
cases.append(check({"name": "render_window_class_mapped", "class": next((l.strip() for l in h.splitlines() if "className={'window" in l), "")},
                   "回归：窗口大小经 SIZE 映射成英文 CSS class（large/small/medium/fullscreen）",
                   lambda: "SIZE[w.大小]" in h and "大: 'large'" in h and "'window ' + w.大小" not in h))
r = req("POST", "/api/validate", raw="not json")
cases.append(check({"name": "invalid_json", "status": r["status"], "body": r["body"]},
                   "HTTP 400 / axum JSON extractor 拒绝非法请求体",
                   lambda: r["status"] == 400 and "Failed to parse" in r["body"]))
r = req("POST", "/api/render", raw="not json")
cases.append(check({"name": "render_invalid_json", "status": r["status"]},
                   "HTTP 400 / 非法 JSON 不进入生成器",
                   lambda: r["status"] == 400))
for name, payload, token in [
    ("empty_title", spec(title=""), "标题不能为空"),
    ("empty_elements", spec(elements=[]), "窗口元素列表为空"),
    ("duplicate_vars", spec(elements=[{"类型": "输入框", "标签": "a", "变量": "dup"}, {"类型": "输入框", "标签": "b", "变量": "dup"}]), "重复"),
    ("empty_select", spec(elements=[{"类型": "选择框", "标签": "s", "变量": "v", "选项": []}]), "选项列表不能为空"),
    ("bad_heading_level", spec(elements=[{"类型": "标题", "内容": "h", "级别": 9}]), "标题级别必须在 1~6"),
]:
    r = req("POST", "/api/validate", payload); b = json.loads(r["body"])
    cases.append(check({"name": name, "status": r["status"], "body": b}, token,
                       lambda b=b, token=token: b["valid"] is False and any(token in e for e in b["errors"])))
r = req("POST", "/api/validate", spec(elements=[{"类型": "未知类型", "内容": "x"}])); b = json.loads(r["body"])
cases.append(check({"name": "unknown_variant_rejected", "status": r["status"], "body": b},
                   "serde tag 未知变体在契约层拒绝",
                   lambda: b["valid"] is False and "JSON 解析失败" in b["errors"][0]))
r = req("POST", "/api/render", spec(title=""))
cases.append(check({"name": "render_validation_gate", "status": r["status"]}, "校验失败不生成 HTML / 400",
                   lambda: r["status"] == 400))
out = {"base": BASE, "total": len(cases), "passed": sum(1 for c in cases if c.get("pass")), "cases": cases}
OUT.write_text(json.dumps(out, ensure_ascii=False, indent=2))
print(json.dumps({"total": out["total"], "passed": out["passed"], "failed": [c["name"] for c in cases if not c.get("pass")]}, ensure_ascii=False))
for c in cases:
    print("CASE", c["name"], "PASS" if c.get("pass") else "FAIL", c.get("expect"))
