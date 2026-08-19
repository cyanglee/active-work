---
name: aw
description: >
  Interact with the `aw` (Active Work) CLI — the user's local cue board that
  answers "what am I working on, where is each task, what's the next action".
  Use this skill whenever the user asks what they are working on (我在做什麼 /
  現在有哪些工作 / active work / 手上的任務), wants to start tracking a piece of
  work (開始做 X 幫我記錄 / start tracking / aw start), wants to update progress
  or state (更新進度 / 記錄一下 / 卡住了等回覆 / 進 review), or finishes a task
  (做完了 / mark done). Also use it proactively: when starting substantial work
  in a project, register the task with `aw start`; before ending a meaningful
  work turn, refresh the cue with `aw update`. NOT for billed hours, timesheets,
  or session time reports — that is kilok / time-tracker, not aw.
---

# aw — Active Work cue board

`aw` keeps one JSON snapshot per task (default `~/.agent-work/tasks/`,
overridable with `AW_HOME`). It is a cue board, not an event log: each
`aw update` replaces the task's current summary/next — history is not kept, so
write cues that stand on their own.

Binary is on PATH (`aw`). Source lives at
`/Users/take5/workspace/playground/active-work` if behavior questions come up.

## Core rule: run from the project directory

`aw start` and `aw update` capture git directory, branch, and dirty state from
the cwd. Always `cd` to (or run within) the project being worked on, not $HOME
or a scratch dir — otherwise the task binds to the wrong worktree and
`aw current` won't find it later.

A worktree may hold several active tasks at once. `aw current` lists all of
them (`--id-only` prints one ID per line, newest first). Register a NEW task
when the work at hand does not match any existing task in this worktree —
never stuff unrelated progress into an existing task's cue. When several tasks
are active, `aw resume` and update flows need an explicit task ID.

## Commands

```bash
# Read the board (bare `aw` = `aw list`; done tasks hidden without --all)
aw                       # active tasks
aw list --all            # includes done

# Resolve the task bound to the current worktree
aw current               # full card; exit 1 + stderr message if none
aw current --id-only     # just the ID — use this in scripts/checks

# Start (ID auto-allocated as AW-NNNN unless one is supplied)
aw start --project ClinicBase --title "Fix invoice rounding" \
  --summary "Reproduced the bug" --next "Inspect the calculator"
aw start CB-142 --title "..."   # manual ID; fails if it already exists

# Update the cue (any subset of flags)
aw update AW-0001 --summary "Found rounding point" --next "Run invoice specs"
aw update AW-0001 --state review --next "Review the diff"

# Pending decision — the trigger is EXPLICIT. You MUST set --ask before
# ending a turn whose closing is one of: (a) an options question to the user
# (e.g. AskUserQuestion), (b) alternatives laid out for the user to pick,
# (c) wording like 「等你確認」「要嗎」「跟我說一聲」. Otherwise do NOT set it.
# Record WHAT is being asked (one or two sentences, options included).
aw update AW-0001 --ask "資料表要沿用 legacy 欄位還是開新表？沿用省時間，新表乾淨。"
aw update AW-0001 --ask ""   # cleared after the user decides (aw done also clears)

# Finish
aw done AW-0001 --summary "Verified and complete"

# Resume the conversation that recorded a task (agent + session are
# auto-detected from the environment on start/update — CLAUDE_CODE_SESSION_ID
# for claude, CODEX_SESSION_ID for codex, AW_AGENT/AW_SESSION_ID as override)
aw resume AW-0001          # prints e.g. `cd '<dir>' && claude --resume <session>`
aw resume AW-0001 --exec   # launches it directly in the task's worktree
```

States: `working` (default), `waiting` (blocked on something external),
`review` (awaiting review), `done`. Change state only when the task actually
changes state — routine progress is just `--summary`/`--next`.

## Writing good cues

- `--summary`: what is true NOW ("Found the rounding point in
  InvoiceCalculator#total"), not a diary entry.
- `--next`: the concrete next physical action ("Run invoice specs"),
  not a goal ("finish the feature"). The board's whole value is that the next
  action is executable at a glance.
- Keep the user's language: cues the user dictates in Chinese stay in Chinese.

### Style (mandatory — the user reads these on a dashboard)

Write summary/next as natural, complete Taiwanese Mandarin sentences, following
the user's ELI5 output style. They are read hours later by someone with no
session context, so each must stand alone: what got done / what to do next, and
why, in plain words.

- FORBIDDEN: telegraphic compression, stacked-noun shorthand, invented
  abbreviations. Bad: 「照四型分型改寫其餘八卡並更新 STYLE.md」. Good:
  「patients.md 的版型定案後，把其餘八張功能卡改成同一種格式，並同步更新
  STYLE.md」.
- "One line" means one sentence-length line, not maximum compression — a clear
  25-character sentence beats a cryptic 12-character one every time.
- English terms sit inline with half-width spaces around them
  (「更新 STYLE.md 並跑 specs」), full-width punctuation for Chinese text.

## Typical flows

**User asks what they're working on** → run `aw` (or `aw list --all` if they
ask about finished work too) and present the board. The output is already
compact; relay it, don't re-tabulate.

**Starting a piece of work** → `cd` to the project, check
`aw current --id-only` first:
- No active task → `aw start` with project, title, and an initial
  summary/next.
- A task whose cue MATCHES this work is active → `aw update` that task.
- Only unrelated tasks are active → `aw start` a new one; multiple active
  tasks per worktree are allowed.

**During/after a work session** → before ending a meaningful work turn, run
`aw update <id>` with a fresh one-line `--summary` and `--next`. This is the
whole point of the tool: the board should always answer "where was I?".

**Blocked / handed off** → `--state waiting` (waiting on external input) or
`--state review` (diff out for review), with `--next` saying what unblocks it.

**Done** → `aw done <id> --summary "<outcome>"`. Done tasks disappear from the
default listing.

## Failure modes (all exit 1 with a message on stderr)

- `no active task in <dir>` — nothing bound to this worktree; start one or the
  user asked in the wrong directory.
- `multiple active tasks in this worktree: ...` — from `aw resume` without an
  ID; pass the task ID explicitly.
- `task X already exists` — manual ID collision on `aw start`; use `aw update`.
- `task X does not exist` — typo'd ID on update/done; run `aw list --all` to
  find the right one.

## Programmatic access

Task files are plain JSON at `$AW_HOME/tasks/<ID>.json` (default
`~/.agent-work/tasks/`) with fields: id, project, title, state, summary, next,
ask (pending user decision, empty when none), updated_at (UTC ISO-8601),
directory, branch, dirty, agent + session_id (most recent writer), and
sessions (every conversation that wrote the task: agent, session_id,
last_seen). Read them with `jq` when a script needs structured data; use the
CLI for everything else.
