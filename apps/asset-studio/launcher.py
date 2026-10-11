"""Double click: a loopback-only FastAPI app in the default browser."""
import socket
import sys
import threading
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

    def open_when_ready():
        import time
        for _ in range(200):
            if server.started:
                import os
                if os.environ.get('GITHUB_ACTIONS') != 'true':
                    webbrowser.open(url)
                return
            time.sleep(0.05)

    threading.Thread(target=open_when_ready, daemon=True).start()
    print('Lain42 素材工坊 · 所有处理都在本机', flush=True)
    print('保存结果后，请在界面删除工作区；关闭这个窗口可退出。', flush=True)
    print(url, flush=True)
    server.run(sockets=[sock])


if __name__ == '__main__':
    main()
