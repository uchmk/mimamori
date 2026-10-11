#!/usr/bin/env python3
# Source is uchmk/ito scripts/lanes/; an app's copy is written by its
# scripts/lanes.sh sync, so edit ito's.
"""Merge the real-machine lanes' pull requests that keep to the rules.

Run by .github/workflows/merge-lanes.yml. It merges a pull request from a lane
branch (test/win-*, in tsumugi test/arm-*, in kura test/linux-*) with a merge commit
pinned to its head, and only when all of these hold (yagura has no lanes: there
it only keeps the labels and counts the votes):

  1. it touches only the files a lane may write: the checklists and one new
     report under qa-reports/ (kura), or the checklists and new files under
     qa-reports/ (tsumugi);
  2. every changed checklist line is the same line with only its mark changed,
     from `[ ]` to a mark that lane may set;
  3. every new mark has its evidence: a line naming the row (or the key) in
     the pull request body -- in tsumugi the new report counts too -- and a
     `[~]` names its picture;
  4. kura: TODO.md's "マージで止めている実機の PR" does not hold it;
  5. every check run on its head has finished, none failed, and the
     checklist job is among them;
  6. GitHub says it merges without a conflict.

A pull request that keeps to 1-4 but conflicts with the base, and has no
red check, is merged here by hand (git, in the workflow's checkout of main):
when only checklist files conflict, each one is main's file with the pull
request's mark changes made again on the rows main still has unticked and
unchanged. The result is checked to be marks and new reports only, then
pushed to main; GitHub then counts the pull request as merged. A mark whose
row main reworded is dropped and named in a comment. Any other conflict is
left alone.

Anything else is left alone. A rule that fails, a red check or a conflict
gets one comment per head (a hidden marker keeps it from repeating), which
the merge routine reads (.claude/merge-routine.md).

It also does the merger's share for lane pull requests merged in the last 7
days whose `#N` is not in CHANGELOG.md: a PATCH bump (Cargo.toml and
Cargo.lock) and a CHANGELOG line per pull request. The reports' proposals,
queue notes and votes go to issues (kura since v0.102.0, tsumugi since
v0.94.0): a vote `- #N: option — why` on an open question issue as a comment
there, the rest as one finding issue per pull request (a hidden
`<!-- kura-lane:#N -->`, named after the repository, keeps it from
repeating), and the pull request gets the label lane-review, which the merge
routine takes off once it has read the report. (A rule with share.role and
todo_heading set takes the ticked rows out of the role's re-test list and
lists the leftovers in TODO.md instead: tsumugi's way before v0.94.0.)

It also keeps the labels as LABELS says (each repository the ones its rule
names; `--labels` does only that), and counts the votes on the open question
issues: `vote[cloud]:` and `vote[win]:` comments, the newest per voter, from
the owner's account (or relayed here). Both on one option: vote-decided;
apart: needs-owner; an owner's `回答:` makes it answered whatever the votes
say.

Last, it watches the lanes' status issues ("Lane status: <lane>", label
lane-status, written by the lane's machine at every firing): one whose last
firing is more than three hours old (five while a run is going) gets the label
lane-stalled and one comment, and loses the label when the lane writes again.

It reads the pull request through the API and git, never runs its code, and
runs only main's copy of this script.

    python3 scripts/merge-lanes.py              # GITHUB_TOKEN, GITHUB_REPOSITORY
    MERGE_LANES_DRY_RUN=1 python3 scripts/merge-lanes.py
    python3 scripts/merge-lanes.py --self-test
    python3 scripts/merge-lanes.py --labels     # what scripts/labels.sh runs

Every app runs the same bytes (the source is uchmk/ito scripts/lanes/); the
repository picks its rules from RULES.
"""

import base64
import datetime
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

API = os.environ.get("GITHUB_API_URL", "https://api.github.com")
REPO = os.environ.get("GITHUB_REPOSITORY", "")
TOKEN = os.environ.get("GITHUB_TOKEN", "")
DRY = os.environ.get("MERGE_LANES_DRY_RUN", "") not in ("", "0", "false")

WINDOWS_MARKS = {
    "TESTING-CHECKS.md": {(" ", "x"), (" ", "~")},
    "TESTING-KEYS.md": {(" ", "x")},
}
# tsumugi's keys may also be looked at: a key whose only effect is on screen
# gets `[~]` with its picture, as a checklist row does (owner, 2026-10-10)
TSUMUGI_WINDOWS_MARKS = {
    "TESTING-CHECKS.md": {(" ", "x"), (" ", "~")},
    "TESTING-KEYS.md": {(" ", "x"), (" ", "~")},
}

RULES = {
    "uchmk/kura": {
        "lanes": {
            "test/win-": WINDOWS_MARKS,
            # no test/arm- since v0.101.3: the ARM64 machine ticks its release
            # rows in the release-arm64 issue and opens no pull request
            "test/linux-": {"TESTING-LINUX.md": {(" ", "x"), (" ", "-")}},
        },
        # a lane watched for a stall only while an issue with this label is
        # open: the ARM64 machine fires only once per release
        "watch_when": {"arm": "release-arm64"},
        # exactly one new report: another run's file is not this run's to edit
        "reports": (1, 1),
        "evidence_in_report": False,
        "required_checks": ["checklists"],
        "hold_heading": "マージで止めている実機の PR",
        # since v0.102.0 the share writes no TODO.md lines: a report's
        # leftovers become one finding issue, its votes comments on the
        # question issues, and the merged pull request gets lane-review
        "share": {
            "role": None,
            "todo_heading": None,
            "todo_mark": "",
            "issues": True,
            "dispatch": ["ci.yml"],
        },
        # labels kept as LABELS says (True: all of them, or a list of names),
        # and question issues' votes counted
        "labels": True,
        "votes": True,
    },
    "uchmk/tsumugi": {
        "lanes": {"test/win-": TSUMUGI_WINDOWS_MARKS, "test/arm-": TSUMUGI_WINDOWS_MARKS},
        "reports": (0, 99),
        "evidence_in_report": True,
        "required_checks": ["check"],
        "hold_heading": None,
        # since v0.94.0 as kura's: re-tests come from retest issues, which the
        # lane's pull request closes, and the leftovers go to issues
        "share": {
            "role": None,
            "todo_heading": None,
            "todo_mark": "",
            "issues": True,
            # a push with the workflow's token starts no workflows, except these
            "dispatch": ["ci.yml", "checklists.yml"],
        },
        "labels": True,
        "votes": True,
    },
    # no lanes (its real-machine checks are TESTING.md, by hand): the labels
    # the issue flow uses and the question issues' votes only
    "uchmk/yagura": {
        "lanes": {},
        "reports": (0, 0),
        "evidence_in_report": False,
        "required_checks": [],
        "hold_heading": None,
        "share": None,
        "labels": ["bug", "finding", "question", "lane:cloud", "lane:qa",
                   "vote", "vote-decided", "needs-owner", "answered"],
        "votes": True,
    },
}

OK_CONCLUSIONS = {"success", "skipped", "neutral"}
MARK_LINE = re.compile(r"^- \[(.)\] (.*)$")
PICTURE = re.compile(r"\.(png|jpe?g|bmp|gif|webp)\b", re.IGNORECASE)
# a shot's bare name: letters, digits, `_` and `-`, no path or extension
PICTURE_NAME = re.compile(r"[A-Za-z][\w-]{1,63}")
MARKER = "<!-- merge-lanes:{kind}:{sha} -->"
MACHINES = {"test/win-": "x64", "test/arm-": "ARM64", "test/linux-": "Linux"}
RETESTS = "| **Re-tests of changed behaviour** |"
BOT = ["-c", "user.name=github-actions[bot]",
       "-c", "user.email=41898282+github-actions[bot]@users.noreply.github.com"]


class RuleError(Exception):
    pass


# ---------------------------------------------------------------- pure parts


