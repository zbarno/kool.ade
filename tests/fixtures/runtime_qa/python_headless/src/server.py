from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/health":
            self.send_error(404)
            return
        self._json(200, {"healthy": True})

    def do_POST(self):
        if self.path != "/save":
            self.send_error(404)
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            message = json.loads(self.rfile.read(length)).get("message", "")
        except (AttributeError, ValueError, json.JSONDecodeError):
            self.send_error(400)
            return
        if not isinstance(message, str) or not message.strip():
            self.send_error(400)
            return
        self._json(200, {"saved": True})

    def _json(self, status, value):
        body = json.dumps(value, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format, *_args):
        return


if __name__ == "__main__":
    host = os.environ.get("HOST", "0.0.0.0")
    port = int(os.environ.get("PORT", "4173"))
    ThreadingHTTPServer((host, port), Handler).serve_forever()
