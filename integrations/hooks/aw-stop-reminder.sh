#!/bin/bash
# Stop hook: if this worktree has active aw tasks that have not been updated
# in the last 30 minutes, ask Claude once to refresh the stale ones before
# stopping. A worktree may hold several active tasks; all are checked.
input=$(cat)
printf '%s' "$input" | jq -e '.stop_hook_active == true' >/dev/null 2>&1 && exit 0

AW_BIN=$(command -v aw 2>/dev/null)
[ -z "$AW_BIN" ] && [ -x "$HOME/.asdf/installs/rust/1.90.0/bin/aw" ] && AW_BIN="$HOME/.asdf/installs/rust/1.90.0/bin/aw"
[ -z "$AW_BIN" ] && exit 0

ids=$("$AW_BIN" current --id-only 2>/dev/null)
[ -z "$ids" ] && exit 0

tasks_dir="${AW_HOME:-$HOME/.agent-work}/tasks"
now=$(date +%s)
stale=""
for id in $ids; do
  task_file="$tasks_dir/${id}.json"
  [ -f "$task_file" ] || continue
  task_age=$(( now - $(stat -f %m "$task_file") ))
  [ "$task_age" -ge 1800 ] && stale="$stale $id"
done
stale="${stale# }"
[ -z "$stale" ] && exit 0

# Belt-and-braces loop guard for hosts that may not set stop_hook_active
# (e.g. Codex's hook compatibility layer): remind at most once per 20 minutes.
sentinel="${AW_HOME:-$HOME/.agent-work}/.stop-reminder-stamp"
if [ -f "$sentinel" ]; then
  sentinel_age=$(( now - $(stat -f %m "$sentinel") ))
  [ "$sentinel_age" -lt 1200 ] && exit 0
fi
touch "$sentinel"

jq -n --arg ids "$stale" '{decision:"block", reason:("這個 worktree 的 aw 任務 " + $ids + " 已超過 30 分鐘沒有更新。如果這個回合對其中某個任務有實質進展，請先對那個任務執行 aw update --summary 與 --next 再結束：用自然的台灣繁體中文完整句子（ELI5 風格），講清楚做到哪與下一步，禁止電報式壓縮。如果你這回合的結尾是在等使用者做決定（提出選項、等確認），順便把在等什麼寫進 --ask。這個回合沒動到的任務不要更新；全部都沒動到就直接結束。")}'
