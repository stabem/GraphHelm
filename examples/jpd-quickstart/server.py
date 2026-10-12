"""The HTTP promise: serves site/ on 127.0.0.1:<port> (default 8765)."""
import http.server
import sys


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory="site", **kwargs)

    def log_message(self, *args):
        pass


port = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
