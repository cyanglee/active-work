#!/bin/bash
# SessionStart hook: inject the aw (Active Work) cue for this worktree,
# plus the standing rules for registering and updating tasks.
# Usage: aw-session-start.sh [agent-name]   (default: claude)
# Shared by Claude Code (~/.claude/settings.json) and Codex (~/.codex/hooks.json).
AGENT="${1:-claude}"
input=$(cat 2>/dev/null)
HOOK_SESSION_ID=$(printf '%s' "$input" | jq -r '.session_id // empty' 2>/dev/null)

AW_BIN=$(command -v aw 2>/dev/null)
[ -z "$AW_BIN" ] && [ -x "$HOME/.asdf/installs/rust/1.90.0/bin/aw" ] && AW_BIN="$HOME/.asdf/installs/rust/1.90.0/bin/aw"
[ -z "$AW_BIN" ] && exit 0

current=$("$AW_BIN" current 2>/dev/null)
[ -z "$current" ] && current="（這個 worktree 目前沒有登記中的 aw 任務）"

extra=""
if [ "$AGENT" != "claude" ]; then
  if [ -n "$HOOK_SESSION_ID" ]; then
    extra="
5. 這個 session 的 agent 是 ${AGENT}、session ID 是 ${HOOK_SESSION_ID}。執行 aw start 或 aw update 時，請一併加上 \`--agent ${AGENT} --session-id ${HOOK_SESSION_ID}\`，這樣之後才能用 \`aw resume\` 接回這個對話。"
  else
    extra="
5. 這個 session 的 agent 是 ${AGENT}。執行 aw start 或 aw update 時請加上 \`--agent ${AGENT}\`；如果能取得自己的 session ID，也用 \`--session-id\` 一併記錄，之後才能用 \`aw resume\` 接回對話。"
  fi
fi

ctx="[aw 工作看板]
${current}

aw 整合規則（適用於每個 session）：
1. 如果這個 session 要做實質的開發工作，而上面沒有「符合這件工作」的任務，就用 \`aw start --project <專案名> --title <標題> --summary <目前狀況> --next <下一步>\` 登記一個新任務——同一個 worktree 可以同時有多個任務，不要把無關的內容硬塞進既有任務。純問答、找資料的對話不用登記。
2. 每完成一段有意義的工作、以及結束回合之前，用 \`aw update <ID> --summary <進度> --next <下一步>\` 更新。狀態真的改變才用 --state（working/waiting/review）或 aw done。
3. --ask（待決定）的觸發條件是明確的：回合結尾如果是（a）用選項式提問（如 AskUserQuestion）問使用者、（b）列出多個方案等使用者挑、或（c）明確寫了「等你確認」「要嗎」「跟我說一聲」這類等待決定的句子，就【必須】在結束前執行 \`aw update <ID> --ask <在等什麼決定、選項是什麼>\`（一兩句）。不符合這三種情況就不要寫。使用者決定之後、工作繼續時用 \`--ask ""\` 清掉。
4. summary 與 next 的寫作風格（重要）：用自然的台灣繁體中文完整句子，依照 ELI5 output style 的語氣，寫給幾小時後回來的自己看。一句話要講清楚做到哪、下一步做什麼、為什麼。禁止電報式壓縮句法、名詞堆疊、自創縮語（反例：「照四型分型改寫其餘八卡並更新」）。英文專有名詞前後留半形空白。${extra}"

jq -n --arg ctx "$ctx" '{hookSpecificOutput:{hookEventName:"SessionStart",additionalContext:$ctx}}'
