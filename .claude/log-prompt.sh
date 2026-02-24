#!/bin/bash
# Claude Code hook: logs user prompts to the memory folder
# Triggered by UserPromptSubmit event
# The user's prompt is passed via stdin
# Only logs when working in the Koupang project

# Only run if current directory is within the koupang project
case "$PWD" in
    /Users/admin/Desktop/code/koupang*) ;;
    *) exit 0 ;;
esac

MEMORY_DIR="$HOME/.claude/projects/-Users-admin-Desktop-code-koupang/memory/llm_usage_logging_folder"
LOG_FILE="$MEMORY_DIR/session-log-$(date +%Y-%m-%d).md"

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
