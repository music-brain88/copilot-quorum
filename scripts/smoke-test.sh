#!/usr/bin/env bash
# Headless E2E smoke test for copilot-quorum against the real Copilot CLI.
#
# Runs the one-shot agent mode with a read-only task and asserts, from the
# JSONL conversation transcript, that (1) the run completed successfully and
# (2) the custom-tool path (external_tool.requested → LocalToolExecutor)
# actually executed — catching "the model answered from context without
# calling tools" regressions that look like success on stdout.
#
# Usage:
#   scripts/smoke-test.sh [MODEL]        # default: gpt-5.3-codex
#
# Output lands in a temp dir (printed at the end). Exit 0 = pass.
# Designed to be runnable by coding agents: no TTY, no interaction.
set -uo pipefail

MODEL="${1:-gpt-5.3-codex}"
OUT="$(mktemp -d "${TMPDIR:-/tmp}/quorum-smoke.XXXXXX")"
PROMPT="Use the read_file tool to read README.md and tell me the project name in one short sentence. Do not modify anything."

echo "== copilot-quorum smoke test (model: $MODEL) =="
echo "   logs: $OUT"

# Isolated config: auto-approve HiL gates so the run never blocks on stdin.
mkdir -p "$OUT/config/copilot-quorum"
cat >"$OUT/config/copilot-quorum/init.lua" <<'LUA'
quorum.config.set("agent.hil_mode", "auto_approve")
LUA

timeout 300 env XDG_CONFIG_HOME="$OUT/config" cargo run -q -p copilot-quorum -- \
    --no-quorum -q -vv -m "$MODEL" --log-dir "$OUT" \
    "$PROMPT" >"$OUT/stdout.log" 2>"$OUT/stderr.log" </dev/null
EXIT=$?

fail() {
    echo "FAIL: $1"
    echo "--- stderr tail ---"
    tail -20 "$OUT/stderr.log"
    exit 1
}

[ "$EXIT" -eq 0 ] || fail "process exited with $EXIT"

JSONL=$(ls "$OUT"/session-*.conversation.jsonl 2>/dev/null | head -1)
[ -n "$JSONL" ] || fail "no conversation JSONL produced"

# 1. Run must have completed successfully.
grep -q '"type":"agent_complete"' "$JSONL" ||
    grep -q '"agent_complete"' "$JSONL" || fail "no agent_complete event in JSONL"

python3 - "$JSONL" <<'EOF' || exit 1
import json, sys
events = [json.loads(l) for l in open(sys.argv[1])]
complete = [e for e in events if e.get("type") == "agent_complete"]
if not complete or not complete[-1].get("success"):
    print("FAIL: agent_complete missing or success=false"); sys.exit(1)

# 2. The external-tool path must have fired: plan must contain real tasks
#    (create_plan delivered its arguments to us, not just to the CLI).
if complete[-1].get("total_tasks", 0) < 1:
    print("FAIL: total_tasks == 0 — custom tool path (external_tool.requested) "
          "likely broken; run scripts/probe-copilot-server.py to diagnose")
    sys.exit(1)
print(f"OK: agent completed, {complete[-1]['total_tasks']} task(s) executed")
EOF

# 3. The external tool request must appear in the debug log.
if ! grep -q "External tool call received" "$OUT/stderr.log"; then
    fail "no 'External tool call received' in debug log — external_tool.requested never arrived"
fi

echo "PASS ($OUT)"
