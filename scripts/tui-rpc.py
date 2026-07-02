#!/usr/bin/env python3
"""Minimal JSON-RPC client for the copilot-quorum TUI remote control socket.

Start the TUI with a socket:
    copilot-quorum --listen /tmp/quorum.sock

Then drive it from another terminal (or from a coding agent):
    scripts/tui-rpc.py /tmp/quorum.sock state.get
    scripts/tui-rpc.py /tmp/quorum.sock panes.list
    scripts/tui-rpc.py /tmp/quorum.sock pane.read '{"last": 5}'
    scripts/tui-rpc.py /tmp/quorum.sock input.send '{"text": "Fix the bug in login.rs"}'
    scripts/tui-rpc.py /tmp/quorum.sock command.exec '{"command": "solo"}'
    scripts/tui-rpc.py /tmp/quorum.sock interaction.spawn '{"form": "ask", "query": "What is DDD?"}'
    scripts/tui-rpc.py /tmp/quorum.sock hil.respond '{"decision": "approve"}'

Prints the JSON-RPC result (or error) to stdout. Exit 0 on result, 1 on error.
"""

import json
import re
import socket
import sys


def request(sock_path: str, method: str, params):
    conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    conn.settimeout(30)
    conn.connect(sock_path)
    body = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    ).encode()
    conn.sendall(b"Content-Length: %d\r\n\r\n" % len(body) + body)

    buf = b""
    while True:
        if b"\r\n\r\n" in buf:
            head, rest = buf.split(b"\r\n\r\n", 1)
            m = re.search(rb"Content-Length: (\d+)", head)
            n = int(m.group(1))
            while len(rest) < n:
                rest += conn.recv(65536)
            conn.close()
            return json.loads(rest[:n])
        chunk = conn.recv(65536)
        if not chunk:
            raise ConnectionError("socket closed before response")
        buf += chunk


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    sock_path, method = sys.argv[1], sys.argv[2]
    params = json.loads(sys.argv[3]) if len(sys.argv) > 3 else {}
    resp = request(sock_path, method, params)
    if "result" in resp:
        print(json.dumps(resp["result"], ensure_ascii=False, indent=2))
        return 0
    print(json.dumps(resp.get("error", resp), ensure_ascii=False, indent=2))
    return 1


if __name__ == "__main__":
    sys.exit(main())
