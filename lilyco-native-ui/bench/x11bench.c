#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/extensions/XTest.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
static long long now_ms(void) {
  struct timespec ts; clock_gettime(CLOCK_REALTIME, &ts);
  return (long long)ts.tv_sec * 1000LL + ts.tv_nsec / 1000000LL;
}
static void query_pointer(Display *d, const char *tag) {
  Window root, child; int rx, ry, wx, wy; unsigned int mask;
  XQueryPointer(d, DefaultRootWindow(d), &root, &child, &rx, &ry, &wx, &wy, &mask);
  printf("%s root=%d,%d win=%d,%d child=0x%lx mask=%u\n", tag, rx, ry, wx, wy, child, mask);
  if (child) {
    XClassHint ch; char *name = NULL; Status s = XFetchName(d, child, &name);
    if (XGetClassHint(d, child, &ch)) {
      printf("%s_class res_name=%s res_class=%s name=%s fetch=%d\n", tag, ch.res_name ? ch.res_name : "-", ch.res_class ? ch.res_class : "-", name ? name : "-", s);
      if (ch.res_name) XFree(ch.res_name); if (ch.res_class) XFree(ch.res_class);
    } else {
      printf("%s_class class=none name=%s fetch=%d\n", tag, name ? name : "-", s);
    }
    if (name) XFree(name);
  }
  fflush(stdout);
}
int main(int argc, char **argv) {
  Display *d = XOpenDisplay(NULL);
  if (!d) { fprintf(stderr, "NO_DISPLAY\n"); return 2; }
  if (argc >= 3 && strcmp(argv[1], "geometry") == 0) {
    Window w = strtoul(argv[2], NULL, 0);
    XWindowAttributes a;
    if (!XGetWindowAttributes(d, w, &a)) { fprintf(stderr, "BAD_WINDOW\n"); return 3; }
    Window child; int rx, ry; unsigned int mask;
    XTranslateCoordinates(d, w, DefaultRootWindow(d), 0, 0, &rx, &ry, &child);
    printf("GEOM id=0x%lx x=%d y=%d w=%d h=%d root_x=%d root_y=%d mapped=%d\n", w, a.x, a.y, a.width, a.height, rx, ry, a.map_state == IsViewable);
  } else if (argc >= 4 && strcmp(argv[1], "warp") == 0) {
    int x = atoi(argv[2]), y = atoi(argv[3]);
    XWarpPointer(d, None, DefaultRootWindow(d), 0, 0, 0, 0, x, y);
    XTestFakeMotionEvent(d, DefaultScreen(d), x, y, 0);
    XFlush(d); XSync(d, False); usleep(150000);
    query_pointer(d, "WARP");
  } else if (argc >= 4 && strcmp(argv[1], "click") == 0) {
    int x = atoi(argv[2]), y = atoi(argv[3]);
    int a, b, c, d2;
    Bool xtest = XTestQueryExtension(d, &a, &b, &c, &d2);
    XWarpPointer(d, None, DefaultRootWindow(d), 0, 0, 0, 0, x, y);
    XTestFakeMotionEvent(d, DefaultScreen(d), x, y, 0);
    XFlush(d); XSync(d, False); usleep(150000);
    fprintf(stderr, "XTEST ext=%d screen=%d\n", xtest, DefaultScreen(d));
    query_pointer(d, "PRE_CLICK");
    long long down = now_ms();
    printf("PRESS_DOWN_TS %lld\n", down); fflush(stdout);
    XTestFakeButtonEvent(d, 1, True, 0);
    XFlush(d); XSync(d, False); usleep(80000);
    long long up = now_ms();
    printf("PRESS_UP_TS %lld\n", up); fflush(stdout);
    XTestFakeButtonEvent(d, 1, False, 0);
    XFlush(d); XSync(d, False); usleep(80000);
    query_pointer(d, "POST_CLICK");
  } else if (argc >= 5 && strcmp(argv[1], "clickwin") == 0) {
    Window w = strtoul(argv[2], NULL, 0);
    int x = atoi(argv[3]), y = atoi(argv[4]);
    XWarpPointer(d, None, w, 0, 0, 0, 0, x, y);
    XFlush(d); XSync(d, False); usleep(150000);
    query_pointer(d, "PRE_CLICK");
    long long down = now_ms();
    printf("PRESS_DOWN_TS %lld\n", down); fflush(stdout);
    XTestFakeButtonEvent(d, 1, True, 0);
    XFlush(d); XSync(d, False); usleep(80000);
    long long up = now_ms();
    printf("PRESS_UP_TS %lld\n", up); fflush(stdout);
    XTestFakeButtonEvent(d, 1, False, 0);
    XFlush(d); XSync(d, False); usleep(80000);
    query_pointer(d, "POST_CLICK");
  } else if (argc >= 3 && strcmp(argv[1], "activate") == 0) {
    Window w = strtoul(argv[2], NULL, 0);
    XRaiseWindow(d, w);
    XSetInputFocus(d, w, RevertToParent, CurrentTime);
    XFlush(d); XSync(d, False); usleep(150000);
    Window f; int revert; XGetInputFocus(d, &f, &revert);
    printf("ACTIVATE 0x%lx focus=0x%lx revert=%d\n", w, f, revert);
  } else if (argc >= 2 && strcmp(argv[1], "pointer") == 0) {
    query_pointer(d, "POINTER");
  } else if (argc >= 2 && strcmp(argv[1], "focus") == 0) {
    Window f; int revert;
    XGetInputFocus(d, &f, &revert);
    printf("FOCUS 0x%lx revert=%d\n", f, revert);
  } else if (argc >= 6 && strcmp(argv[1], "move") == 0) {
    Window w = strtoul(argv[2], NULL, 0);
    XMoveResizeWindow(d, w, atoi(argv[3]), atoi(argv[4]), atoi(argv[5]), atoi(argv[6]));
    XFlush(d);
    printf("MOVED 0x%lx\n", w);
  } else { fprintf(stderr, "USAGE geometry|warp|click|clickwin|activate|pointer|focus|move\n"); return 1; }
  XCloseDisplay(d); return 0;
}
