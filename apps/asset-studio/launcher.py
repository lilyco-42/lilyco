"""Native Windows window; the same loopback app remains available in a browser."""
import json
import os
from pathlib import Path
import socket
import sys
import threading
import time
import webbrowser

import uvicorn
from studio.app import create_app


def main():
    # Windows redirected consoles may default to cp1252; keep the startup URL readable.
    for stream in (sys.stdout, sys.stderr):
        if stream is not None and hasattr(stream, 'reconfigure'):
            stream.reconfigure(encoding='utf-8', errors='replace')
    # Keep the bound socket: no free-port race between probing and listening.
    sock = socket.socket()
    sock.bind(('127.0.0.1', 0))
    port = sock.getsockname()[1]
    url = f'http://127.0.0.1:{port}/'
    app = create_app()
    config = uvicorn.Config(app, host='127.0.0.1', port=port, log_level='warning',
                            loop='asyncio', http='h11', limit_concurrency=8)
    server = uvicorn.Server(config)

    print('Lain42 素材工坊 · 所有处理都在本机', flush=True)
    print(url, flush=True)
    ci = os.environ.get('GITHUB_ACTIONS') == 'true'
    native_smoke = ci and os.environ.get('LAIN42_DESKTOP_SMOKE') == 'true'
    native = sys.platform == 'win32' and '--browser' not in sys.argv and (not ci or native_smoke)
    if not native:
        def open_when_ready():
            for _ in range(200):
                if server.started:
                    if not ci:
                        webbrowser.open(url)
                    return
                time.sleep(0.05)
        threading.Thread(target=open_when_ready, daemon=True).start()
        print('保存成果后关闭启动窗口，或按 Ctrl+C 退出。', flush=True)
        server.run(sockets=[sock])
        return

    worker = threading.Thread(target=lambda: server.run(sockets=[sock]), daemon=True)
    worker.start()
    try:
        for _ in range(200):
            if server.started or not worker.is_alive():
                break
            time.sleep(0.05)
        if not server.started:
            raise RuntimeError('本机服务未能启动，请重新打开应用')
        import webview
        # No Python functions are exposed to page scripts. Downloads still require
        # the loopback API's HttpOnly session, like the ordinary browser frontend.
        webview.settings['ALLOW_DOWNLOADS'] = True
        webview.settings['ALLOW_FILE_URLS'] = False
        if native_smoke:
            webview.settings['REMOTE_DEBUGGING_PORT'] = int(os.environ['LAIN42_DESKTOP_DEBUG_PORT'])
            Path(os.environ['LAIN42_DESKTOP_RECEIPT']).write_text(json.dumps({
                'url': url, 'workspace_root': str(app.state.workspaces.root),
            }), encoding='utf-8')
        window = webview.create_window('Lain42 素材工坊', url, width=1280, height=900,
                                      min_size=(800, 600), text_select=True,
                                      background_color='#171719')
        def apply_window_icon():
            from System.Drawing import Icon
            icon_file = Path(__file__).parent / 'resources/app.ico'
            window._lain42_icon = Icon(str(icon_file))
            window.native.Icon = window._lain42_icon
            window.native.ShowIcon = True
            if native_smoke:
                startup_file = Path(os.environ['LAIN42_DESKTOP_RECEIPT'])
                startup = json.loads(startup_file.read_text(encoding='utf-8'))
                startup['native_icon_applied'] = window.native.Icon is not None
                startup_file.write_text(json.dumps(startup), encoding='utf-8')
        window.events.shown += apply_window_icon
        window.events.closed += lambda: setattr(server, 'should_exit', True)
        if not ci:
            import ctypes
            console = ctypes.windll.kernel32.GetConsoleWindow()
            if console:
                ctypes.windll.user32.ShowWindow(console, 0)
        # Explicit renderer: never silently switch to the obsolete IE engine.
        webview.start(gui='edgechromium', private_mode=True)
    except Exception:
        if ci:
            raise
        import ctypes
        ctypes.windll.user32.MessageBoxW(None,
            '桌面窗口启动失败。请安装 Microsoft WebView2 Runtime 后重试。\n'
            '当前将用默认浏览器打开同一个本机工作台；完成后关闭启动窗口。',
            'Lain42 素材工坊', 0x40)
        console = ctypes.windll.kernel32.GetConsoleWindow()
        if console:
            ctypes.windll.user32.ShowWindow(console, 5)
        webbrowser.open(url)
        try:
            while worker.is_alive():
                time.sleep(.2)
        except KeyboardInterrupt:
            pass
    finally:
        server.should_exit = True
        worker.join(timeout=15)
        sock.close()
        if worker.is_alive():
            raise RuntimeError('本机服务未能正常退出')


if __name__ == '__main__':
    main()
