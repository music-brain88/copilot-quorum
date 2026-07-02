#!/usr/bin/env python3
"""Protocol probe for `copilot --server` (JSON-RPC over TCP, LSP framing).

Spawns the Copilot CLI in server mode, registers one custom tool, enables
permission auto-approval (required on CLI 1.0.65+), sends a prompt that
forces a tool call, and prints every protocol event — so upstream protocol
changes can be diagnosed without going through the full Rust stack.

Usage:
    python3 scripts/probe-copilot-server.py [--model MODEL] [--raw]

    --model MODEL   Model ID to use (default: gpt-5.3-codex)
    --raw           Print full raw JSON for every message (very verbose)
    --list-models   Just print the models the CLI offers and exit

Exit codes: 0 = tool round-trip succeeded, 1 = failure (see output).

History: this reproduces the debugging approach used for issue #245
(CLI 1.0.25 protocol break) and the 1.0.65 permission-gating break.
"""

import argparse
import json
import re
import socket
import subprocess
import sys
import time

TOOL = {
    "name": "get_magic_number",
    "description": (
        "Returns the secret magic number. MUST be called to answer any "
        "question about the magic number."
    ),
    "parameters": {"type": "object", "properties": {}, "required": []},
    "overridesBuiltInTool": True,
}

PROMPT = "What is the magic number? You MUST call the get_magic_number tool."


class Rpc:
    def __init__(self):
        self.proc = subprocess.Popen(
            ["copilot", "--server"],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        port = None
        for _ in range(100):
            line = self.proc.stdout.readline()
            m = re.search(r"listening on port (\d+)", line)
            if m:
                port = int(m.group(1))
                break
        if not port:
            raise RuntimeError("copilot --server did not report a port")
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=30)
        self.buf = b""
        self.next_id = 0

    def send(self, obj):
        body = json.dumps(obj).encode()
        self.sock.sendall(b"Content-Length: %d\r\n\r\n" % len(body) + body)

    def recv(self, timeout=60):
        self.sock.settimeout(timeout)
        while True:
            if b"\r\n\r\n" in self.buf:
                head, rest = self.buf.split(b"\r\n\r\n", 1)
                n = int(re.search(rb"Content-Length: (\d+)", head).group(1))
                while len(rest) < n:
                    rest += self.sock.recv(65536)
                self.buf = rest[n:]
                return json.loads(rest[:n])
            try:
                chunk = self.sock.recv(65536)
            except socket.timeout:
                return None
            if not chunk:
                return None
            self.buf += chunk

    def call(self, method, params, timeout=30):
        """Send a request and wait for its response, printing events seen."""
        self.next_id += 1
        rid = self.next_id
        self.send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
        deadline = time.time() + timeout
        while time.time() < deadline:
            msg = self.recv(timeout)
            if msg is None:
                return None
            if msg.get("id") == rid and "method" not in msg:
                return msg
        return None

    def close(self):
        self.proc.kill()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="gpt-5.3-codex")
    ap.add_argument("--raw", action="store_true")
    ap.add_argument("--list-models", action="store_true")
    args = ap.parse_args()

    rpc = Rpc()
    try:
        if args.list_models:
            resp = rpc.call("models.list", {})
            for m in resp["result"]["models"]:
                sup = m.get("capabilities", {}).get("supports", {})
                print(f"{m['id']}  tool_calls={sup.get('tool_calls')}")
            return 0

        resp = rpc.call("session.create", {"model": args.model, "tools": [TOOL]})
        if not resp or "result" not in resp:
            print("FAIL: session.create ->", json.dumps(resp))
            return 1
        sid = resp["result"]["sessionId"]
        print("session:", sid)

        # CLI 1.0.65+: permission request events default to OFF and custom
        # tools are denied outright without this.
        resp = rpc.call(
            "session.permissions.setApproveAll", {"sessionId": sid, "enabled": True}
        )
        print("setApproveAll ->", json.dumps(resp and resp.get("result")))

        rpc.next_id += 1
        rpc.send(
            {
                "jsonrpc": "2.0",
                "id": rpc.next_id,
                "method": "session.send",
                "params": {"sessionId": sid, "prompt": PROMPT},
            }
        )

        tool_ok = False
        deadline = time.time() + 120
        while time.time() < deadline:
            msg = rpc.recv()
            if msg is None:
                print("FAIL: timed out waiting for events")
                return 1
            if args.raw:
                print("RAW:", json.dumps(msg)[:2000])
            if msg.get("method") != "session.event":
                continue
            ev = msg["params"]["event"]
            etype = ev["type"]
            data = ev.get("data", {})
            if etype == "external_tool.requested":
                print("external_tool.requested:", data.get("toolName"))
                rpc.next_id += 1
                rpc.send(
                    {
                        "jsonrpc": "2.0",
                        "id": rpc.next_id,
                        "method": "session.tools.handlePendingToolCall",
                        "params": {
                            "sessionId": sid,
                            "requestId": data["requestId"],
                            "result": "42",
                        },
                    }
                )
            elif etype == "permission.requested":
                print("permission.requested:", json.dumps(data)[:300])
            elif etype == "tool.execution_complete":
                ok = data.get("success")
                print("tool.execution_complete: success =", ok, data.get("error") or "")
                tool_ok = tool_ok or bool(ok)
            elif etype == "assistant.message":
                content = data.get("content", "")
                if content:
                    print("assistant:", content[:200])
            elif etype == "session.idle":
                break

        if tool_ok:
            print("OK: custom tool round-trip succeeded")
            return 0
        print("FAIL: custom tool never executed successfully")
        return 1
    finally:
        rpc.close()


if __name__ == "__main__":
    sys.exit(main())
