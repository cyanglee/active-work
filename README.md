# Active Work (`aw`)

`aw` is a small local cue board. It answers three questions:

- What am I working on?
- Where is each task now?
- What is the next action?

It is intentionally not a task tracker or an event log. Each task is one JSON
snapshot that gets replaced when its cue changes.

## Install

```bash
./install.sh
```

This installs the `aw` binary (`cargo install`) and symlinks the Claude Code /
Codex integrations (skills + hooks) from `integrations/` into place, so a
`git pull` here updates them everywhere. One manual step — registering the two
hooks — is documented in `integrations/README.md`. Binary only:

```bash
cargo install --path .
```

## Use

```bash
aw start \
  --project ClinicBase \
  --title "Fix invoice rounding" \
  --summary "Reproduced the bug" \
  --next "Inspect the calculator"

aw update CB-142 \
  --summary "Found the rounding point" \
  --next "Run invoice specs"

aw update CB-142 --state review --next "Review the diff"

aw

aw done CB-142 --summary "Verified and complete"

aw list --all

aw time                  # active time per task, merged from heartbeats
aw time --month 2026-08  # one month only (local time)
```

Active time comes from heartbeats: the PostToolUse hooks (see
`integrations/`) run `aw ping` on every agent tool call, attributed to the
task the conversation is writing via its session ID. Heartbeats closer than
5 minutes merge into continuous work; larger gaps count as idle. Storage is
plain local files under `$AW_HOME/heartbeats/`.

## Live views

Two ways to keep the board on screen while agents work:

```bash
aw serve            # web board on http://127.0.0.1:7337 (--port to change)
aw watch            # terminal board, redrawn every 2s (--interval, --all)
```

`aw serve` binds localhost only and serves a single self-contained page that
polls every two seconds: tasks grouped by project, state-colored cards, the
next action, branch and dirty state, relative update times, and a "show done"
toggle. Cards flash briefly when their cue changes.

`aw watch` renders the same grouping with ANSI colors — green working, yellow
waiting, magenta review — and hides done tasks unless `--all` is supplied.

When no ID is supplied, `aw start` allocates a local, human-readable ID such as
`AW-0001`. Allocation is serialized with a file lock, so concurrent agents that
share the same `AW_HOME` cannot receive the same ID. A manual ID such as
`CB-142` is still accepted and can never overwrite an existing task.

Use `aw current` to see the active tasks bound to the current Git worktree
(`--id-only` prints one ID per line, newest first). A worktree may hold
several active tasks at once — register one task per piece of work and keep
each cue scoped to its own task.

By default, task files live in `~/.agent-work/tasks`. Set `AW_HOME` to use a
different location. Git directory, branch, and dirty state are captured from
the directory where `aw start` or `aw update` runs.

## Agent integration

The first integration can be a single instruction instead of a complex hook:

> Before finishing a meaningful work turn, run `aw update <task-id>` with a
> one-line `--summary` and one-line `--next`. Use `--state waiting`,
> `--state review`, or `aw done` only when the task actually changes state.

## Test

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```
