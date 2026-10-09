#!/usr/bin/env python3
"""Talk to `omahelm serve` over its socket, as the window does.

    helm.py SOCKET [--send JSON]... --until KEY=VALUE [--timeout SECONDS]

Prints every line the engine sends, verbatim, and each request it sent
prefixed with "> ". The requests go after the first `state`. Exits 0 once a
message has KEY (a dotted path, like charts.status) equal to VALUE, and 1
if none does before the timeout.
"""

import argparse
import json
import socket
import sys
import time


def get(msg, path):
    for key in path.split("."):
        if not isinstance(msg, dict) or key not in msg:
            return None
        msg = msg[key]
    return msg


def main():
    p = argparse.ArgumentParser()
    p.add_argument("socket")
    p.add_argument("--send", action="append", default=[])
    p.add_argument("--until", required=True)
    p.add_argument("--timeout", type=float, default=20)
    a = p.parse_args()
    key, want = a.until.split("=", 1)
    deadline = time.monotonic() + a.timeout
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(a.socket)
    s.settimeout(0.5)
    buf, sent = b"", False
    while time.monotonic() < deadline:
        try:
            chunk = s.recv(65536)
        except socket.timeout:
            continue
        if not chunk:
            break
        buf += chunk
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            print(line.decode(), flush=True)
            msg = json.loads(line)
            if json.dumps(get(msg, key)).strip('"') == want:
                return 0
            if msg.get("type") == "state" and not sent:
                for r in a.send:
                    print("> " + r, flush=True)
                    s.sendall(r.encode() + b"\n")
                sent = True
    print(f"no message with {a.until} in {a.timeout:.0f} s", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
