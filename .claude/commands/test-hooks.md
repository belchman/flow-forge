---
allowed-tools: Bash
---

# FlowForge Hook Test

Run `flowforge test-hooks $ARGUMENTS` to test Claude Code hook integration.

If no arguments are provided, test all hook events. If arguments are provided, use them as an event filter (e.g., `PreToolUse`, `PostToolUse`, `SessionStart`).

After running:
- If all hooks pass, report success with a summary
- If any hooks fail, diagnose the issue:
  - Check if the binary exists: `which flowforge`
  - Check for hook errors: `cat .flowforge/hook-errors.log 2>/dev/null`
  - Suggest rebuilding: `cargo build --release && rm -f ~/.cargo/bin/flowforge && cp target/release/flowforge ~/.cargo/bin/flowforge`
