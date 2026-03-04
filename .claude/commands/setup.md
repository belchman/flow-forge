---
allowed-tools: Bash
---

# FlowForge Setup

Guide through project initialization step by step:

1. **Build**: `cargo build --release`
2. **Install**: `rm -f ~/.cargo/bin/flowforge && cp target/release/flowforge ~/.cargo/bin/flowforge`
3. **Initialize**: `flowforge init --project`
4. **Verify hooks**: `flowforge test-hooks`
5. **Verify binary**: `flowforge --version`

Run each step sequentially. If any step fails, stop and diagnose the issue before proceeding. The `rm` before `cp` is critical — macOS caches in-place overwrites, causing stale binaries.
