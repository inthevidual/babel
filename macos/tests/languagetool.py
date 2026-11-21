#!/usr/bin/env python3
"""A stand-in for LanguageTool's /v2/check in the UI tests: every "report"
is a grammar issue, so which segments have issues is known.

    python3 macos/tests/languagetool.py PORT
"""
import json
import re
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs


def utf16_len(s):
    return len(s.encode("utf-16-le")) // 2


class Check(BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0))).decode()
        text = parse_qs(body).get("text", [""])[0]
        matches = [
            {
                "offset": utf16_len(text[: m.start()]),
                "length": utf16_len(m.group()),
                "message": "A test issue.",
                "shortMessage": "Test",
                "replacements": [{"value": "account"}],
                "rule": {"id": "TEST_RULE", "issueType": "grammar", "category": {"id": "GRAMMAR"}},
            }
            for m in re.finditer(r"\breport\b", text, re.IGNORECASE)
        ]
        out = json.dumps({"matches": matches}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)

    def log_message(self, *args):
        pass


HTTPServer(("127.0.0.1", int(sys.argv[1])), Check).serve_forever()