def mark_pairs(patch):
    """The (old, new) line pairs of a unified diff that only replaces lines.

    Raises RuleError when a group of removed lines is not matched by as many
    added lines (a line added or removed, not changed)."""
    pairs, removed, added = [], [], []

    def flush():
        if len(removed) != len(added):
            raise RuleError(
                f"{len(removed)} line(s) removed and {len(added)} added in one place"
                " -- only changed marks may differ")
        pairs.extend(zip(removed, added))
        removed.clear()
        added.clear()

    for line in patch.split("\n"):
        if line.startswith("\\"):  # "\ No newline at end of file"
            continue
        if line.startswith("-"):
            if added:
                flush()
            removed.append(line[1:])
        elif line.startswith("+"):
            added.append(line[1:])
        else:  # a hunk header or a context line
            flush()
    flush()
    return pairs


def new_marks(path, patch, allowed):
    """[(mark, rest of the line)] for each line the patch re-marks.

    Raises RuleError for any change that is not one allowed mark change."""
    out = []
    for old, new in mark_pairs(patch):
        mo, mn = MARK_LINE.match(old), MARK_LINE.match(new)
        if not mo or not mn:
            raise RuleError(f"{path}: a changed line is not a checklist row: {new[:80]!r}")
        if mo.group(2) != mn.group(2):
            raise RuleError(f"{path}: a row's text changed, not only its mark: {new[:80]!r}")
        if (mo.group(1), mn.group(1)) not in allowed:
            raise RuleError(
                f"{path}: [{mo.group(1)}] -> [{mn.group(1)}] is not a lane's to make: {new[:80]!r}")
        out.append((mn.group(1), mn.group(2)))
    return out


def row_id(path, rest):
    """The row's id (`**25.4e**`), or for TESTING-KEYS.md the key (`<Esc>`)."""
    if path == "TESTING-KEYS.md":
        m = re.match(r"`([^`]+)`", rest)
    else:
        m = re.match(r"\*\*([^*]+)\*\*", rest)
    if not m:
        raise RuleError(f"{path}: no id at the start of {rest[:80]!r}")
    return m.group(1)


RANGE = re.compile(r"(\d+)\.(\d+)\s*(?:-|–|—|〜|~|to)\s*(?:(\d+)\.)?(\d+)(?![\w.])")


def mentions(line, rid, key):
    """Whether an evidence line names this row or key."""
    if key:
        return f"`{rid}`" in line
    if f"**{rid}**" in line:
        return True
    if re.search(r"(?<![\w.])" + re.escape(rid) + r"(?![\w]|\.\d)", line):
        return True
    m = re.fullmatch(r"(\d+)\.(\d+)", rid)
    if m:  # "2.32-2.35", "2.4〜2.7", "49.9-49.11"
        major, n = m.group(1), int(m.group(2))
        for r in RANGE.finditer(line):
            if r.group(1) == major and r.group(3) in (None, major) \
                    and int(r.group(2)) <= n <= int(r.group(4)):
                return True
    return False


def evidence_items(text):
    """The text's lines, each list item joined with its indented continuation
    lines, so that a picture named on the item's second line still counts."""
    out = []
    for line in text.split("\n"):
        if out and line.startswith((" ", "\t")) and line.strip() \
                and not line.lstrip().startswith(("- ", "* ", "|")) and out[-1].lstrip().startswith(("- ", "* ")):
            out[-1] += " " + line.strip()
        else:
            out.append(line)
    return out


def picture_names(items):
    """The backticked names in a prose paragraph about pictures or shots: a lane's
    report lists its shots once (`s273b`, from Save-Shot -Name) and names them
    without `.png` on the evidence lines."""
    names, para = set(), []
    for line in items + [""]:
        if line.lstrip().startswith(("- ", "* ", "|", "#")):
            line = ""  # evidence items, tables and headings list no shots
        if line.strip():
            para.append(line)
            continue
        joined = " ".join(para)
        if re.search(r"\b(pictures?|shots?|screenshots?)\b", joined, re.IGNORECASE):
            names.update(n for n in re.findall(r"`([^`\s]+)`", joined) if PICTURE_NAME.fullmatch(n))
        para = []
    return names


def missing_evidence(ticks, text):
    """The ticks with no evidence line in `text`, each with what is missing.

    `ticks` is [(path, mark, id)]. An evidence line names the row and says
    something beyond it; a `[~]` also names its picture somewhere on a line
    that names the row: a file name with an image extension, or a backticked
    name the text lists as a picture."""
    items = evidence_items(text)
    shots = picture_names(items)
    out = []
    for path, mark, rid in ticks:
        key = path == "TESTING-KEYS.md"
        found = [ln for ln in items if mentions(ln, rid, key) and len(ln.strip()) >= len(rid) + 10]
        if not found:
            out.append(f"{rid} [{mark}] ({path}): no evidence line")
        elif mark == "~" and not any(PICTURE.search(ln) or shots & set(re.findall(r"`([^`\s]+)`", ln))
                                     for ln in found):
            out.append(f"{rid} [~] ({path}): no picture named on its evidence line")
    return out


def held_numbers(todo, heading):
    """The pull request numbers named under TODO.md's hold heading."""
    out, inside = set(), False
    for line in todo.split("\n"):
        if line.startswith("#"):
            inside = line.lstrip("#").strip() == heading
            continue
        if inside:
            out.update(int(n) for n in re.findall(r"#(\d+)(?!\d)", line))
    return out


def check_verdict(runs, statuses, required, own):
    """'green', 'pending' or 'red: <names>' for a head's check runs.

    Re-runs leave older runs of the same name behind: the newest counts."""
    latest = {}
    for r in runs:
        if r["name"] in own:
            continue
        k = (r["name"], (r.get("app") or {}).get("id"))
        if k not in latest or r["id"] > latest[k]["id"]:
            latest[k] = r
    runs = list(latest.values())
    red = sorted(r["name"] for r in runs
                 if r["status"] == "completed" and r["conclusion"] not in OK_CONCLUSIONS)
    red += [s["context"] for s in statuses if s["state"] in ("failure", "error")]
    if red:
        return "red: " + ", ".join(red)
    if any(r["status"] != "completed" for r in runs) \
            or any(s["state"] == "pending" for s in statuses):
        return "pending"
    names = {r["name"] for r in runs if r["conclusion"] == "success"}
    if any(n not in names for n in required):
        return "pending"  # not started yet
    return "green"


def diff_body(diff):
    """A `git diff` without its file headers: what mark_pairs reads."""
    i = diff.find("\n@@")
    if diff.startswith("@@"):
        return diff
    return "" if i < 0 else diff[i + 1:]


def reapply_marks(text, marks):
    """`text` with each (mark, rest) set on its `- [ ] rest` line.

    Returns the text and the rests whose unticked line is not there (main
    reworded the row, or someone ticked it already)."""
    lines = text.split("\n")
    where = {ln: i for i, ln in enumerate(lines)}
    dropped = []
    for mark, rest in marks:
        i = where.get(f"- [ ] {rest}")
        if i is None:
            if f"- [{mark}] {rest}" not in where:
                dropped.append(rest)
            continue
        lines[i] = f"- [{mark}] {rest}"
    return "\n".join(lines), dropped


def bump_patch(version):
    major, minor, patch = version.split(".")
    return f"{major}.{minor}.{int(patch) + 1}"


def set_lock_version(lock, old, new):
    """Cargo.lock with the workspace's own packages (no `source`) at `new`."""
    blocks = lock.split("\n\n")
    count = 0
    for i, b in enumerate(blocks):
        if b.startswith("[[package]]") and "\nsource = " not in b \
                and f'\nversion = "{old}"\n' in b + "\n":
            blocks[i] = b.replace(f'\nversion = "{old}"', f'\nversion = "{new}"', 1)
            count += 1
    return "\n\n".join(blocks), count


def add_changelog(text, version, date, lines):
    """A new section for `version` at the top, under `## [未リリース]`."""
    head = "## [未リリース]\n\n"
    if head not in text:
        raise RuntimeError("CHANGELOG.md has no `## [未リリース]` heading")
    section = f"## [{version}] - {date}\n\n### 変更\n\n" + "\n".join(lines) + "\n\n"
    return text.replace(head, head + section, 1)


def drop_retests(role, ids):
    """The role with `ids` out of its re-test list (`none yet` when it empties)."""
    out = []
    for line in role.split("\n"):
        if line.startswith(RETESTS):
            cells = line.split("|")
            cell = cells[-2]
            m = re.match(r"(.*:\s*|\s*)(.*?)(\s*)$", cell)
            pre, items, post = m.groups()
            kept = [t.strip() for t in items.split(",") if t.strip() and t.strip() not in ids]
            if items.strip() != "none yet":
                cells[-2] = pre + (", ".join(kept) if kept else "none yet") + post
            line = "|".join(cells)
        out.append(line)
    return "\n".join(out)


