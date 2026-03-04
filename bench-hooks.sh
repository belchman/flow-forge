#!/usr/bin/env bash
# FlowForge Hook Overhead Benchmark
# Compares hook latency and token output with/without FlowForge
set -euo pipefail

RUNS=20
BINARY="${1:-flowforge}"

# Realistic Claude Code payloads
PRE_TOOL_USE_PAYLOAD='{
  "session_id":"bench-session","transcript_path":"/tmp/bench.jsonl",
  "cwd":"/tmp","permission_mode":"bypassPermissions",
  "hook_event_name":"PreToolUse",
  "tool_name":"Bash","tool_input":{"command":"ls -la"},
  "tool_use_id":"toolu_bench1"
}'

POST_TOOL_USE_PAYLOAD='{
  "session_id":"bench-session","transcript_path":"/tmp/bench.jsonl",
  "cwd":"/tmp","permission_mode":"bypassPermissions",
  "hook_event_name":"PostToolUse",
  "tool_name":"Read","tool_input":{"file_path":"/tmp/test.txt"},
  "tool_response":{"content":"hello world"},"tool_use_id":"toolu_bench2"
}'

PROMPT_SUBMIT_PAYLOAD='{
  "session_id":"bench-session","transcript_path":"/tmp/bench.jsonl",
  "cwd":"/tmp","permission_mode":"bypassPermissions",
  "hook_event_name":"UserPromptSubmit",
  "prompt":"refactor the authentication module to use JWT tokens instead of session cookies"
}'

SESSION_START_PAYLOAD='{
  "session_id":"bench-session","transcript_path":"/tmp/bench.jsonl",
  "cwd":"/tmp","permission_mode":"bypassPermissions",
  "hook_event_name":"SessionStart"
}'

SESSION_END_PAYLOAD='{
  "session_id":"bench-session","transcript_path":"/tmp/bench.jsonl",
  "cwd":"/tmp","permission_mode":"bypassPermissions",
  "hook_event_name":"SessionEnd"
}'

