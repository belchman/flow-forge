#!/usr/bin/env bash
set -euo pipefail

# FlowForge Setup Script
# Builds and installs FlowForge globally. Run `flowforge init --project`
# separately in each project where you want to use FlowForge.
#
# Usage:
#   ./setup.sh              # Build and install
#   ./setup.sh --help       # Show help

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

for arg in "$@"; do
    case "$arg" in
        --help|-h)
            echo "FlowForge Setup"
            echo ""
            echo "Usage: ./setup.sh"
            echo ""
            echo "Builds FlowForge and installs the binary to ~/.cargo/bin."
            echo "After installing, cd to your project and run:"
            echo "  flowforge init --project"
            echo ""
            echo "Options:"
            echo "  -h, --help   Show this help"
            exit 0
            ;;
        *)
            echo "Unknown option: $arg"
            exit 1
            ;;
    esac
done

echo "==> Building FlowForge (release)..."
cargo build --release --manifest-path "$SCRIPT_DIR/Cargo.toml" 2>&1

echo ""
echo "==> Installing flowforge to ~/.cargo/bin..."
# Remove old binary first — macOS caches in-place overwrites, causing the
# new binary to hang.  rm + copy forces the OS to pick up the fresh build.
rm -f "$HOME/.cargo/bin/flowforge" 2>/dev/null || true
cargo install --path "$SCRIPT_DIR/crates/flowforge-cli" --force 2>&1

# Ensure ~/.cargo/bin is on PATH for the current shell
if ! command -v flowforge &>/dev/null; then
    # Try sourcing cargo env
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck disable=SC1091
        . "$HOME/.cargo/env"
    fi
fi

# Detect the user's shell profile
detect_shell_profile() {
    local shell_name
    shell_name="$(basename "${SHELL:-/bin/bash}")"
    case "$shell_name" in
        zsh)  echo "$HOME/.zshrc" ;;
        bash)
            if [ -f "$HOME/.bash_profile" ]; then
                echo "$HOME/.bash_profile"
            else
                echo "$HOME/.bashrc"
            fi
            ;;
        fish) echo "$HOME/.config/fish/config.fish" ;;
        *)    echo "$HOME/.profile" ;;
    esac
}

# Add cargo env sourcing to shell profile if not already present
ensure_path() {
    local profile
    profile="$(detect_shell_profile)"

    if [ -f "$profile" ] && grep -q '\.cargo/env' "$profile" 2>/dev/null; then
        return 0  # Already configured
    fi

    echo ""
    echo "==> Adding ~/.cargo/bin to PATH in $profile..."
    echo "" >> "$profile"
    echo '# Added by FlowForge setup' >> "$profile"
    echo '. "$HOME/.cargo/env"' >> "$profile"
    echo "    Added: . \"\$HOME/.cargo/env\" to $profile"

    # Source it now so the rest of the script works
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck disable=SC1091
        . "$HOME/.cargo/env"
    fi
}

# If flowforge still isn't found, fix the PATH
if ! command -v flowforge &>/dev/null; then
    ensure_path
fi

# Final check
if ! command -v flowforge &>/dev/null; then
    echo ""
    echo "WARNING: flowforge is still not on your PATH."
    echo "Manually add this to your shell profile:"
    echo '  . "$HOME/.cargo/env"'
    echo ""
    echo "Then restart your terminal and run:"
    echo "  cd /your/project && flowforge init --project"
    exit 1
fi

echo ""
echo "==> Installed: $(which flowforge)"
echo "    Version:   $(flowforge --version)"

echo ""
echo "============================================"
echo " FlowForge is installed!"
echo "============================================"
echo ""
echo "To set up a project, cd to your project directory and run:"
echo ""
echo "  cd /path/to/your/project"
echo "  flowforge init --project"
echo ""
echo "This will create:"
echo "  - .flowforge/config.toml  (project config)"
echo "  - .flowforge/flowforge.db (SQLite database)"
echo "  - .claude/settings.json   (Claude Code hooks)"
echo "  - .mcp.json               (MCP server auto-registration)"
echo "  - CLAUDE.md               (agent instructions)"
echo ""
echo "Quick start (after init):"
echo "  flowforge agent list              # See 60+ built-in agents"
echo "  flowforge route \"<task>\"           # Get agent suggestions"
echo "  flowforge work create --type task --title \"My task\""
echo "  flowforge mcp serve               # Start MCP server (auto via .mcp.json)"
echo ""
echo "Start a new Claude Code session to activate hooks."