def section_items(text, name):
    """The bullets (and paragraphs) under every `## name` / `### name` heading,
    each folded onto one line."""
    items, inside = [], False
    for line in text.split("\n"):
        if line.startswith("#"):
            inside = re.fullmatch(r"#{2,4}\s+" + re.escape(name) + r"\s*", line) is not None
            continue
        if not inside or not line.strip():
            continue
        if line.startswith("- ") or not items or not line.startswith((" ", "\t")):
            items.append(line.strip()[2:] if line.startswith("- ") else line.strip())
        else:
            items[-1] += " " + line.strip()
    return items


def changelog_line(number, machine, done, dropped):
    """One Japanese CHANGELOG line for a merged lane pull request.

    `done` and `dropped` are [(path, mark, id)]."""
    def ids(marks):
        return "・".join(f"`{rid}`" if path == "TESTING-KEYS.md" else rid
                         for path, _, rid in marks)
    ticked = [t for t in done if t[1] != "~"]
    looked = [t for t in done if t[1] == "~"]
    parts = []
    if ticked:
        parts.append(f"{ids(ticked)} を確かめ")
    if looked:
        parts.append(f"{ids(looked)} を画面の画像で見")
    if parts:
        line = f"- 実機（{machine}）で " + "、".join(parts) + "た"
    else:
        line = f"- 実機（{machine}）の実行の報告を足した。印の変わった行は無い"
    if dropped:
        line += f"。{ids(dropped)} は main で行が変わっていたので印を入れていない"
    return line + f"（#{number}）。"


def todo_add(todo, heading, lines):
    """TODO.md with `lines` at the end of the `## heading` section."""
    rows = todo.split("\n")
    start = next((i for i, ln in enumerate(rows) if ln == f"## {heading}"), None)
    if start is None:
        rows += ["", f"## {heading}", ""]
        start = len(rows) - 2
    end = next((i for i in range(start + 1, len(rows)) if rows[i].startswith("## ")), len(rows))
    while end > start + 1 and not rows[end - 1].strip():
        end -= 1
    return "\n".join(rows[:end] + lines + rows[end:])


# ---------------------------------------------------------------- issues


# The labels the issue flow uses, kept as written here by ensure_labels (every
# run, and scripts/labels.sh by hand): name -> (colour, description).
LABELS = {
    "bug": ("d73a4a", "Something isn't working"),
    "finding": ("fbca04", "Something a lane, QA or a session saw, to sort"),
    "question": ("d876e3", "A decision for the owner or the two votes (was QUESTIONS.md)"),
    "retest": ("0e8a16", "Rows to press again on the x64 machine"),
    "release-arm64": ("0e8a16", "Rows the ARM64 machine checks once per release (TESTING-ARM64.md)"),
    "lane:win": ("1d76db", "From the x64 Windows lane"),
    "lane:linux": ("1d76db", "From the Linux lane"),
    "lane:cloud": ("1d76db", "From a cloud session"),
    "lane:qa": ("1d76db", "From the QA lane"),
    "vote": ("c5def5", "Up for the two votes, cloud and win"),
    "vote-decided": ("0e8a16", "The two votes agree: the dev session implements it"),
    "needs-owner": ("b60205", "Waits on the owner"),
    "answered": ("0e8a16", "The owner answered: the dev session implements it"),
    "lane-status": ("ededed", "A lane's status, rewritten at every firing"),
    "lane-stalled": ("b60205", "A lane has not fired for too long"),
    "lane-review": ("fef2c0", "A merged lane pull request whose report the merge routine reads"),
}
LANE_LABELS = {"test/win-": "lane:win", "test/arm-": "lane:win", "test/linux-": "lane:linux"}
VOTERS = {"test/win-": "win"}


def label_changes(existing, wanted):
    """[(method, name, colour, description)] that make `existing` ({name:
    (colour, description)}) hold `wanted`; labels not in `wanted` are left."""
    out = []
    for name, (colour, desc) in wanted.items():
        have = existing.get(name)
        if have is None:
            out.append(("POST", name, colour, desc))
        elif (have[0].lower(), have[1] or "") != (colour, desc):
            out.append(("PATCH", name, colour, desc))
    return out


VOTE = re.compile(r"^\s*vote\[(\w+)\]:\s*(\S+)\s*(?:[—–-]+\s*(.*))?$", re.M)
UNCOUNT = re.compile(r"^\s*uncount\[(\w+)\]:", re.M)
ANSWER = re.compile(r"^\s*回答[:：]", re.M)
TRUSTED = {"OWNER", "MEMBER", "COLLABORATOR"}
VOTE_LABELS = {"vote", "vote-decided", "needs-owner", "answered"}


def tally(comments):
    """'answered', 'decided:<option>', 'split' or 'open' for a question issue.

    `comments` is [(author_association, body)], oldest first; the workflow's
    own relayed votes come as association BOT. Only the owner's side counts:
    a `回答:` from it answers (and wins over any vote), and `vote[cloud]:` /
    `vote[win]:` lines vote, the newest per voter counting; `uncount[voter]:`
    takes that voter's earlier vote back. Both voters on one option decide
    it; two options, or either saying `owner`, split it to the owner."""
    votes, answered = {}, False
    for who, body in comments:
        if who not in TRUSTED and who != "BOT":
            continue
        body = body or ""
        if who != "BOT" and ANSWER.search(body):
            answered = True
        for m in re.finditer(VOTE.pattern + "|" + UNCOUNT.pattern, body, re.M):
            if m[1] in ("cloud", "win"):
                votes[m[1]] = m[2].rstrip(".,")
            elif m[4] in ("cloud", "win"):
                votes.pop(m[4], None)
    if answered:
        return "answered"
    if len(votes) < 2:
        return "open"
    if votes["cloud"] == votes["win"] and votes["cloud"] != "owner":
        return f"decided:{votes['cloud']}"
    return "split"


def vote_labels(state, current):
    """The vote labels (of VOTE_LABELS) a question issue should carry."""
    if state == "answered":
        return {"answered"}
    if state.startswith("decided:"):
        return {"vote-decided"}
    if state == "split":
        return {"needs-owner"}
    have = current & VOTE_LABELS
    if "vote-decided" in have:  # a vote taken back: up for the votes again
        return (have - {"vote-decided"}) | {"vote"}
    return have


RELAYED_VOTE = re.compile(r"^#(\d+):\s*(\S+)\s*(?:[—–-]+\s*(.*))?$")


def finding_marker(number):
    """The hidden line that ties a finding issue to its lane pull request:
    `<!-- kura-lane:#N -->` in uchmk/kura, named after the repository."""
    app = REPO.rsplit("/", 1)[-1] or "kura"
    return f"<!-- {app}-lane:#{number} -->"


def finding_body(number, machine, items):
    """The body of the one finding issue for a merged lane pull request's
    leftovers: `items` is [(section, where, text)]."""
    lines = [finding_marker(number),
             f"What the {machine} lane's run in #{number} left in its report or pull request body"
             " for somebody to sort (written by scripts/merge-lanes.py). Make each one its own"
             " `bug` / `finding` / `question` issue, a TODO.md line or nothing, then close this.", ""]
    for section in ("Proposals", "Queue", "Votes"):
        rows = [f"- {text} ({where})" for s, where, text in items if s == section]
        if rows:
            lines += [f"### {section}", "", *rows, ""]
    return "\n".join(lines)


# ---------------------------------------------------------------- GitHub


LAST_FIRING = re.compile(r"\*\*Last firing\*\* (\d{4}-\d\d-\d\d \d\d:\d\d) ([+-])(\d\d):?(\d\d)")
STALLED = "lane-stalled"


def last_firing(body):
    """When a lane-status issue's body says the lane last fired, or None."""
    m = LAST_FIRING.search(body or "")
    if not m:
        return None
    offset = datetime.timedelta(hours=int(m[3]), minutes=int(m[4])) * (-1 if m[2] == "-" else 1)
    return datetime.datetime.strptime(m[1], "%Y-%m-%d %H:%M").replace(tzinfo=datetime.timezone(offset))


