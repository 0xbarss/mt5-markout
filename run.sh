#!/usr/bin/env bash
set -e

# Default environment variables
export MT5_PIPE_SECRET="${MT5_PIPE_SECRET:-test}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Build for Windows GNU target if needed
cargo build --target x86_64-pc-windows-gnu

# Ensure DLL is present next to the executable
cp -n "$SCRIPT_DIR/mt5_bridge.dll" "$SCRIPT_DIR/target/x86_64-pc-windows-gnu/debug/" 2>/dev/null || true

# Run under Wine with forwarded arguments
exec wine "$SCRIPT_DIR/target/x86_64-pc-windows-gnu/debug/mt5-markout.exe" "$@"
