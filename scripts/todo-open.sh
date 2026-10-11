#!/usr/bin/env bash
# Source is uchmk/ito scripts/lanes/; an app's copy is written by its
# scripts/lanes.sh sync, so edit ito's.
# How many open TODO.md items the development session may take: `- [ ]`
# items with none of the marks scripts/lanes.conf's `todo_marks` names (in kura
# 【人】 【QA】 【実機】 【後】 【pane】 and 要確認) anywhere in them (an item's
# mark can sit on a continuation line). Prints the count alone.
# A mark inside backticks is a mention, not a mark: "if so, add `【pane】`"
# hid four takeable items from 2026-10-08 to 2026-10-09 and the routine
# ended every run with ALL_DONE.
#
#   scripts/todo-open.sh          12
#   scripts/todo-open.sh -v       the items too, one per line with its line number
#   scripts/todo-open.sh --qa     the 【QA】 items instead (no other mark), which
#   scripts/todo-open.sh --qa -v  the routine takes once the first count is 0
#
# The development routine ends with ALL_DONE only when both say 0
# (.claude/dev-routine.md). On 2026-10-05 a session judged "what is left is
# mostly for the machine or the eye" with 70 such items open, and stopped.

set -euo pipefail
cd "$(dirname "$0")/.."

python3 - "$@" <<'PY'
import re, sys

conf = dict(l.split("=", 1) for l in open("scripts/lanes.conf", encoding="utf-8").read().splitlines()
            if "=" in l and not l.startswith("#"))
MARKS = tuple(conf["todo_marks"].split())
qa = "--qa" in sys.argv[1:]
lines = open("TODO.md", encoding="utf-8").read().split("\n")
items, i = [], 0
while i < len(lines):
    m = re.match(r"^(\s*)- \[ \] ", lines[i])
    if not m:
        i += 1
        continue
    indent, start, text = len(m.group(1)), i, [lines[i]]
    i += 1
    # The item goes on while lines are indented deeper and are not an item.
    while i < len(lines) and lines[i].strip() and not re.match(r"^\s*- \[", lines[i]) \
            and len(lines[i]) - len(lines[i].lstrip()) > indent:
        text.append(lines[i])
        i += 1
    marks = {mark for mark in MARKS if mark in re.sub(r"`[^`]*`", "", " ".join(text))}
    if marks == ({"【QA】"} if qa else set()):
        items.append((start + 1, lines[start].strip()))

print(len(items))
if "-v" in sys.argv[1:]:
    for n, first in items:
        print(f"TODO.md:{n}: {first[:160]}")
PY