def overdue_hours(body):
    """How old the last firing may be: a firing every hour, a run up to four."""
    return 5 if "**This firing:** A run is going" in (body or "") else 3


def api(method, path, body=None):
    url = path if path.startswith("http") else API + path
    req = urllib.request.Request(url, method=method)
    req.add_header("Authorization", f"Bearer {TOKEN}")
    req.add_header("Accept", "application/vnd.github+json")
    req.add_header("X-GitHub-Api-Version", "2022-11-28")
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, data, timeout=60) as resp:
            raw = resp.read()
            link = resp.headers.get("Link", "")
            return resp.status, (json.loads(raw) if raw else None), link
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            payload = json.loads(raw)
        except ValueError:
            payload = {"message": raw.decode(errors="replace")}
        return e.code, payload, ""


def get(path):
    status, payload, _ = api("GET", path)
    if status != 200:
        raise RuntimeError(f"GET {path}: {status} {payload}")
    return payload


def get_all(path, key=None):
    out, url = [], path + ("&" if "?" in path else "?") + "per_page=100"
    while url:
        status, payload, link = api("GET", url)
        if status != 200:
            raise RuntimeError(f"GET {url}: {status} {payload}")
        out.extend(payload[key] if key else payload)
        m = re.search(r'<([^>]+)>;\s*rel="next"', link)
        url = m.group(1) if m else None
    return out


def raw_file(path, ref):
    status, payload, _ = api("GET", f"/repos/{REPO}/contents/{path}?ref={ref}")
    if status != 200:
        return ""
    return base64.b64decode(payload.get("content", "")).decode("utf-8", errors="replace")


def comment_once(pr, kind, sha, text, lead=None):
    marker = MARKER.format(kind=kind, sha=sha)
    for c in get_all(f"/repos/{REPO}/issues/{pr['number']}/comments"):
        if marker in (c.get("body") or ""):
            return
    if lead:
        body = f"{marker}\n**merge-lanes** {lead}:\n\n{text}"
    else:
        body = f"{marker}\n**merge-lanes** did not merge this ({kind}, head `{sha[:7]}`):\n\n{text}\n\n" \
               "The merge routine reads this and takes it from here (`.claude/merge-routine.md`)."
    if DRY:
        print(f"  (dry run) would comment: {kind}: {text}")
        return
    api("POST", f"/repos/{REPO}/issues/{pr['number']}/comments", {"body": body})


def git(*args, check=True):
    p = subprocess.run(["git", *args], capture_output=True, text=True, encoding="utf-8")
    if check and p.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {p.stderr.strip() or p.stdout.strip()}")
    return p.stdout if check else p


def read(path):
    try:
        with open(path, encoding="utf-8", newline="") as f:
            return f.read()
    except FileNotFoundError:
        return ""


def write(path, text):
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write(text)


def checkout_base(base):
    git("fetch", "-q", "origin", f"+refs/heads/{base}:refs/remotes/origin/{base}")
    git("checkout", "-q", "--force", "--detach", f"origin/{base}")


def staged_problems(base, allowed_marks):
    """What in the index, against the base, is not marks and new reports."""
    problems = []
    for line in git("diff", "--cached", "--name-status", f"origin/{base}").splitlines():
        status, path = line.split("\t", 1)
        if path.startswith("qa-reports/") and status == "A":
            continue
        if path in allowed_marks and status == "M":
            try:
                new_marks(path, diff_body(git("diff", "--cached", f"origin/{base}", "--", path)),
                          allowed_marks[path])
            except RuleError as e:
                problems.append(str(e))
            continue
        problems.append(f"{path}: {status} after the merge")
    return problems


def resolve_conflict(pr, allowed_marks, ticks):
    """Merge a conflicting lane pull request by hand and push it to the base.

    Returns (outcome, dropped rests) -- outcome None when it was not done."""
    n, head, base = pr["number"], pr["head"]["sha"], pr["base"]["ref"]
    checkout_base(base)
    git("fetch", "-q", "origin", f"+refs/pull/{n}/head:refs/merge-lanes/{n}")
    if git("rev-parse", f"refs/merge-lanes/{n}").strip() != head:
        return "head moved while resolving; next time", []
    label = pr["head"]["label"].replace(":", "/", 1)
    msg = f"Merge pull request #{n} from {label}\n\n{pr['title']}"
    git("merge", "-q", "--no-ff", "--no-commit", "-m", msg, head, check=False)
    conflicted = git("diff", "--name-only", "--diff-filter=U").split()
    others = [f for f in conflicted if f not in allowed_marks]
    if others:
        git("merge", "--abort", check=False)
        return None, []
    dropped = []
    for path in conflicted:
        marks = [(mark, rest) for p, mark, _, rest in ticks if p == path]
        text, lost = reapply_marks(git("show", f"origin/{base}:{path}"), marks)
        write(path, text)
        git("add", "--", path)
        dropped += [(p, mark, rid) for p, mark, rid, rest in ticks if p == path and rest in lost]
    problems = staged_problems(base, allowed_marks)
    if problems:
        git("merge", "--abort", check=False)
        raise RuntimeError("the resolved merge is not marks only: " + "; ".join(problems))
    if DRY:
        git("merge", "--abort", check=False)
        return f"(dry run) would resolve and push ({len(conflicted)} file(s))", dropped
    git(*BOT, "commit", "-q", "--no-edit")
    p = git("push", "-q", "origin", f"HEAD:refs/heads/{base}", check=False)
    if p.returncode != 0:
        return f"push refused, next time: {p.stderr.strip()[:200]}", dropped
    return f"resolved {len(conflicted)} conflicting file(s) and pushed to {base}", dropped


# ---------------------------------------------------------------- one pull request


def rule_problems(pr, rules, allowed_marks, head):
    """What breaks the file, mark and evidence rules (empty: none)."""
    files = get_all(f"/repos/{REPO}/pulls/{pr['number']}/files")
    problems, ticks, reports = [], [], []
    for f in files:
        name, status = f["filename"], f["status"]
        if name.startswith("qa-reports/"):
            if status != "added":
                problems.append(f"{name}: {status} -- a run only adds its own report")
            else:
                reports.append(name)
            continue
        if name not in allowed_marks:
            problems.append(f"{name}: not a file this lane may change")
            continue
        if status != "modified" or "patch" not in f:
            problems.append(f"{name}: {status}, no line diff to read")
            continue
        try:
            for mark, rest in new_marks(name, f["patch"], allowed_marks[name]):
                ticks.append((name, mark, row_id(name, rest), rest))
        except RuleError as e:
            problems.append(str(e))
    lo, hi = rules["reports"]
    if not lo <= len(reports) <= hi:
        problems.append(f"{len(reports)} new file(s) under qa-reports/; expected {lo}" +
                        (f" to {hi}" if hi != lo else ""))
    if not problems:
        text = pr.get("body") or ""
        if rules["evidence_in_report"]:
            text += "\n" + "\n".join(raw_file(r, head) for r in reports if r.endswith(".md"))
        problems += missing_evidence([t[:3] for t in ticks], text)
    return problems, ticks


def mergeable(number):
    """GitHub's `mergeable` for a pull request, waiting a little while it is computed."""
    for _ in range(6):
        pr = get(f"/repos/{REPO}/pulls/{number}")
        if pr["mergeable"] is not None:
            return pr
        time.sleep(5)
    return pr


