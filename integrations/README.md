# aw agent integrations

Everything an LLM agent needs to keep the cue board current lives here and is
managed by this repository. `./install.sh` (repo root) symlinks these files
into place, so editing them here updates every agent at once:

| Path | Installed as | Purpose |
|---|---|---|
| `claude-skill/` | `~/.claude/skills/aw` | Claude Code skill: when and how to run `aw` |
| `codex-skill/` | `~/.codex/skills/aw` | Codex skill (`$aw`), same rules adapted |
| `hooks/aw-session-start.sh` | `~/.claude/hooks/` | Injects the worktree's tasks + integration rules at session start; shared by Claude and Codex (pass `codex` as the first argument) |
| `hooks/aw-stop-reminder.sh` | `~/.claude/hooks/` | Blocks a stopping session once when its tasks are >30 min stale |

## Hook wiring (one-time, manual)

The hook scripts only run once they are registered. Add to
`~/.claude/settings.json`:

```json
"hooks": {
  "SessionStart": [
    { "matcher": "*", "hooks": [ { "type": "command",
      "command": "bash '<HOME>/.claude/hooks/aw-session-start.sh'", "timeout": 10 } ] }
  ],
  "Stop": [
    { "hooks": [ { "type": "command",
      "command": "bash '<HOME>/.claude/hooks/aw-stop-reminder.sh'", "timeout": 10 } ] }
  ]
}
```

For Codex, add the same two entries to `~/.codex/hooks.json`, with the
session-start command ending in `... aw-session-start.sh' codex` so the
injected instructions tell Codex to pass `--agent codex --session-id <id>`
explicitly (Claude sessions auto-detect via `CLAUDE_CODE_SESSION_ID`).

Merge into the existing arrays — both files may already carry hooks from
other tools.

## Design notes

- Session identity: `aw start`/`aw update` auto-detect the writing agent from
  `CLAUDE_CODE_SESSION_ID` / `CODEX_SESSION_ID`, with `AW_AGENT` /
  `AW_SESSION_ID` as the generic override for other tools.
- The stop reminder guards against hosts that do not set `stop_hook_active`
  with a 20-minute sentinel file (`$AW_HOME/.stop-reminder-stamp`), so it can
  never block in a loop.
- Cue style rules (natural Taiwanese Mandarin full sentences, no telegraphic
  compression) are duplicated into both skills and the session-start hook on
  purpose: the hook covers sessions where the skill never triggers.
