#!/usr/bin/env python3
"""TEST-ATTRAPPE für die GitHub-Release-Schnittstelle: liefert die Dateien eines Ordners als "neuestes Release".

  scripts/release-stub.py <ordner> <port> <tag>
  GET /repos/<besitzer>/<projekt>/releases/latest  -> JSON wie bei GitHub (tag_name, assets[name,url])
  GET /assets/<datei>                               -> Datei
Zum Testen der Updater: ULTARFLASH_API=http://127.0.0.1:<port>  bzw. in /data/ultar/update.toml: api = "http://10.0.2.2:<port>"
"""
import json, os, sys
from http.server import BaseHTTPRequestHandler, HTTPServer

D, PORT, TAG = sys.argv[1], int(sys.argv[2]), sys.argv[3]


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        host = self.headers.get("Host", "127.0.0.1:%d" % PORT)
        if self.path.endswith("/releases/latest"):
            assets = [{"name": n, "url": "http://%s/assets/%s" % (host, n), "size": os.path.getsize(os.path.join(D, n))} for n in sorted(os.listdir(D))]
            body = json.dumps({"tag_name": TAG, "assets": assets}).encode()
            ctype = "application/json"
        elif self.path.startswith("/assets/") and os.path.isfile(os.path.join(D, os.path.basename(self.path))):
            body = open(os.path.join(D, os.path.basename(self.path)), "rb").read()
            ctype = "application/octet-stream"
        else:
            self.send_response(404); self.end_headers(); return
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


HTTPServer(("0.0.0.0", PORT), H).serve_forever()