def handle(pr, rules, allowed_marks, held, own_checks):
    n, head = pr["number"], pr["head"]["sha"]
    if n in held:
        return "held in TODO.md"
    problems, ticks = rule_problems(pr, rules, allowed_marks, head)
    if problems:
        comment_once(pr, "rules", head, "\n".join(f"- {p}" for p in problems))
        return "rules: " + "; ".join(problems)
    runs = get_all(f"/repos/{REPO}/commits/{head}/check-runs", "check_runs")
    statuses = get(f"/repos/{REPO}/commits/{head}/status")["statuses"]
    verdict = check_verdict(runs, statuses, rules["required_checks"], own_checks)
    if verdict.startswith("red"):
        comment_once(pr, "red", head, f"- {verdict}")
        return verdict
    fresh = mergeable(n)
    if fresh["head"]["sha"] != head:
        return "head moved while checking; next time"
    if fresh["mergeable"] is None:
        return "GitHub has not worked out mergeability yet; next time"
    if not fresh["mergeable"]:
        # a conflicting pull request's checks may never start, so this does
        # not wait for green: its files are marks and reports, checked above
        outcome, dropped = resolve_conflict(fresh, allowed_marks, ticks)
        if outcome is None:
            comment_once(pr, "conflict", head, "- it conflicts with the base branch in a file"
                         " other than the checklists")
            return "conflict"
        if dropped and not DRY and outcome.startswith("resolved"):
            comment_once(pr, "dropped", head, "\n".join(
                f"- {rid} [{mark}] ({path})" for path, mark, rid in dropped),
                lead="merged this by resolving the conflict; main had reworded these rows,"
                     " so their marks were not carried over")
        return outcome
    if verdict != "green":
        return "checks still running"
    if DRY:
        return f"(dry run) would merge {head[:7]} ({len(ticks)} marks)"
    status, payload, _ = api("PUT", f"/repos/{REPO}/pulls/{n}/merge",
                             {"merge_method": "merge", "sha": head})
    if status != 200 or not payload.get("merged"):
        return f"merge refused: {status} {payload.get('message')}"
    return f"merged {head[:7]} ({len(ticks)} marks)"


# ---------------------------------------------------------------- the share


def merged_ticks(pr, allowed_marks, merge_sha):
    """The merged pull request's (path, mark, id, rest), and its new reports."""
    ticks, reports = [], []
    for f in get_all(f"/repos/{REPO}/pulls/{pr['number']}/files"):
        name = f["filename"]
        if name.startswith("qa-reports/") and f["status"] == "added" and name.endswith(".md"):
            reports.append((name, read(name) or raw_file(name, merge_sha)))
        elif name in allowed_marks and "patch" in f:
            try:
                for mark, rest in new_marks(name, f["patch"], allowed_marks[name]):
                    ticks.append((name, mark, row_id(name, rest), rest))
            except RuleError as e:  # merged by hand by the owner
                print(f"  #{pr['number']}: {e}")
    return ticks, reports


def share_once(rules, share, base, since):
    """Make the share commit on a fresh base; returns (prs shared, problem)."""
    checkout_base(base)
    changelog = read("CHANGELOG.md")
    prs = []
    for pr in get_all(f"/repos/{REPO}/pulls?state=closed&sort=updated&direction=desc")[:100]:
        lane = next((p for p in rules["lanes"] if pr["head"]["ref"].startswith(p)), None)
        if lane and pr.get("merged_at") and pr["merged_at"] >= since \
                and not re.search(rf"#{pr['number']}(?!\d)", changelog):
            prs.append((pr, lane))
    if not prs:
        return [], None
    prs.sort(key=lambda x: x[0]["number"])
    lines, todo_lines, retested = [], [], set()
    for pr, lane in prs:
        n = pr["number"]
        ticks, reports = merged_ticks(pr, rules["lanes"][lane], pr["merge_commit_sha"])
        done, dropped = [], []
        for path, mark, rid, rest in ticks:
            (done if f"- [{mark}] {rest}" in read(path).split("\n") else dropped).append((path, mark, rid))
        lines.append(changelog_line(n, MACHINES[lane], done, dropped))
        retested.update(rid for path, _, rid in done if path == "TESTING-CHECKS.md")
        mark = share["todo_mark"]
        seen, items = set(), []
        for where, text in reports + \
                [("PR 本文", pr.get("body") or "")]:
            for kind, name in [("", "Proposals"), ("・キュー", "Queue"), ("・票", "Votes")]:
                for item in section_items(text, name):
                    if item not in seen:
                        seen.add(item)
                        items.append((name, where, item))
                        todo_lines.append(f"- [ ] （実機 #{n}{kind}）{item}（{where}）{mark}")
        if share["issues"]:
            todo_lines = []
            if not DRY:
                file_findings(pr, lane, items)
            else:
                print(f"  (dry run) #{n}: would file {len(items)} leftovers and label it lane-review")
    cargo = read("Cargo.toml")
    m = re.search(r'^version = "(\d+\.\d+\.\d+)"', cargo, re.M)
    old = m.group(1)
    new = bump_patch(old)
    lock, count = set_lock_version(read("Cargo.lock"), old, new)
    if count == 0:
        return [], f"Cargo.lock has no workspace package at {old}"
    write("Cargo.toml", cargo[:m.start(1)] + new + cargo[m.end(1):])
    write("Cargo.lock", lock)
    today = datetime.datetime.now(datetime.timezone(datetime.timedelta(hours=9))).strftime("%Y-%m-%d")
    write("CHANGELOG.md", add_changelog(changelog, new, today, lines))
    if retested and share["role"]:
        write(share["role"], drop_retests(read(share["role"]), retested))
    if todo_lines:
        write("TODO.md", todo_add(read("TODO.md"), share["todo_heading"], todo_lines))
    numbers = ", ".join(f"#{pr['number']}" for pr, _ in prs)
    which = ", ".join(f"#{pr['number']} ({MACHINES[lane]})" for pr, lane in prs)
    msg = (f"v{new}: Merge real-machine checks from {numbers}\n\n"
           f"Record the real-machine lane runs merged from {which}: the rows they checked are listed"
           " in the changelog" + (", the re-test list drops them," if share["role"] else "") +
           (" and their reports' proposals, queue notes and votes are in a finding issue each,"
            " for the merge routine to sort." if share["issues"] else
            " and their reports' proposals, queue notes and votes are listed in TODO.md"
            " for an interactive session."))
    if DRY:
        print(f"(dry run) would commit:\n{msg}\n" + "\n".join(lines + todo_lines))
        git("checkout", "-q", "--force", "--detach", f"origin/{base}")
        return [pr for pr, _ in prs], None
    git("add", "-A", "--", "Cargo.toml", "Cargo.lock", "CHANGELOG.md", "TODO.md",
        *([share["role"]] if share["role"] else []))
    git(*BOT, "commit", "-q", "-m", msg)
    p = git("push", "-q", "origin", f"HEAD:refs/heads/{base}", check=False)
    if p.returncode != 0:
        return [], "push refused: " + p.stderr.strip()[:200]
    return [pr for pr, _ in prs], None


def do_share(rules, share):
    base = get(f"/repos/{REPO}")["default_branch"]
    since = (datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(days=7)) \
        .strftime("%Y-%m-%dT%H:%M:%SZ")
    problem = None
    for _ in range(3):  # main moved under the push: start again from it
        shared, problem = share_once(rules, share, base, since)
        if shared:
            print("share: " + ", ".join(f"#{pr['number']}" for pr in shared))
            if not DRY:
                for wf in share["dispatch"]:
                    status, payload, _ = api("POST", f"/repos/{REPO}/actions/workflows/{wf}/dispatches",
                                             {"ref": base})
                    print(f"  dispatched {wf}: {status}" + ("" if status == 204 else f" {payload}"))
            return
        if problem is None:
            print("share: nothing to share")
            return
        print(f"share: {problem}")
        if not problem.startswith("push refused"):
            return
    print("share: gave up for this run")


