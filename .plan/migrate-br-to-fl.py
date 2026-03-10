#!/usr/bin/env python3
"""Migrate br (beads) tasks to filament (fl)."""
import json
import subprocess
import sys

def run(cmd, check=True):
    """Run a command, return stdout."""
    r = subprocess.run(cmd, capture_output=True, text=True)
    if check and r.returncode != 0:
        print(f"FAIL: {' '.join(cmd)}\n  {r.stderr.strip()}", file=sys.stderr)
        return None
    return r.stdout.strip()

def fl_task_add(title, summary, priority, task_type="task"):
    """Create a filament task, return slug."""
    fl_type = "doc" if task_type == "docs" else "task"
    cmd = ["fl", "task", "add", title, "--summary", summary or title, "--priority", str(priority), "--json"]
    if fl_type == "doc":
        # docs go as regular entities, not tasks
        cmd = ["fl", "add", title, "--type", "doc", "--summary", summary or title, "--json"]
    out = run(cmd)
    if not out:
        return None
    try:
        data = json.loads(out)
        # fl task add --json returns the slug in different formats
        if isinstance(data, dict):
            return data.get("slug") or data.get("id")
        return None
    except json.JSONDecodeError:
        # Try to parse slug from text output
        for line in out.split("\n"):
            if "slug" in line.lower() or len(line.strip()) == 8:
                return line.strip().split()[-1]
        print(f"  Could not parse slug from: {out[:100]}", file=sys.stderr)
        return None

# ── Load br tasks ──
print("Loading br tasks...")
raw = run(["br", "list", "--json"])
tasks = json.loads(raw)
print(f"  {len(tasks)} open tasks")

# ── Load br dependencies ──
print("Loading br dependencies...")
deps = []  # (child_br_id, parent_br_id) — parent blocks child
open_ids = {t["id"] for t in tasks}

for t in tasks:
    tid = t["id"]
    out = run(["br", "dep", "list", tid], check=False)
    if not out:
        continue
    for line in out.split("\n"):
        line = line.strip()
        if line.startswith("-> "):
            # Parse: "-> bd-xxx (blocks): Title" or "-> bd-xxx (parent-child): Title"
            parts = line[3:].split(" ", 1)
            parent_id = parts[0]
            if parent_id in open_ids:
                deps.append((tid, parent_id))  # parent blocks child

print(f"  {len(deps)} active dependencies (between open tasks)")

# ── Create filament tasks ──
print("\nCreating filament entities...")
br_to_fl = {}  # br_id -> fl_slug
errors = []

for t in tasks:
    br_id = t["id"]
    title = t["title"]
    desc = t.get("description", "") or ""
    priority = t["priority"]
    issue_type = t["issue_type"]

    # Include br_id in summary for traceability
    summary = f"[{br_id}] {desc}" if desc else f"[{br_id}] {title}"

    slug = fl_task_add(title, summary, priority, issue_type)
    if slug:
        br_to_fl[br_id] = slug
        print(f"  {br_id} -> {slug}: {title[:60]}")
    else:
        errors.append(br_id)
        print(f"  FAILED: {br_id}: {title[:60]}")

print(f"\nCreated {len(br_to_fl)}/{len(tasks)} entities")
if errors:
    print(f"  Errors: {errors}")

# ── Create dependency relations ──
print("\nCreating dependency relations...")
dep_ok = 0
dep_fail = 0

for child_br, parent_br in deps:
    child_fl = br_to_fl.get(child_br)
    parent_fl = br_to_fl.get(parent_br)
    if not child_fl or not parent_fl:
        print(f"  SKIP: {parent_br} blocks {child_br} (missing slug)")
        dep_fail += 1
        continue

    # In filament: "A blocks B" means B cannot start until A closes
    out = run(["fl", "relate", parent_fl, "blocks", child_fl], check=False)
    if out is not None:
        dep_ok += 1
    else:
        dep_fail += 1

print(f"  Created {dep_ok} relations, {dep_fail} failures")

# ── Save mapping for reference ──
mapping_path = ".plan/br-to-fl-mapping.json"
with open(mapping_path, "w") as f:
    json.dump(br_to_fl, f, indent=2)
print(f"\nMapping saved to {mapping_path}")

# ── Summary ──
print(f"""
Migration complete:
  Tasks migrated: {len(br_to_fl)}/{len(tasks)}
  Dependencies:   {dep_ok}/{len(deps)}
  Mapping file:   {mapping_path}
""")
