#!/usr/bin/env python3
"""A stand-in for Airframes Desktop's netboot API (docs/NETBOOT.md), for
testing the network installer in QEMU without Desktop.

    mock-desktop.py --dir <dir> --image <name> [--port 47811] [--waits 1]
                    [--wrong-sha] [--wrong-target] [--sha HEX] [--log events.jsonl]

<dir> holds the netinstall files (vmlinuz, initramfs.img), the image and
SHA256SUMS / SHA256SUMS.minisig. The first --waits /hello calls answer
"wait", the next one answers "install" for the first non-removable disk the
installer reported. Every request is appended to --log as one JSON line.
"""
import argparse
import hashlib
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

TOKEN = "test-token-0123456789"
BASE = "/netboot/v1"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True)
    ap.add_argument("--image", required=True)
    ap.add_argument("--port", type=int, default=47811)
    ap.add_argument("--waits", type=int, default=1)
    ap.add_argument("--wrong-sha", action="store_true")
    ap.add_argument("--wrong-target", action="store_true")
    ap.add_argument("--sha", help="SHA-256 to announce instead of the served file's")
    ap.add_argument("--log", default="events.jsonl")
    ap.add_argument("--host", default="10.0.2.2", help="address the guest reaches us on")
    args = ap.parse_args()

    image_path = os.path.join(args.dir, args.image)
    sha = hashlib.sha256(open(image_path, "rb").read()).hexdigest()
    if args.sha:
        sha = args.sha
    if args.wrong_sha:
        sha = "0" * 64
    state = {"hellos": 0}
    logf = open(args.log, "a", buffering=1)

    def record(kind, **kw):
        logf.write(json.dumps({"kind": kind, **kw}) + "\n")

    class H(BaseHTTPRequestHandler):
        def log_message(self, fmt, *a):
            sys.stderr.write("mock-desktop: " + (fmt % a) + "\n")

        def authed(self):
            q = parse_qs(urlparse(self.path).query)
            auth = self.headers.get("Authorization", "")
            return auth == f"Bearer {TOKEN}" or q.get("token") == [TOKEN]

        def send_json(self, code, obj):
            body = json.dumps(obj).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def send_file(self, path):
            if not os.path.isfile(path):
                return self.send_json(404, {"error": "not found"})
            size = os.path.getsize(path)
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(size))
            self.end_headers()
            with open(path, "rb") as f:
                while True:
                    chunk = f.read(1 << 20)
                    if not chunk:
                        break
                    self.wfile.write(chunk)

        def do_GET(self):
            p = urlparse(self.path).path
            if p in ("/", BASE, BASE + "/"):
                return self.send_json(404, {})
            if not self.authed():
                return self.send_json(403, {"error": "bad token"})
            record("get", path=p)
            for prefix in (BASE + "/files/", BASE + "/image/", BASE + "/release/test/"):
                if p.startswith(prefix):
                    name = os.path.basename(p[len(prefix):])
                    return self.send_file(os.path.join(args.dir, name))
            self.send_json(404, {"error": "not found"})

        def do_POST(self):
            p = urlparse(self.path).path
            n = int(self.headers.get("Content-Length", "0") or 0)
            body = json.loads(self.rfile.read(n) or b"{}")
            if not self.authed():
                return self.send_json(403, {"error": "bad token"})
            if p == BASE + "/progress":
                record("progress", **body)
                return self.send_json(202, {"ok": True})
            if p != BASE + "/hello":
                return self.send_json(404, {})
            record("hello", body=body)
            state["hellos"] += 1
            if state["hellos"] <= args.waits:
                return self.send_json(200, {"action": "wait", "retry_after": 2,
                                            "message": "Waiting for you to approve this device in Airframes Desktop"})
            disks = [d for d in body.get("disks", []) if not d.get("removable")]
            if not disks:
                return self.send_json(200, {"action": "abort", "message": "no disk"})
            d = disks[0]
            target = {"path": d["path"], "serial": d.get("serial"), "size_bytes": d["size_bytes"]}
            if args.wrong_target:
                target["size_bytes"] += 512
            self.send_json(200, {
                "action": "install",
                "image": {"name": args.image, "url": f"image/{args.image}", "sha256": sha,
                          "size_bytes": os.path.getsize(image_path)},
                "sums": {"url": f"http://{args.host}:{args.port}{BASE}/release/test/SHA256SUMS",
                         "sig_url": "release/test/SHA256SUMS.minisig"},
                "target": target,
                "erase_confirmed": True,
                "countdown_seconds": 2,
                "awcfg": {
                    "schema": 1, "hostname": "airwaves-qemu", "channel": "stable",
                    "timezone": "America/Los_Angeles", "wifi": {"ssid": "", "psk": ""}, "ssh_keys": [],
                    "auto_install": True,
                    "enroll": {"url": f"http://{args.host}:{args.port}{BASE}/enroll",
                               "code": "enroll-code-abcdef0123456789", "expires_at": "2030-01-01T00:00:00Z"},
                    "station": {"ident": "XX-QEMU", "latitude": 38.6766, "longitude": -121.0158,
                                "altitude_m": 260, "name": "QEMU test",
                                "xng": {"feed_airframes": True}},
                },
                "post_install": "poweroff",
            })

    srv = ThreadingHTTPServer(("0.0.0.0", args.port), H)
    print(f"mock Desktop on :{args.port}{BASE} token={TOKEN}", flush=True)
    srv.serve_forever()


if __name__ == "__main__":
    main()