def watch_lanes(watch_when=None):
    """Label a lane whose status issue has not been written for too long.

    The lane's machine rewrites its open "Lane status: <lane>" issue (label
    lane-status) at every firing, so an old **Last firing** means the firings
    stopped, and nothing on the machine can say so: it is off or asleep, or the
    scheduled task fails before the script runs. This adds lane-stalled and one
    comment per last firing (a comment notifies; an edit does not), and takes
    the label off once the lane writes again. Closing the issue retires the lane.

    A lane named in watch_when ({lane: label}) is watched only while an issue
    with that label is open, and is not stalled otherwise.
    """
    now = datetime.datetime.now(datetime.timezone.utc)
    gates = {}
    for issue in get_all(f"/repos/{REPO}/issues?state=open&labels=lane-status"):
        if "pull_request" in issue:
            continue
        number, body = issue["number"], issue.get("body") or ""
        labels = {label["name"] for label in issue.get("labels", [])}
        gate = (watch_when or {}).get(issue.get("title", "").removeprefix("Lane status: ").strip())
        if gate:
            if gate not in gates:
                gates[gate] = any("pull_request" not in i
                                  for i in get_all(f"/repos/{REPO}/issues?state=open&labels={gate}"))
            if not gates[gate]:
                print(f"watch: #{number}: no open {gate} issue, not watched")
                if STALLED in labels and not DRY:
                    api("DELETE", f"/repos/{REPO}/issues/{number}/labels/{STALLED}")
                continue
        when, hours = last_firing(body), overdue_hours(body)
        if when is None:
            print(f"watch: #{number}: no **Last firing** in the body")
            continue
        age = now - when
        if age <= datetime.timedelta(hours=hours):
            if STALLED in labels:
                print(f"watch: #{number}: writing again, {STALLED} off")
                if not DRY:
                    api("DELETE", f"/repos/{REPO}/issues/{number}/labels/{STALLED}")
            continue
        late = f"{age.days * 24 + age.seconds // 3600} h {age.seconds // 60 % 60} min"
        print(f"watch: #{number}: last firing {late} ago, {STALLED}")
        if STALLED not in labels and not DRY:
            api("POST", f"/repos/{REPO}/issues/{number}/labels", {"labels": [STALLED]})
        comment_once({"number": number}, "stalled", when.strftime("%Y-%m-%dT%H:%M%z"),
                     f"The last firing written here was {late} ago ({when:%Y-%m-%d %H:%M %z}), and a firing "
                     f"should write every hour ({hours} h is the limit{' while a run is going' if hours > 3 else ''}). "
                     "The machine is off or asleep, or its scheduled task fails before the script runs: what to "
                     "look at on the machine is in the body's last paragraph. The label comes off at the next "
                     "firing that writes here.",
                     lead=f"has not seen this lane fire for {late}")


def file_findings(pr, lane, items):
    """A merged lane pull request's report, as issues: the votes on a question
    issue (`- #N: option — why`) as a comment there, everything else in one
    finding issue, and the label lane-review on the pull request itself, which
    the merge routine reads and takes off."""
    n = pr["number"]
    api("POST", f"/repos/{REPO}/issues/{n}/labels", {"labels": ["lane-review"]})
    rest = []
    for section, where, text in items:
        m = RELAYED_VOTE.match(text) if section == "Votes" and lane in VOTERS else None
        if m:
            status, issue, _ = api("GET", f"/repos/{REPO}/issues/{m[1]}")
            names = {label["name"] for label in (issue or {}).get("labels", [])} if status == 200 else set()
            if "question" in names and issue.get("state") == "open":
                why = f" — {m[3]}" if m[3] else ""
                comment_once({"number": int(m[1])}, "vote", f"pr{n}",
                             f"vote[{VOTERS[lane]}]: {m[2]}{why}",
                             lead=f"relays the {MACHINES[lane]} lane's vote from #{n} ({where})")
                continue
        rest.append((section, where, text))
    if not rest:
        return
    marker = finding_marker(n)
    for issue in get_all(f"/repos/{REPO}/issues?state=all&labels=finding"):
        if marker in (issue.get("body") or ""):
            return
    status, payload, _ = api("POST", f"/repos/{REPO}/issues", {
        "title": f"Lane #{n} ({MACHINES[lane]}): what its report left to sort",
        "body": finding_body(n, MACHINES[lane], rest),
        "labels": ["finding", LANE_LABELS[lane]]})
    print(f"  #{n}: finding issue {status} {payload.get('html_url') or payload.get('message')}")


def wanted_labels(setting):
    """The labels of LABELS a rule's `labels` asks for: True for all of them,
    or a list of names; nothing when it is missing or false."""
    if setting is True:
        return dict(LABELS)
    return {name: LABELS[name] for name in setting or []}


def ensure_labels(setting=True):
    """Create or correct the labels the rule names; others are left alone."""
    existing = {label["name"]: (label["color"], label.get("description") or "")
                for label in get_all(f"/repos/{REPO}/labels")}
    changes = label_changes(existing, wanted_labels(setting))
    for method, name, colour, desc in changes:
        print(f"labels: {method} {name}")
        if DRY:
            continue
        path = f"/repos/{REPO}/labels" + ("" if method == "POST" else "/" + urllib.parse.quote(name))
        status, payload, _ = api(method, path, {"name": name, "color": colour, "description": desc})
        if status not in (200, 201):
            print(f"  {status} {payload.get('message')}")
    if not changes:
        print("labels: as LABELS says")


def count_votes():
    """Count the votes on every open question issue and set its labels.

    An owner's `回答:` makes it answered on any question issue. The votes count
    only on one up for them (vote or vote-decided): agreement makes it
    vote-decided, a split needs-owner (which stays until the owner answers or
    puts vote back), each with one comment saying so."""
    for issue in get_all(f"/repos/{REPO}/issues?state=open&labels=question"):
        if "pull_request" in issue:
            continue
        number = issue["number"]
        current = {label["name"] for label in issue.get("labels", [])}
        comments = [("BOT" if (c.get("user") or {}).get("login") == "github-actions[bot]"
                     else c.get("author_association", ""), c.get("body") or "")
                    for c in get_all(f"/repos/{REPO}/issues/{number}/comments")]
        state = tally(comments)
        if state != "answered" and not current & {"vote", "vote-decided"}:
            continue
        wanted = vote_labels(state, current)
        have = current & VOTE_LABELS
        if wanted == have:
            continue
        print(f"votes: #{number}: {state}, labels {sorted(have)} -> {sorted(wanted)}")
        if DRY:
            continue
        for name in have - wanted:
            api("DELETE", f"/repos/{REPO}/issues/{number}/labels/{urllib.parse.quote(name)}")
        if wanted - have:
            api("POST", f"/repos/{REPO}/issues/{number}/labels", {"labels": sorted(wanted - have)})
        if state.startswith("decided:"):
            option = state.split(":", 1)[1]
            comment_once({"number": number}, "decided", option,
                         f"Both votes say **{option}**. The dev session implements it, takes"
                         f" `（要確認: #{number}）` out of TODO.md and closes this issue. A `回答:` from the"
                         " owner still wins.", lead="counted the votes")
        elif state == "split":
            comment_once({"number": number}, "split", f"c{len(comments)}",
                         "The two votes differ (or one says `owner`), so this waits on the owner's `回答:`.",
                         lead="counted the votes")


def main():
    rules = RULES.get(REPO)
    if not rules:
        sys.exit(f"merge-lanes: no rules for {REPO!r}")
    if not TOKEN:
        sys.exit("merge-lanes: GITHUB_TOKEN is not set")
    own_checks = {os.environ.get("MERGE_LANES_JOB", "merge")}
    held = set()
    if rules["hold_heading"]:
        default = get(f"/repos/{REPO}")["default_branch"]
        held = held_numbers(raw_file("TODO.md", default), rules["hold_heading"])
    prs = get_all(f"/repos/{REPO}/pulls?state=open&sort=created&direction=asc")
    for pr in prs:
        ref = pr["head"]["ref"]
        lane = next((p for p in rules["lanes"] if ref.startswith(p)), None)
        if lane is None:
            continue
        if pr["draft"] or (pr["head"].get("repo") or {}).get("full_name") != REPO:
            print(f"#{pr['number']} {ref}: skipped (draft or from a fork)")
            continue
        try:
            outcome = handle(pr, rules, rules["lanes"][lane], held, own_checks)
        except RuntimeError as e:
            outcome = f"error: {e}"
        print(f"#{pr['number']} {ref}: {outcome}")
    if rules.get("labels"):
        try:
            ensure_labels(rules["labels"])
        except RuntimeError as e:
            print(f"labels: {e}")
    if rules["share"]:
        do_share(rules, rules["share"])
    if rules.get("votes"):
        try:
            count_votes()
        except RuntimeError as e:
            print(f"votes: {e}")
    try:
        watch_lanes(rules.get("watch_when"))
    except RuntimeError as e:
        print(f"watch: {e}")


# ---------------------------------------------------------------- self-test


