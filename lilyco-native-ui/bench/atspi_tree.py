#!/usr/bin/env python3
"""导出 NSH 原生窗口的 AT-SPI 树；可选注入文本与激活动作。

用法：
  python3 atspi_tree.py dump <pid> <out.json>
  python3 atspi_tree.py run <pid> <out.json> <set_name> <set_value> <activate_name>
"""
import ctypes, json, sys, time
atspi = ctypes.CDLL("libatspi.so.0")
gobject = ctypes.CDLL("libgobject-2.0.so.0")
glib = ctypes.CDLL("libglib-2.0.so.0")
pointer = ctypes.c_void_p
integer = ctypes.c_int
boolean = ctypes.c_int
double = ctypes.c_double
cstr = ctypes.c_char_p
def bind(lib, name, result, args):
    f = getattr(lib, name); f.restype = result; f.argtypes = args; return f
initialize = bind(atspi, "atspi_init", integer, [])
desktop = bind(atspi, "atspi_get_desktop", pointer, [integer])
child_count = bind(atspi, "atspi_accessible_get_child_count", integer, [pointer, pointer])
child_at = bind(atspi, "atspi_accessible_get_child_at_index", pointer, [pointer, integer, pointer])
process_id = bind(atspi, "atspi_accessible_get_process_id", ctypes.c_uint, [pointer, pointer])
get_name = bind(atspi, "atspi_accessible_get_name", pointer, [pointer, pointer])
get_role_name = bind(atspi, "atspi_accessible_get_role_name", pointer, [pointer, pointer])
get_action = bind(atspi, "atspi_accessible_get_action_iface", pointer, [pointer])
action_count = bind(atspi, "atspi_action_get_n_actions", integer, [pointer, pointer])
action_name = bind(atspi, "atspi_action_get_action_name", pointer, [pointer, integer, pointer])
do_action = bind(atspi, "atspi_action_do_action", boolean, [pointer, integer, pointer])
get_text = bind(atspi, "atspi_accessible_get_text_iface", pointer, [pointer])
text_get = bind(atspi, "atspi_text_get_text", pointer, [pointer, integer, integer, pointer])
get_editable = bind(atspi, "atspi_accessible_get_editable_text_iface", pointer, [pointer])
editable_set = bind(atspi, "atspi_editable_text_set_text_contents", boolean, [pointer, cstr, pointer])
get_value = bind(atspi, "atspi_accessible_get_value_iface", pointer, [pointer])
value_current = bind(atspi, "atspi_value_get_current_value", double, [pointer, pointer])
unref = bind(gobject, "g_object_unref", None, [pointer])
free = bind(glib, "g_free", None, [pointer])
def string(fn, *args):
    v = fn(*args, None)
    if not v: return ""
    try: return ctypes.string_at(v).decode(errors="replace")
    finally: free(v)
def node_info(node, depth):
    d = {"depth": depth, "name": string(get_name, node), "role": string(get_role_name, node)}
    act = get_action(node)
    if act:
        try:
            d["actions"] = [string(action_name, act, i) for i in range(max(action_count(act, None), 0))]
            d["action_ptr"] = int(act) if act else None
        finally: unref(act)
    t = get_text(node)
    if t:
        try: d["text"] = string(text_get, t, 0, -1)
        finally: unref(t)
    e = get_editable(node)
    if e:
        d["editable"] = True
        d["editable_ptr"] = int(e)
        unref(e)
    v = get_value(node)
    if v:
        try: d["value"] = value_current(v, None)
        finally: unref(v)
    return d
def walk(node, depth, out):
    if depth > 40: return
    out.append(node_info(node, depth))
    for i in range(max(child_count(node, None), 0)):
        ch = child_at(node, i, None)
        if ch:
            try: walk(ch, depth + 1, out)
            finally: unref(ch)
def app_node(pid, deadline=8.0):
    root = desktop(0)
    end = time.monotonic() + deadline
    while time.monotonic() < end:
        for i in range(max(child_count(root, None), 0)):
            a = child_at(root, i, None)
            if not a: continue
            if process_id(a, None) == pid: return root, a
            unref(a)
        time.sleep(0.05)
    unref(root)
    raise SystemExit("NO_APP_FOR_PID " + str(pid))
def find(out, name, action=None):
    for x in out:
        if x.get("name") != name: continue
        if action and action not in x.get("actions", []): continue
        return x
    return None
def main():
    mode = sys.argv[1]; pid = int(sys.argv[2]); out_path = sys.argv[3]
    if initialize() != 0: raise SystemExit("ATSPI_INIT_FAIL")
    root, app = app_node(pid)
    events = []
    if mode == "run":
        set_name, set_value, activate_name = sys.argv[4:7]
        # 拿到可写接口：需要重新按深度遍历，保留指针仅在本次进程有效
        def visit(node, depth):
            if depth > 40: return
            info = node_info(node, depth)
            if info.get("name") == set_name and info.get("editable"):
                e = get_editable(node)
                ok = bool(editable_set(e, set_value.encode(), None)) if e else False
                events.append({"kind": "set_text", "name": set_name, "value": set_value, "ok": ok})
                if e: unref(e)
            for i in range(max(child_count(node, None), 0)):
                ch = child_at(node, i, None)
                if ch:
                    try: visit(ch, depth + 1)
                    finally: unref(ch)
        visit(app, 0)
        time.sleep(0.25)
        def activate(node, depth):
            if depth > 40: return
            info = node_info(node, depth)
            if info.get("name") == activate_name and "nsh.activate" in info.get("actions", []):
                a = get_action(node)
                ok = bool(do_action(a, 0, ctypes.byref(pointer()))) if a else False
                events.append({"kind": "activate", "name": activate_name, "ok": ok})
                if a: unref(a)
                return
            for i in range(max(child_count(node, None), 0)):
                ch = child_at(node, i, None)
                if ch:
                    try: activate(ch, depth + 1)
                    finally: unref(ch)
        activate(app, 0)
        time.sleep(0.45)
    out = []
    walk(app, 0, out)
    for x in out: x.pop("action_ptr", None); x.pop("editable_ptr", None)
    payload = {"pid": pid, "node_count": len(out), "events": events, "nodes": out}
    open(out_path, "w").write(json.dumps(payload, ensure_ascii=False, indent=2))
    print("ATSPI_TREE", json.dumps({"pid": pid, "nodes": len(out), "events": events}, ensure_ascii=False), flush=True)
    unref(app); unref(root)
main()
