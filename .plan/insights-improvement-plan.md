# Insights Improvement Plan

Source: Claude Code `/insights` report (2026-02-23 to 2026-02-28, 73 sessions)
Implemented: 2026-03-01

---

## Part A: General Improvements — ALL DONE

| Item | What | Where |
|------|------|-------|
| A1 | Minimize exploration, proceed to implementation | `~/.claude/CLAUDE.md` |
| A2 | Tests gate completion | `~/.claude/CLAUDE.md` |
| A3 | Plans go to files | `~/.claude/CLAUDE.md` |
| A4 | Sub-agents return full content | `~/.claude/CLAUDE.md` |
| A5 | `/handoff` skill for session summaries | `~/.claude/skills/handoff/SKILL.md` |
| A6 | Post-build `cargo check` hook (Rust projects) | `koupang/.claude/settings.json` |

## Part B: Project-Specific Improvements — ALL DONE

| Item | What | Resolution |
|------|------|------------|
| B1 | Rust pitfalls checklist | Skipped — covered by `rust-skills` plugin |
| B2 | Exploration budget | Covered by A1 in global CLAUDE.md |
| B3 | Module layout pattern | Covered by `/implement` skill |

## Bonus: Project Skills Created

Converted frequently-read `.plan/` docs into auto-triggering skills:

| Skill | Replaces | Triggers on |
|-------|----------|-------------|
| `/bootstrap` | `.plan/bootstrap-recipe.md` (deleted) | "new service", "scaffold" |
| `/implement` | `.plan/patterns.md` (deleted) | "add endpoint", "new module" |
| `/test-guide` | `.plan/test-standards.md` (deleted) | "write tests", "test strategy" |
