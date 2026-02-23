#!/bin/bash
set -e

ADR_DIR=".plan/adr"

# Find the next ADR number
last_num=$(ls "$ADR_DIR"/*.md 2>/dev/null | grep -oP '\d{3}' | sort -n | tail -1)
if [ -z "$last_num" ]; then
    next_num="001"
else
    next_num=$(printf "%03d" $((10#$last_num + 1)))
fi

# Prompt for title
if [ $# -eq 0 ]; then
    read -p "Enter ADR title (e.g. 'use redis for caching'): " title
else
    title="$*"
fi

# Convert title to filename slug
slug=$(echo "$title" | tr '[:upper:]' '[:lower:]' | tr ' ' '-' | tr -cd 'a-z0-9-')

filename="${ADR_DIR}/${next_num}-${slug}.md"
date_str=$(date '+%Y-%m-%d')

cat > "$filename" << EOF
# ADR-${next_num}: ${title}

**Date:** ${date_str}
**Status:** Accepted

## Context

What is the issue that we're seeing that is motivating this decision or change?

## Decision

What is the change that we're proposing and/or doing?

## Consequences

What becomes easier or more difficult to do because of this change?
EOF

echo "Created: $filename"
echo "Edit the file to fill in the details."
