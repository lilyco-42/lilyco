import ctypes, os, sys, time
atspi = ctypes.CDLL("libatspi.so.0")
gobject = ctypes.CDLL("libgobject-2.0.so.0")
glib = ctypes.CDLL("libglib-2.0.so.0")
pointer = ctypes.c_void_p
integer = ctypes.c_int
def bind(lib, name, result, args):
    f = getattr(lib, name)
    f.restype = result
    f.argtypes = args
    return f
initialize = bind(atspi, "atspi_init", integer, [])
desktop = bind(atspi, "atspi_get_desktop", pointer, [integer])
child_count = bind(atspi, "atspi_accessible_get_child_count", integer, [pointer, pointer])
child_at = bind(atspi, "atspi_accessible_get_child_at_index", pointer, [pointer, integer, pointer])
process_id = bind(atspi, "atspi_accessible_get_process_id", ctypes.c_uint, [pointer, pointer])
get_name = bind(atspi, "atspi_accessible_get_name", pointer, [pointer, pointer])
get_action = bind(atspi, "atspi_accessible_get_action_iface", pointer, [pointer])
action_count = bind(atspi, "atspi_action_get_n_actions", integer, [pointer, pointer])
action_name = bind(atspi, "atspi_action_get_action_name", pointer, [pointer, integer, pointer])
unref = bind(gobject, "g_object_unref", None, [pointer])
free = bind(glib, "g_free", None, [pointer])
def string(fn, *args):
    v = fn(*args, None)
    if not v:
        return ""
    try:
        return ctypes.string_at(v).decode()
    finally:
        free(v)
def walk(node, depth, out):
    if depth > 40:
        return
    name = string(get_name, node)
    act = get_action(node)
    if act:
        n = action_count(act, None)
        if n > 0:
            out.append((depth, name, string(action_name, act, 0), n))
        unref(act)
    cnt = child_count(node, None)
    for i in range(max(cnt, 0)):
        ch = child_at(node, i, None)
        if ch:
            try:
                walk(ch, depth + 1, out)
            finally:
                unref(ch)
def main():
    pid = int(sys.argv[1])
    if initialize() != 0:
        print("ATSPI_INIT_FAIL")
        sys.exit(2)
    root = desktop(0)
    found = []
    deadline = time.monotonic() + 6
    while not found and time.monotonic() < deadline:
        cnt = child_count(root, None)
        for i in range(max(cnt, 0)):
            app = child_at(root, i, None)
            if not app:
                continue
            try:
                if process_id(app, None) != pid:
                    continue
                items = []
                walk(app, 0, items)
                if items:
                    found = items
                    break
            finally:
                unref(app)
        if not found:
            time.sleep(0.05)
    print("ATSPI_SCAN pid=" + str(pid) + " count=" + str(len(found)), flush=True)
    for i, (d, n, a, c) in enumerate(found):
        print("ACTION i=" + str(i) + " depth=" + str(d) + " name=" + repr(n) + " action=" + repr(a) + " n=" + str(c), flush=True)
    unref(root)
main()
