"""Synthetic non-Node web app fixture for restricted browser QA."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import socketserver
import threading
import time

UDP_SEEN = threading.Event()

class UdpHandler(socketserver.BaseRequestHandler):
    def handle(self):
        UDP_SEEN.set()

class UdpServer(socketserver.ThreadingUDPServer):
    allow_reuse_address = True

PAGE = b'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>QA fixture</title><style>
body{font:16px sans-serif;margin:2rem;max-width:52rem}label{display:block;margin:.75rem 0}input{font:inherit;padding:.5rem}
button{font:inherit;padding:.6rem;margin:.4rem 0}.error{color:#a00}.clipped{height:1rem;overflow:hidden}
@media(max-width:480px){body{margin:.5rem}main{padding:.5rem}}
</style></head><body><main><h1>Preview fixture</h1><p id="status" data-testid="status" role="status">Ready</p>
<form id="save"><label for="message">Message</label><input id="message" name="message" required>
<button type="submit">Save message</button></form><button id="open-dialog">Open details</button>
<a href="/?page=settings">Settings</a>
<dialog id="details" data-testid="details-dialog"><p>Details are available</p><button id="close-dialog">Close</button></dialog>
<p class="clipped" id="clipped">This text can be clipped by the seeded visual defect.</p>
<script>
const params=new URLSearchParams(location.search);
const status=document.querySelector('#status');
if(params.get('defect')==='clipped') document.querySelector('#clipped').classList.add('clipped');
if(params.get('defect')==='js-error') setTimeout(()=>{throw new Error('seeded fixture exception')},0);
if(params.get('defect')==='metadata') fetch('http://169.254.169.254/latest/meta-data/').catch(()=>{status.textContent='Request blocked'});
if(params.get('defect')==='iframe') { const frame=document.createElement('iframe'); frame.src='http://169.254.169.254/latest/meta-data/'; document.body.append(frame); }
if(params.get('defect')==='localhost') fetch('http://127.0.0.1:4173/').catch(()=>{});
if(params.get('defect')==='alternate-port') fetch('http://qa-preview:8081/').catch(()=>{});
if(params.get('defect')==='websocket') { const socket=new WebSocket('ws://example.com/socket'); socket.onerror=()=>{}; }
if(params.get('defect')==='wrong-action') document.querySelector('#open-dialog').addEventListener('click',()=>{status.textContent='Wrong destination';history.pushState({},'', '/?page=wrong')});
document.querySelector('#save').addEventListener('submit',async event=>{event.preventDefault();const input=document.querySelector('#message');
if(!input.value.trim()){status.textContent='Message is required';status.className='error';return}
const response=await fetch('/save',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({message:input.value})});
status.textContent=response.ok?'Saved':'Save failed';status.className=response.ok?'':'error';});
document.querySelector('#open-dialog').addEventListener('click',()=>document.querySelector('#details').showModal());
document.querySelector('#close-dialog').addEventListener('click',()=>document.querySelector('#details').close());
</script></main></body></html>'''


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/__cancel_started":
            try:
                with open("/tmp/qa-browser-cancel-started", "rb") as marker:
                    started = marker.read() == b"started"
            except OSError:
                started = False
            body = b'<p data-testid="status">started</p>' if started else b'<p data-testid="status">waiting</p>'
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path == "/?defect=slow-cancel":
            with open("/tmp/qa-browser-cancel-started", "wb") as marker:
                marker.write(b"started")
            time.sleep(30)
        if self.path == "/?defect=parallel":
            marker = "/tmp/qa-browser-parallel-active"
            with open(marker, "wb") as active:
                active.write(b"active")
            try:
                time.sleep(10)
            finally:
                try:
                    os.unlink(marker)
                except FileNotFoundError:
                    pass
        if self.path == "/__cache_ready":
            cache_root = os.environ.get("NPM_CONFIG_CACHE", "")
            try:
                with open(os.path.join(cache_root, "package.tgz"), "rb") as cache_file:
                    cache_ready = (
                        cache_file.read() == b"approved offline browser cache fixture"
                        and os.environ.get("NPM_CONFIG_OFFLINE") == "true"
                    )
            except (OSError, ValueError):
                cache_ready = False
            body = (
                b'<!doctype html><p data-testid="cache-status">available</p>'
                if cache_ready
                else b'<!doctype html><p data-testid="cache-status">missing</p>'
            )
            self.send_response(200 if cache_ready else 503)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path == "/__udp_seen":
            body = b"yes" if UDP_SEEN.is_set() else b"no"
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path == "/redirect":
            self.send_response(302)
            self.send_header("Location", "http://169.254.169.254/latest/meta-data/")
            self.end_headers()
            return
        if self.path.split("?", 1)[0] != "/":
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(PAGE)))
        self.end_headers()
        self.wfile.write(PAGE)

    def do_POST(self):
        if self.path != "/save":
            self.send_error(404)
            return
        length = int(self.headers.get("Content-Length", "0"))
        if length <= 0 or length > 4096:
            self.send_error(413)
            return
        try:
            value = json.loads(self.rfile.read(length))
            valid = isinstance(value, dict) and isinstance(value.get("message"), str) and len(value["message"]) <= 1024
        except (ValueError, UnicodeDecodeError):
            valid = False
        status = 200 if valid else 400
        body = b'{"saved":true}' if valid else b'{"saved":false}'
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        return


if __name__ == "__main__":
    port = int(os.environ.get("PORT", os.environ.get("QA_FIXTURE_PORT", "4173")))
    host = os.environ.get("HOST", os.environ.get("QA_FIXTURE_HOST", "127.0.0.1"))
    udp = UdpServer((host, int(os.environ.get("QA_FIXTURE_UDP_PORT", "4174"))), UdpHandler)
    threading.Thread(target=udp.serve_forever, daemon=True).start()
    ThreadingHTTPServer((host, port), Handler).serve_forever()
