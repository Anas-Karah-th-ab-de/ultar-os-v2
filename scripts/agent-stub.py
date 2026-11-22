#!/usr/bin/env python3
"""TEST-ATTRAPPE, kein Sprachmodell. Spielt einen OpenAI-kompatiblen Modell-Server mit festem Drehbuch,
um die Agenten-Schleife der Oberfläche (HTTP, Werkzeuge, Rückfrage) ohne echtes Modell zu prüfen.

  scripts/agent-stub.py [port]     (QEMU-Gast erreicht den Host unter 10.0.2.2:<port>)
"""
import json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer


def decide(messages):
    last = messages[-1]["content"]
    user = [m["content"] for m in messages if m["role"] == "user" and not m["content"].startswith("TOOL_RESULT")][-1].lower()
    if last.startswith("TOOL_RESULT"):
        return {"answer": "Erledigt. Ergebnis: " + last[13:160]}
    if "hell" in user or "light" in user:
        return {"tool": "set_setting", "args": {"name": "dark", "value": "false"}}
    if "speicher" in user or "disk" in user:
        return {"tool": "run_command", "args": {"command": "df -h / | tail -1"}}
    if "status" in user:
        return {"tool": "get_status", "args": {}}
    return {"answer": "Ich bin die Test-Attrappe und kenne nur: hell, speicher, status."}


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _send(self, obj):
        b = json.dumps(obj).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        self._send({"data": [{"id": "stub"}]})

    def do_POST(self):
        req = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        out = decide(req["messages"])
        print("->", json.dumps(out, ensure_ascii=False), flush=True)
        self._send({"choices": [{"message": {"role": "assistant", "content": json.dumps(out, ensure_ascii=False)}}]})


HTTPServer(("0.0.0.0", int(sys.argv[1]) if len(sys.argv) > 1 else 8099), H).serve_forever()
