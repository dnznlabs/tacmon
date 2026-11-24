# Serves a 20 MB /wants body of tiny elements on localhost, to measure tacmon's memory amplification.
import http.server, sys
N = 10_000_000
body = b'{"wants":[' + b'0,' * (N - 1) + b'0]}'
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        b = body if self.path.endswith("/wants") else b'{}'
        self.send_response(200); self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
