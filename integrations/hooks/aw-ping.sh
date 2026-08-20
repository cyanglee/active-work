#!/bin/bash
# PostToolUse hook: forward the hook JSON on stdin to `aw ping`, which
# records a work heartbeat for the task this conversation is writing.
# Shared by Claude Code and Codex.
AW_BIN=$(command -v aw 2>/dev/null)
[ -z "$AW_BIN" ] && [ -x "$HOME/.asdf/installs/rust/1.90.0/bin/aw" ] && AW_BIN="$HOME/.asdf/installs/rust/1.90.0/bin/aw"
[ -z "$AW_BIN" ] && exec cat >/dev/null
exec "$AW_BIN" ping
