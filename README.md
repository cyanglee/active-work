# Active Work (`aw`)

`aw` is a small local cue board. It answers three questions:

- What am I working on?
- Where is each task now?
- What is the next action?

It is intentionally not a task tracker or an event log. Each task is one JSON
snapshot that gets replaced when its cue changes.

## Install

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
```

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

Use `aw current --id-only` to resolve the one active task associated with the
current Git worktree. `aw` intentionally allows only one active task per
worktree; use another worktree when another writing agent needs a separate task.

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
