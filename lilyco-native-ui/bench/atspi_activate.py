import ctypes, sys, time
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
do_action = bind(atspi, "atspi_action_do_action", integer, [pointer, integer, pointer])
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
def find_named_action(node, target_name, depth=0):
    if depth > 40:
        return None
    name = string(get_name, node)
    if name == target_name:
        act = get_action(node)
        if act:
            n = action_count(act, None)
            if n > 0 and string(action_name, act, 0) == "nsh.activate":
                return act
            unref(act)
    cnt = child_count(node, None)
    for i in range(max(cnt, 0)):
        child = child_at(node, i, None)
        if child:
            try:
                found = find_named_action(child, target_name, depth + 1)
                if found:
                    return found
            finally:
                unref(child)
    return None
def main():
    pid = int(sys.argv[1])
    target_name = sys.argv[2] if len(sys.argv) > 2 else "开始压缩"
    if initialize() != 0:
        print("ATSPI_INIT_FAIL", flush=True)
        sys.exit(2)
    root = desktop(0)
    if not root:
        print("NO_DESKTOP", flush=True)
        sys.exit(2)
    found = None
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
                found = find_named_action(app, target_name)
                if found:
                    break
            finally:
                unref(app)
        if not found:
            time.sleep(0.05)
    if not found:
        print("NO_NAMED_ACTION_FOUND pid=" + str(pid) + " name=" + repr(target_name), flush=True)
        sys.exit(3)
    error = pointer()
    t0 = time.time_ns() // 1000
    accepted = bool(do_action(found, 0, ctypes.byref(error)))
    t1 = time.time_ns() // 1000
    print("ATSPI_ACTIVATE name=" + repr(target_name) + " start_epoch_us=" + str(t0) + " end_epoch_us=" + str(t1) + " do_action_us=" + str(t1 - t0) + " accepted=" + str(accepted) + " pid=" + str(pid), flush=True)
    unref(found)
    unref(root)
main()
