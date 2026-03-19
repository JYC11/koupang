#!/bin/bash
# Claude Code hook: logs user prompts to the memory folder
# Triggered by UserPromptSubmit event
# The user's prompt is passed via stdin
# Logs from any project with project name in the filename

MEMORY_DIR="$HOME/.claude/llm_usage_logging_folder"

# Derive project name from working directory
case "$PWD" in
    /Users/admin/Desktop/code/*)
        PROJECT=$(echo "$PWD" | sed 's|/Users/admin/Desktop/code/||' | cut -d/ -f1)
        ;;
    /Users/admin)
        PROJECT="global"
        ;;
    *)
        PROJECT=$(basename "$PWD")
        ;;
esac

LOG_FILE="$MEMORY_DIR/session-log-${PROJECT}-$(date +%Y-%m-%d).md"

mkdir -p "$MEMORY_DIR"

# Read prompt from stdin
PROMPT=$(cat)

# Skip system/command messages
if echo "$PROMPT" | grep -q '<local-command-'; then
    exit 0
fi
if echo "$PROMPT" | grep -q '<command-name>'; then
    exit 0
fi

# Skip empty prompts
if [ -z "$PROMPT" ]; then
    exit 0
fi

# Append to daily log
{
    echo ""
    echo "### $(date '+%H:%M:%S')"
    echo ""
    echo '```'
    echo "$PROMPT"
    echo '```'
    echo ""
} >> "$LOG_FILE"
