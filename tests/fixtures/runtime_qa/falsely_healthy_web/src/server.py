from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import os


PAGE = Path(__file__).with_name("index.html")


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            body = b"healthy"
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path == "/":
            body = PAGE.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        self.send_error(404)

    def do_POST(self):
        if self.path != "/save":
            self.send_error(404)
            return
        length = int(self.headers.get("Content-Length", "0"))
        message = self.rfile.read(length).decode("utf-8")
        if not message.strip():
            self.send_error(400)
            return
        body = b"Saved"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format, *_args):
        return


ThreadingHTTPServer(
    (os.environ.get("HOST", "127.0.0.1"), int(os.environ.get("PORT", "4173"))),
    Handler,
).serve_forever()
