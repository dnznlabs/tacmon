import http.server, json, sys
item = '{"amount":"1","until":9999999999}'
n = (4 * 1024 * 1024 - 64) // (len(item) + 1)
WANTS = ('{"wants":[' + ",".join([item] * n) + ']}').encode()
TERMS = json.dumps({"minAmount":"1","maxAmount":"100000000000","feeBps":"150","flatFee":"10000000"}).encode()
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self,*a): pass
    def do_GET(self):
        b = WANTS if self.path == "/wants" else TERMS if self.path == "/info" else b'{"ok":true,"ads":[]}'
        self.send_response(200); self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
sys.stderr.write(f"{n} wants, {len(WANTS)} bytes\n")
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