def self_test():
    patch = ("@@ -38,4 +38,4 @@\n - [x] **1.1** a\n-- [ ] **1.2** b\n-- [ ] **1.3** c\n"
             "+- [x] **1.2** b\n+- [~] **1.3** c\n - [ ] **1.4** d\n"
             "@@ -90,2 +90,2 @@\n-- [ ] **49.10** e\n+- [x] **49.10** e\n\\ No newline at end of file")
    marks = new_marks("TESTING-CHECKS.md", patch, WINDOWS_MARKS["TESTING-CHECKS.md"])
    assert marks == [("x", "**1.2** b"), ("~", "**1.3** c"), ("x", "**49.10** e")], marks
    for bad, why in [
        ("@@\n-- [ ] **1.2** b\n+- [x] **1.2** B\n", "text changed"),
        ("@@\n - [ ] **1.2** b\n+- [x] **1.3** c\n", "line added"),
        ("@@\n-- [x] **1.2** b\n+- [ ] **1.2** b\n", "untick"),
        ("@@\n-- [~] **1.2** b\n+- [x] **1.2** b\n", "[~] to [x] is the owner's"),
        ("@@\n-## 1. Pane\n+## 1. Panes\n", "not a row"),
    ]:
        try:
            new_marks("TESTING-CHECKS.md", bad, WINDOWS_MARKS["TESTING-CHECKS.md"])
        except RuleError:
            continue
        raise AssertionError(f"accepted: {why}")
    try:
        new_marks("TESTING-KEYS.md", "@@\n-- [ ] `q` — Quit\n+- [~] `q` — Quit\n",
                  WINDOWS_MARKS["TESTING-KEYS.md"])
        raise AssertionError("accepted [~] in kura's TESTING-KEYS.md")
    except RuleError:
        pass
    assert new_marks("TESTING-KEYS.md", "@@\n-- [ ] `F1` — Help\n+- [~] `F1` — Help\n",
                     TSUMUGI_WINDOWS_MARKS["TESTING-KEYS.md"]) == [("~", "`F1` — Help")]

    assert row_id("TESTING-CHECKS.md", "**25.4e** text") == "25.4e"
    assert row_id("TESTING-KEYS.md", "`<C-q>` — Quit the process · `quit`") == "<C-q>"
    assert row_id("TESTING-KEYS.md", "`Ctrl+Shift+T` · `Cmd+T` — New session") == "Ctrl+Shift+T"

    assert mentions("- **25.4e** `[x]`: shows v7.5.4.0", "25.4e", False)
    assert mentions("- 2.28: Ctrl+Shift+Up x3 put CMD5 at the top", "2.28", False)
    assert not mentions("- 2.28: ...", "2.2", False)
    assert not mentions("- **1.1a** ...", "1.1", False)
    assert not mentions("- 12.28: ...", "2.28", False)
    assert mentions("- 2.32-2.35: all four held as written", "2.33", False)
    assert mentions("- 49.9-49.11 held on the new pane", "49.10", False)
    assert mentions("- 2.4〜2.7 looked right in the picture", "2.4", False)
    assert not mentions("- 2.32-2.35: ...", "2.36", False)
    assert not mentions("- 3.32-3.35: ...", "2.33", False)
    assert mentions("- `Ctrl+Shift+T`: key log `Some(NewTab)`", "Ctrl+Shift+T", True)
    assert not mentions("- `Ctrl+Shift+Tab`: ...", "Ctrl+Shift+T", True)

    body = ("## Evidence\n- **1.2** `[x]`: printed `ok` twice, exit 0\n"
            "- **1.3** `[~]`: the block cursor, `C:\\ev\\run\\1.3-cursor.png`, no hollow box\n"
            "- **1.4** `[~]`: looked fine\n- **1.5**\n")
    ticks = [("TESTING-CHECKS.md", "x", "1.2"), ("TESTING-CHECKS.md", "~", "1.3"),
             ("TESTING-CHECKS.md", "~", "1.4"), ("TESTING-CHECKS.md", "x", "1.5"),
             ("TESTING-CHECKS.md", "x", "1.6")]
    miss = missing_evidence(ticks, body)
    assert [m.split(" ")[0] for m in miss] == ["1.4", "1.5", "1.6"], miss

    # tsumugi #12: the shots listed once without `.png`, the name on the item's second line
    report = ("Pictures are in `C:\\shots\\run\\` (not in the repo): `s252_pwsh`,\n"
              "`s273b`, `s510_1280`.\n\n## Evidence\n"
              "- **2.73** A 257 586-byte PNG through OSC 1337: `after` at 0.19 s\n"
              "  and 0.17 s. The picture drew (`s273b`); the shape is a look, so `[~]`.\n"
              "- **2.74** `[~]`: the bar looked right, `after` read\n"
              "- **2.75** `[~]`: as in the picture\n  - sub-item `s273b`\n")
    ticks = [("TESTING-CHECKS.md", "~", "2.73"), ("TESTING-CHECKS.md", "~", "2.74"),
             ("TESTING-CHECKS.md", "~", "2.75")]
    miss = missing_evidence(ticks, report)
    assert [m.split(" ")[0] for m in miss] == ["2.74", "2.75"], miss

    todo = ("## A\n- #12 not this\n## マージで止めている実機の PR\n\n説明（#3 の例）\n\n"
            "- #311: 32.20 を…\n  - 持ち主の答え: #312 と同じ\n## B\n- #400\n")
    assert held_numbers(todo, "マージで止めている実機の PR") == {3, 311, 312}
    assert held_numbers("## マージで止めている実機の PR\n\nいまは無い\n", "マージで止めている実機の PR") == set()

    def run(name, status, conclusion, rid=1, app=1):
        return {"name": name, "status": status, "conclusion": conclusion, "id": rid, "app": {"id": app}}
    green = [run("checklists", "completed", "success"), run("audit", "completed", "skipped", 2)]
    assert check_verdict(green, [], ["checklists"], {"merge"}) == "green"
    assert check_verdict(green, [], ["clippy"], {"merge"}) == "pending"
    assert check_verdict(green + [run("test", "in_progress", None, 3)], [], [], set()) == "pending"
    assert check_verdict(green + [run("test", "completed", "failure", 3)], [], [], set()) == "red: test"
    rerun = [run("test", "completed", "failure", 3), run("test", "completed", "success", 4)]
    assert check_verdict(green + rerun, [], [], set()) == "green"
    assert check_verdict(green + [run("merge", "in_progress", None, 5)], [], [], {"merge"}) == "green"
    assert check_verdict(green, [{"context": "ci/x", "state": "failure"}], [], set()) == "red: ci/x"

    diff = ("diff --git a/T.md b/T.md\nindex 1..2 100644\n--- a/T.md\n+++ b/T.md\n"
            "@@ -1 +1 @@\n-- [ ] **1.2** b\n+- [x] **1.2** b\n")
    assert new_marks("T.md", diff_body(diff), {(" ", "x")}) == [("x", "**1.2** b")]
    assert diff_body("") == ""

    main_text = "# T\n- [x] **1.1** a\n- [ ] **1.2** b\n- [ ] **1.3** C now\n- [~] **1.4** d\n"
    text, lost = reapply_marks(main_text, [("x", "**1.2** b"), ("~", "**1.3** c"), ("~", "**1.4** d")])
    assert text == "# T\n- [x] **1.1** a\n- [x] **1.2** b\n- [ ] **1.3** C now\n- [~] **1.4** d\n", text
    assert lost == ["**1.3** c"], lost

    assert bump_patch("0.76.5") == "0.76.6"
    lock = ('version = 4\n\n[[package]]\nname = "a"\nversion = "0.76.5"\n'
            'source = "registry+https://github.com/rust-lang/crates.io-index"\n\n'
            '[[package]]\nname = "tsumugi"\nversion = "0.76.5"\ndependencies = [\n "a",\n]\n\n'
            '[[package]]\nname = "tsumugi-ipc"\nversion = "0.76.5"\n')
    out, count = set_lock_version(lock, "0.76.5", "0.76.6")
    assert count == 2 and out.count('"0.76.6"') == 2 and out.count('"0.76.5"') == 1, out

    log = "# 変更履歴\n\n## [未リリース]\n\n## [0.76.5] - 2026-10-10\n"
    assert add_changelog(log, "0.76.6", "2026-10-11", ["- a（#9）。"]) == (
        "# 変更履歴\n\n## [未リリース]\n\n## [0.76.6] - 2026-10-11\n\n### 変更\n\n"
        "- a（#9）。\n\n## [0.76.5] - 2026-10-10\n")

    role = ("| Chunk | Up to | Notes |\n"
            "| **Re-tests of changed behaviour** | 15 rows | Still `[ ]`: 2.58, 2.54, 9.5 |\n| x | y |")
    assert drop_retests(role, {"2.54", "3.1"}).split("\n")[1] == \
        "| **Re-tests of changed behaviour** | 15 rows | Still `[ ]`: 2.58, 9.5 |"
    assert drop_retests(role, {"2.58", "2.54", "9.5"}).split("\n")[1] == \
        "| **Re-tests of changed behaviour** | 15 rows | Still `[ ]`: none yet |"
    assert drop_retests("| **Re-tests of changed behaviour** | 2.4, 1.3 |", {"2.4"}) == \
        "| **Re-tests of changed behaviour** | 1.3 |"
    none = "| **Re-tests of changed behaviour** | 15 rows | Still `[ ]`: none yet |"
    assert drop_retests(none, {"2.4"}) == none

    report = ("# Run\n\n### Proposals\n\n- **Reword 2.62** so the command\n  differs. Size: small.\n"
              "- Kit helpers.\n\n## Queue\n\n- 2.67 stays `[ ]`.\n\n## Other\n- not this\n")
    assert section_items(report, "Proposals") == ["**Reword 2.62** so the command differs. Size: small.",
                                                  "Kit helpers."]
    assert section_items(report, "Queue") == ["2.67 stays `[ ]`."]
    assert section_items("## Queue\n\nNothing in the queue needs to change.\n", "Queue") == \
        ["Nothing in the queue needs to change."]

    done = [("TESTING-CHECKS.md", "x", "2.61"), ("TESTING-CHECKS.md", "~", "2.62"),
            ("TESTING-KEYS.md", "x", "F1")]
    assert changelog_line(9, "ARM64", done, []) == "- 実機（ARM64）で 2.61・`F1` を確かめ、2.62 を画面の画像で見た（#9）。"
    assert changelog_line(10, "x64", [], [("TESTING-CHECKS.md", "x", "2.3")]) == \
        "- 実機（x64）の実行の報告を足した。印の変わった行は無い。2.3 は main で行が変わっていたので印を入れていない（#10）。"

    todo = "# TODO\n\n## 実機のレーンから\n\n説明\n\n- [ ] a\n\n## 後で\n\n- b\n"
    assert todo_add(todo, "実機のレーンから", ["- [ ] c"]) == \
        "# TODO\n\n## 実機のレーンから\n\n説明\n\n- [ ] a\n- [ ] c\n\n## 後で\n\n- b\n"
    assert todo_add("# TODO\n\n## 実機のレーンから\n\n- [ ] a\n", "実機のレーンから", ["- [ ] c"]) == \
        "# TODO\n\n## 実機のレーンから\n\n- [ ] a\n- [ ] c\n"

    status = ("<!-- Written -->\n**Lane** `win` · **Last firing** 2026-10-11 08:20 +09:00 · **Script** x\n\n"
              "**This firing:** Nothing new on main.\n")
    assert last_firing(status) == datetime.datetime(2026, 10, 10, 23, 20, tzinfo=datetime.timezone.utc)
    assert overdue_hours(status) == 3
    assert overdue_hours(status.replace("Nothing new on main.", "A run is going (started ...).")) == 5
    assert last_firing("**Last firing** 2026-10-11 08:20 -05:30") == \
        datetime.datetime(2026, 10, 11, 13, 50, tzinfo=datetime.timezone.utc)
    assert last_firing("no firing yet") is None

    have = {"bug": ("D73A4A", "Something isn't working"), "vote": ("ffffff", "old"), "other": ("000000", "")}
    changes = label_changes(have, LABELS)
    assert ("PATCH", "vote", *LABELS["vote"]) in changes
    assert not any(name in ("bug", "other") for _, name, _, _ in changes), changes
    assert len(changes) == len(LABELS) - 1, changes
    for method, name, colour, desc in changes:
        have[name] = (colour, desc)
    assert label_changes(have, LABELS) == []

    c, w, o = "OWNER", "OWNER", "NONE"
    assert tally([]) == "open"
    assert tally([(c, "vote[cloud]: 1 — the small one")]) == "open"
    assert tally([(c, "vote[cloud]: 1 — x"), (w, "vote[win]: 1 -- y")]) == "decided:1"
    assert tally([(c, "vote[cloud]: 1"), (w, "vote[win]: 2 — y")]) == "split"
    assert tally([(c, "vote[cloud]: owner — no evidence"), (w, "vote[win]: owner")]) == "split"
    assert tally([(c, "vote[cloud]: 1"), (w, "vote[win]: 2"), (w, "vote[win]: 1 — on second look")]) == "decided:1"
    assert tally([(c, "vote[cloud]: 1"), (w, "vote[win]: 1"), (c, "uncount[cloud]: wrong issue")]) == "open"
    assert tally([(c, "vote[cloud]: 1"), (o, "vote[win]: 1")]) == "open", "a stranger's vote counts"
    assert tally([(c, "vote[cloud]: 1"), ("BOT", "<!-- m -->\n**merge-lanes** relays:\n\nvote[win]: 1 — y")]) \
        == "decided:1"
    assert tally([("BOT", "回答: 2")]) == "open"
    assert tally([(c, "vote[cloud]: 1"), (w, "vote[win]: 1"), ("OWNER", "回答：2 にする")]) == "answered"
    assert tally([(o, "回答: 2")]) == "open"
    assert tally([(c, "Some prose.\nvote[cloud]: 3.\n")]) == "open"

    assert vote_labels("answered", {"vote", "question"}) == {"answered"}
    assert vote_labels("decided:1", {"vote"}) == {"vote-decided"}
    assert vote_labels("split", {"vote"}) == {"needs-owner"}
    assert vote_labels("open", {"vote", "question"}) == {"vote"}
    assert vote_labels("open", {"vote-decided"}) == {"vote"}
    assert vote_labels("open", {"needs-owner"}) == {"needs-owner"}

    m = RELAYED_VOTE.match("#412: 2 — the menu keeps it")
    assert m and (m[1], m[2], m[3]) == ("412", "2", "the menu keeps it")
    assert RELAYED_VOTE.match("Q57: 1 -- old style") is None
    body = finding_body(331, "x64", [("Proposals", "qa-reports/a.md", "Reword 2.62."),
                                     ("Votes", "PR 本文", "Q57: 1 -- old"),
                                     ("Proposals", "PR 本文", "Kit helpers.")])
    assert body.startswith(finding_marker(331) + "\n")
    global REPO
    saved = REPO
    for REPO, marker in (("uchmk/kura", "<!-- kura-lane:#331 -->"), ("uchmk/tsumugi", "<!-- tsumugi-lane:#331 -->")):
        assert finding_marker(331) == marker, finding_marker(331)
    REPO = saved
    assert set(wanted_labels(True)) == set(LABELS)
    assert wanted_labels(None) == {}
    for repo, rules in RULES.items():
        assert set(wanted_labels(rules.get("labels"))) <= set(LABELS), repo
        share = rules["share"]
        assert share is None or share["issues"] or share["todo_heading"], repo
        assert set(rules["lanes"]) <= set(MACHINES), repo
    assert wanted_labels(RULES["uchmk/yagura"]["labels"])["question"] == LABELS["question"]
    assert "retest" not in wanted_labels(RULES["uchmk/yagura"]["labels"])
    assert "### Proposals\n\n- Reword 2.62. (qa-reports/a.md)\n- Kit helpers. (PR 本文)\n" in body, body
    assert "### Queue" not in body and "### Votes\n\n- Q57: 1 -- old (PR 本文)\n" in body, body
    print("merge-lanes self-test: OK")


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    elif sys.argv[1:] == ["--labels"]:
        if not TOKEN or not REPO:
            sys.exit("merge-lanes: GITHUB_TOKEN and GITHUB_REPOSITORY are needed")
        if not (RULES.get(REPO) or {}).get("labels"):
            sys.exit(f"merge-lanes: no labels for {REPO!r} in RULES")
        ensure_labels(RULES[REPO]["labels"])
    else:
        main()