# Timing helper: runs N iterations, outputs mean/p50/p95 in ms
bench_hook() {
  local hook="$1"
  local payload="$2"
  local label="$3"
  local env_prefix="${4:-}"
  local times=()

  for ((i=0; i<RUNS; i++)); do
    local start end elapsed_ms
    start=$(python3 -c 'import time; print(time.monotonic_ns())')
    if [ -n "$env_prefix" ]; then
      echo "$payload" | env $env_prefix $BINARY hook "$hook" >/dev/null 2>/dev/null
    else
      echo "$payload" | $BINARY hook "$hook" >/dev/null 2>/dev/null
    fi
    end=$(python3 -c 'import time; print(time.monotonic_ns())')
    elapsed_ms=$(python3 -c "print(f'{($end - $start) / 1_000_000:.1f}')")
    times+=("$elapsed_ms")
  done

  # Calculate stats with python
  python3 -c "
import statistics
times = sorted([float(t) for t in '${times[*]}'.split()])
n = len(times)
mean = statistics.mean(times)
p50 = times[n//2]
p95 = times[int(n*0.95)]
mn = times[0]
mx = times[-1]
print(f'  {\"$label\":<35} mean={mean:6.1f}ms  p50={p50:6.1f}ms  p95={p95:6.1f}ms  min={mn:5.1f}  max={mx:5.1f}')
"
}

# Token counting helper
count_tokens() {
  local hook="$1"
  local payload="$2"
  local label="$3"
  local env_prefix="${4:-}"

  local output
  if [ -n "$env_prefix" ]; then
    output=$(echo "$payload" | env $env_prefix $BINARY hook "$hook" 2>/dev/null || true)
  else
    output=$(echo "$payload" | $BINARY hook "$hook" 2>/dev/null || true)
  fi
  local chars=${#output}
  # Rough token estimate: ~4 chars per token for English text
  local tokens=$((chars / 4))
  local lines=$(echo "$output" | wc -l | tr -d ' ')

  printf "  %-35s %5d chars  ~%4d tokens  %3d lines\n" "$label" "$chars" "$tokens" "$lines"
}

echo "╔══════════════════════════════════════════════════════════════════╗"
echo "║           FlowForge Hook Overhead Benchmark                    ║"
echo "╠══════════════════════════════════════════════════════════════════╣"
echo "║  Binary: $(which $BINARY || echo $BINARY)"
echo "║  Runs per hook: $RUNS"
echo "║  Date: $(date '+%Y-%m-%d %H:%M')"
echo "╚══════════════════════════════════════════════════════════════════╝"
echo ""

echo "── 1. LATENCY: FlowForge hooks enabled (normal) ──"
bench_hook "pre-tool-use"       "$PRE_TOOL_USE_PAYLOAD"    "PreToolUse"
bench_hook "post-tool-use"      "$POST_TOOL_USE_PAYLOAD"   "PostToolUse"
bench_hook "user-prompt-submit" "$PROMPT_SUBMIT_PAYLOAD"   "UserPromptSubmit"
bench_hook "session-start"      "$SESSION_START_PAYLOAD"   "SessionStart"
bench_hook "session-end"        "$SESSION_END_PAYLOAD"     "SessionEnd"
echo ""

echo "── 2. LATENCY: FlowForge hooks DISABLED (kill-switch) ──"
bench_hook "pre-tool-use"       "$PRE_TOOL_USE_PAYLOAD"    "PreToolUse (disabled)"        "FLOWFORGE_HOOKS_DISABLED=1"
bench_hook "post-tool-use"      "$POST_TOOL_USE_PAYLOAD"   "PostToolUse (disabled)"       "FLOWFORGE_HOOKS_DISABLED=1"
bench_hook "user-prompt-submit" "$PROMPT_SUBMIT_PAYLOAD"   "UserPromptSubmit (disabled)"  "FLOWFORGE_HOOKS_DISABLED=1"
echo ""

echo "── 3. PER-TOOL-USE OVERHEAD (PreToolUse + PostToolUse combined) ──"
echo "  Simulating 50 tool uses..."
enabled_total=0
disabled_total=0
for ((i=0; i<50; i++)); do
  start=$(python3 -c 'import time; print(time.monotonic_ns())')
  echo "$PRE_TOOL_USE_PAYLOAD" | $BINARY hook pre-tool-use >/dev/null 2>/dev/null
  echo "$POST_TOOL_USE_PAYLOAD" | $BINARY hook post-tool-use >/dev/null 2>/dev/null
  end=$(python3 -c 'import time; print(time.monotonic_ns())')
  enabled_total=$((enabled_total + end - start))
done
for ((i=0; i<50; i++)); do
  start=$(python3 -c 'import time; print(time.monotonic_ns())')
  echo "$PRE_TOOL_USE_PAYLOAD" | env FLOWFORGE_HOOKS_DISABLED=1 $BINARY hook pre-tool-use >/dev/null 2>/dev/null
  echo "$POST_TOOL_USE_PAYLOAD" | env FLOWFORGE_HOOKS_DISABLED=1 $BINARY hook post-tool-use >/dev/null 2>/dev/null
  end=$(python3 -c 'import time; print(time.monotonic_ns())')
  disabled_total=$((disabled_total + end - start))
done
python3 -c "
enabled = $enabled_total / 1_000_000
disabled = $disabled_total / 1_000_000
overhead = enabled - disabled
per_tool = overhead / 50
print(f'  Enabled (50 tools):   {enabled:8.0f}ms total')
print(f'  Disabled (50 tools):  {disabled:8.0f}ms total (process spawn only)')
print(f'  FlowForge overhead:   {overhead:8.0f}ms total  ({per_tool:.0f}ms per tool use)')
print(f'  Overhead ratio:       {enabled/disabled:.1f}x')
"
echo ""

echo "── 4. TOKEN INJECTION: UserPromptSubmit output size ──"
count_tokens "user-prompt-submit" "$PROMPT_SUBMIT_PAYLOAD" "With FlowForge"
count_tokens "user-prompt-submit" "$PROMPT_SUBMIT_PAYLOAD" "Kill-switch (disabled)"  "FLOWFORGE_HOOKS_DISABLED=1"

# Also test with a short prompt to verify KV gate
SHORT_PROMPT_PAYLOAD='{
  "session_id":"bench-session","transcript_path":"/tmp/bench.jsonl",
  "cwd":"/tmp","permission_mode":"bypassPermissions",
  "hook_event_name":"UserPromptSubmit",
  "prompt":"fix it"
}'
count_tokens "user-prompt-submit" "$SHORT_PROMPT_PAYLOAD"  "Short prompt (KV gated)"
echo ""

echo "── 5. SESSION LIFECYCLE OVERHEAD ──"
echo "  (SessionStart + 10 prompts + 50 tool uses + SessionEnd)"
start=$(python3 -c 'import time; print(time.monotonic_ns())')
echo "$SESSION_START_PAYLOAD" | $BINARY hook session-start >/dev/null 2>/dev/null
for ((i=0; i<10; i++)); do
  echo "$PROMPT_SUBMIT_PAYLOAD" | $BINARY hook user-prompt-submit >/dev/null 2>/dev/null
done
for ((i=0; i<50; i++)); do
  echo "$PRE_TOOL_USE_PAYLOAD" | $BINARY hook pre-tool-use >/dev/null 2>/dev/null
  echo "$POST_TOOL_USE_PAYLOAD" | $BINARY hook post-tool-use >/dev/null 2>/dev/null
done
echo "$SESSION_END_PAYLOAD" | $BINARY hook session-end >/dev/null 2>/dev/null
end=$(python3 -c 'import time; print(time.monotonic_ns())')
python3 -c "
total = ($end - $start) / 1_000_000
print(f'  Full session (enabled):  {total:8.0f}ms')
"

start=$(python3 -c 'import time; print(time.monotonic_ns())')
echo "$SESSION_START_PAYLOAD" | env FLOWFORGE_HOOKS_DISABLED=1 $BINARY hook session-start >/dev/null 2>/dev/null
for ((i=0; i<10; i++)); do
  echo "$PROMPT_SUBMIT_PAYLOAD" | env FLOWFORGE_HOOKS_DISABLED=1 $BINARY hook user-prompt-submit >/dev/null 2>/dev/null
done
for ((i=0; i<50; i++)); do
  echo "$PRE_TOOL_USE_PAYLOAD" | env FLOWFORGE_HOOKS_DISABLED=1 $BINARY hook pre-tool-use >/dev/null 2>/dev/null
  echo "$POST_TOOL_USE_PAYLOAD" | env FLOWFORGE_HOOKS_DISABLED=1 $BINARY hook post-tool-use >/dev/null 2>/dev/null
done
echo "$SESSION_END_PAYLOAD" | env FLOWFORGE_HOOKS_DISABLED=1 $BINARY hook session-end >/dev/null 2>/dev/null
end=$(python3 -c 'import time; print(time.monotonic_ns())')
python3 -c "
total = ($end - $start) / 1_000_000
print(f'  Full session (disabled): {total:8.0f}ms  (process spawn only)')
"
echo ""
echo "Done."
