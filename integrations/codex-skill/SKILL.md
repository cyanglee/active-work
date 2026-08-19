---
name: aw
description: Maintain a concise Active Work cue for the current task with the local `aw` CLI. Use when the user explicitly invokes `$aw` to start or resume tracked work, inspect active work, update the current cue, mark work waiting or ready for review, or finish a tracked task.
---

# Active Work

Use `aw` as a cue board, not as a task tracker or event log. Keep only the
current state and next action.

## Route the request

- For a status or resume request, run `aw current` in the current worktree. Run
  `aw` only when the user asks for the global list.
- For work to perform, run `aw current --id-only` before changing files.
  Continue the existing task only when its cue matches the requested work.
- If no current task matches the work at hand, run `aw start` with a short
  title, one-sentence summary, and exact next action. Omit the ID so `aw`
  allocates one. Supply an ID only when the user explicitly provides an
  external task or issue ID. A worktree may hold several active tasks; never
  stuff unrelated progress into an existing task's cue — start a new task
  instead.

Example:

```bash
aw start \
  --project YourClinic \
  --title "Fix invoice rounding" \
  --summary "Starting investigation of the invoice total mismatch" \
  --next "Reproduce the mismatch with a focused test"
```

Capture the ID printed by `aw start`. Never invent an `AW-NNNN` ID; allocation
and collision prevention belong to the CLI.

## Record authorship so `aw resume` works

Every `aw start` / `aw update` should carry `--agent codex --session-id <id>`,
using the session ID injected by the SessionStart hook (the "[aw 工作看板]"
context block names it). This lets `aw resume <task-id>` print or launch
`codex resume <session-id>` later. If no session ID was injected, pass at
least `--agent codex`. (Claude Code sessions auto-detect via environment
variables; Codex must pass the flags explicitly.)

## Keep the cue useful

Perform the user's work normally. Before the final response, update the cue at
most once when the current state or next action changed:

```bash
aw update AW-0001 \
  --summary "Located rounding before tax aggregation" \
  --next "Move rounding to the final total and rerun the focused spec"
```

Use these states deliberately:

- `working`: more agent work remains.
- `waiting`: a concrete user decision or external dependency is required.
- `review`: implementation is ready for the user to inspect.
- `done`: the requested outcome is complete and verified; prefer `aw done`.

Keep `summary` and `next` to one sentence each. Record conclusions, not command
history. Do not include secrets, credentials, patient data, or other sensitive
content.

### Style (mandatory — the user reads cues on a dashboard)

Write summary/next as natural, complete Taiwanese Mandarin sentences, read
hours later by someone with no session context. Each must stand alone: what got
done / what to do next, and why, in plain words.

- FORBIDDEN: telegraphic compression, stacked-noun shorthand, invented
  abbreviations. Bad: 「照四型分型改寫其餘八卡並更新 STYLE.md」. Good:
  「patients.md 的版型定案後，把其餘八張功能卡改成同一種格式，並同步更新
  STYLE.md」.
- "One sentence" means one clear sentence, not maximum compression — a clear
  25-character sentence beats a cryptic 12-character one every time.
- Full-width punctuation for Chinese text; English terms sit inline with
  half-width spaces around them (「更新 STYLE.md 並跑 specs」).

For a blocker:

```bash
aw update AW-0001 \
  --state waiting \
  --summary "Implementation is blocked on the overlap policy" \
  --next "Wait for the user to choose whether overlaps are allowed"
```

The `--ask` trigger is EXPLICIT. You MUST set it before ending a turn whose
closing is one of: (a) an options question to the user, (b) alternatives laid
out for the user to pick, (c) wording like 「等你確認」「要嗎」「跟我說一聲」.
Otherwise do NOT set it. Record WHAT is being asked (one or two sentences,
options included, same mandatory style); clear it with `--ask ""` once the
user decides. `aw done` clears it too.

```bash
aw update AW-0001 --ask "重疊時段要直接擋下，還是允許但顯示警告？擋下較安全，警告較彈性。"
```

For completion:

```bash
aw done AW-0001 --summary "Implemented and verified invoice rounding"
```

If an `aw` command fails, do not silently claim that the cue was updated.
Continue safe task work when possible and report the cue failure briefly.

## Preserve isolation

Different agents and LLMs may share the same `~/.agent-work` store safely
because `aw` allocates IDs under a file lock and writes each task atomically.
Do not set a different `AW_HOME` unless the user explicitly requests an
isolated store. A worktree may hold several active tasks (`aw current` lists
them all); keep each task's cue scoped to its own piece of work.

If `aw` is unavailable, report that it must be installed from
`/Users/take5/workspace/playground/active-work` with `cargo install --path .`.
