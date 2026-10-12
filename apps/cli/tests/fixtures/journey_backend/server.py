"""Fixture app: serves site/ and counts every DELETE it receives in deletes.count."""
import http.server
import sys


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory="site", **kwargs)

    def do_DELETE(self):
        with open("deletes.count", "a", encoding="utf-8") as count:
            count.write("x")
        self.send_response(204)
        self.end_headers()

    def log_message(self, *args):
        pass


http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
