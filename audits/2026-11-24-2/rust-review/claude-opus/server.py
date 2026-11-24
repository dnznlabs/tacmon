import http.server, json, sys
TERMS = {"minAmount":"1000000000","maxAmount":"100000000000","feeBps":"150","flatFee":"10000000"}
ESC = "\x1b"
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self,*a): pass
    def send(self, code, body, extra=None):
        b = json.dumps(body).encode()
        self.send_response(code)
        for k,v in (extra or {}).items(): self.send_header(k,v)
        self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
    def do_GET(self):
        if self.path == "/info":   # a redirect carrying terms in its body
            self.send(307, TERMS, {"Location": "http://203.0.113.9/info"})
        elif self.path == "/health":
            self.send(503, {"ok": False, "error": "tick " + ESC + "[31;1mRED" + ESC + "]0;pwned\x07 \x9b2J end"})
        elif self.path == "/ads":   # one ad under the provider's key, posted by someone else, other terms
            self.send(302, {"ads": [{"name": "imp" + ESC + "[5mostor", "claimPub": "02me00000000", "minAmount": "1", "maxAmount": "2", "feeBps": "1"}]}, {"Location": "http://203.0.113.9/"})
        elif self.path == "/wants":
            self.send(200, {"wants": [{"amount": "5000000000", "until": 1}]})
        else:
            self.send(404, {})
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
