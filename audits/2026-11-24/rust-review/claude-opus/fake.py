# A stand-in for the board / provider API / RPC, on localhost only. Mode chosen by argv.
import http.server, json, sys
port, mode = int(sys.argv[1]), sys.argv[2]
BIG = str(2**127)
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def send(self, obj, code=200):
        b = json.dumps(obj).encode(); self.send_response(code)
        self.send_header("Content-Type","application/json"); self.send_header("Content-Length",str(len(b))); self.end_headers(); self.wfile.write(b)
    def hit(self, m):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0)).decode()
        with open(f"sink-{port}.log","a") as f: f.write(f"{m} {self.path} ua={self.headers.get('User-Agent')} body={body}\n")
    def do_POST(self):
        self.hit("POST")
        if mode == "redirect": self.send_response(307); self.send_header("Location","http://127.0.0.1:18799/stolen-post"); self.end_headers(); return
        self.send({"result":"0x0"})
    def do_GET(self):
        self.hit("GET")
        if mode == "redirect":
            self.send_response(302); self.send_header("Location", f"http://127.0.0.1:18799/elsewhere{self.path}"); self.end_headers(); return
        p = self.path
        if p == "/info": self.send({"minAmount":"1000000000","maxAmount":"100000000000","feeBps":100,"flatFee":"10000000"})
        elif p == "/health": self.send({"ok":True})
        elif p.endswith("/wants"):
            if mode == "overflow": self.send({"wants":[{"amount":BIG},{"amount":BIG},{"amount":"500000000000"}]})
            elif mode == "capstr": self.send({"wants":[{"amount":"5000000000","maxFeeBps":"10"}]})
            else: self.send({"wants":[]})
        elif p.endswith("/ads"):
            if mode == "overflow": self.send({"ads":[{"name":"cheap?","claimPub":"03ff","feeBps":"18446744073709551616","flatFee":str(2**64*10**6+1),"minAmount":"1","maxAmount":"10"},{"name":"honest","claimPub":"03aa","feeBps":30,"flatFee":"10000000"}]})
            elif mode == "spoof": self.send({"ads":[{"name":"IMPERSONATOR","claimPub":"02a1b2c3d4e5f6","feeBps":1,"flatFee":"0"}]})
            else: self.send({"ads":[]})
        else: self.send({}, 404)
http.server.HTTPServer(("127.0.0.1", port), H).serve_forever()
