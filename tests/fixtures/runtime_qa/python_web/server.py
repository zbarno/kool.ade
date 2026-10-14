#!/usr/bin/env python3
"""Small standard-library web target for future isolated browser QA runs."""

from argparse import ArgumentParser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        route = urlparse(self.path).path
        if route == "/health":
            self.respond(200, "text/plain; charset=utf-8", "ready")
            return
        if route == "/error":
            script = "<script>console.error('fixture intentional error')</script>"
        else:
            script = ""
        if route not in {"/", "/done", "/error"}:
            self.respond(404, "text/plain; charset=utf-8", "not found")
            return
        title = "Saved" if route == "/done" else "Python Preview"
        body = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title><style>
body{{font:16px sans-serif;margin:2rem;max-width:48rem}}label,input,button{{display:block;margin:.75rem 0}}
@media(max-width:600px){{body{{margin:1rem}}}}
</style></head><body>{script}<h1>{title}</h1>
<a href="/done">Open saved state</a><button id="open-dialog">Open dialog</button>
<dialog id="fixture-dialog"><p>Dialog content</p><button id="close-dialog">Close</button></dialog>
<form method="post" action="/submit"><label for="name">Name</label>
<input id="name" name="name" required><button type="submit">Save</button></form>
<output id="viewport">viewport ready</output>
<script>
const dialog=document.querySelector('#fixture-dialog');
document.querySelector('#open-dialog').onclick=()=>dialog.showModal();
document.querySelector('#close-dialog').onclick=()=>dialog.close();
window.addEventListener('resize',()=>document.querySelector('#viewport').textContent=`${{innerWidth}}x${{innerHeight}}`);
</script></body></html>"""
        self.respond(200, "text/html; charset=utf-8", body)

    def do_POST(self):
        if urlparse(self.path).path != "/submit":
            self.respond(404, "text/plain; charset=utf-8", "not found")
            return
        size = int(self.headers.get("Content-Length", "0"))
        if size > 4096:
            self.respond(413, "text/plain; charset=utf-8", "form too large")
            return
        values = parse_qs(self.rfile.read(size).decode("utf-8", "replace"))
        if not values.get("name", [""])[0].strip():
            self.respond(400, "text/plain; charset=utf-8", "name required")
            return
        self.send_response(303)
        self.send_header("Location", "/done")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def respond(self, status, content_type, content):
        payload = content.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, _format, *_args):
        # Fixture logs contain no secrets or caller-supplied URLs.
        return


def main():
    parser = ArgumentParser()
    parser.add_argument("--port", type=int, required=True)
    port = parser.parse_args().port
    if not 1024 <= port <= 65535:
        raise SystemExit("port must be in the unprivileged range")
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()


if __name__ == "__main__":
    main()
