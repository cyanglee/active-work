#!/bin/bash
# Install the aw binary and wire the Claude Code / Codex integrations.
# Integrations are symlinked back into this repository, so `git pull`
# updates the skills and hooks everywhere at once.
set -euo pipefail

REPO="$(cd "$(dirname "$0")" && pwd)"

echo "==> Installing aw binary (cargo install)"
cargo install --path "$REPO"

link() {
  local src=$1 dst=$2
  if [ -e "$dst" ] && [ ! -L "$dst" ]; then
    local backup="$dst.bak.$(date +%Y%m%d%H%M%S)"
    mv "$dst" "$backup"
    echo "    backed up existing $dst -> $backup"
  fi
  mkdir -p "$(dirname "$dst")"
  ln -sfn "$src" "$dst"
  echo "    $dst -> $src"
}

echo "==> Linking Claude Code integration"
link "$REPO/integrations/claude-skill" "$HOME/.claude/skills/aw"
link "$REPO/integrations/hooks/aw-session-start.sh" "$HOME/.claude/hooks/aw-session-start.sh"
link "$REPO/integrations/hooks/aw-stop-reminder.sh" "$HOME/.claude/hooks/aw-stop-reminder.sh"
link "$REPO/integrations/hooks/aw-ping.sh" "$HOME/.claude/hooks/aw-ping.sh"

if [ -d "$HOME/.codex" ]; then
  echo "==> Linking Codex integration"
  link "$REPO/integrations/codex-skill" "$HOME/.codex/skills/aw"
else
  echo "==> Skipping Codex integration (~/.codex not found)"
fi

cat <<'EOF'

Done. One manual step remains if the hooks are not wired yet:
register the SessionStart and Stop hooks in ~/.claude/settings.json
(and ~/.codex/hooks.json for Codex). The exact JSON snippets are in
integrations/README.md.
EOF
