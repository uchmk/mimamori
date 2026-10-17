#!/usr/bin/env python3
# Source is uchmk/ito scripts/lanes/; an app's copy is written by its scripts/lanes.sh sync, so edit ito's.
#
# "ito first": fail when an app's crate depends directly on a crate that an
# ito crate already stands in for (the list is ito-first.txt beside this file).
# What ito has, the app uses; what it lacks, the app asks ito for (an issue in
# uchmk/ito, or the ito session) rather than writing its own.
#
#   scripts/ito-first.py            check the workspace in the current folder
#   scripts/ito-first.py --self-test
#
# The other way too: ito-required.txt lists the ito crates every app depends
# on (the --keys script, keymap, help, notifications, F12 report, window
# place, checklists ...), so a new app cannot leave one out unnoticed.
#
# An app that keeps one on purpose, or goes without a required one, names it,
# with the reason, in scripts/ito-first.allow (`<crate> <why>`); a line
# without a reason does not count.

import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def read_pairs(path):
    """`name rest` lines of a file; comments and blank lines skipped. {} when missing."""
    out = {}
    try:
        lines = open(path, encoding="utf-8").read().splitlines()
    except FileNotFoundError:
        return out
    for line in lines:
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        name, _, rest = line.partition(" ")
        out[name] = rest.strip()
    return out


def offenders(packages, banned, allowed):
    """(member, dependency, instead) for each direct dependency of a member on a banned crate."""
    found = []
    for p in packages:
        for d in p["dependencies"]:
            name = d["name"]
            if name in banned and not allowed.get(name):
                found.append((p["name"], name, banned[name]))
    return sorted(set(found))


def missing_required(packages, required, allowed):
    """(crate, what) for each required ito crate no member depends on and the app has not excused."""
    have = {d["name"] for p in packages for d in p["dependencies"]}
    return sorted((n, w) for n, w in required.items() if n not in have and not allowed.get(n))


def check():
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "-q", "--no-deps", "--format-version", "1"],
            check=True, capture_output=True, text=True,
        ).stdout
    )
    members = set(meta["workspace_members"])
    packages = [p for p in meta["packages"] if p["id"] in members]
    banned = read_pairs(os.path.join(HERE, "ito-first.txt"))
    required = read_pairs(os.path.join(HERE, "ito-required.txt"))
    allowed = read_pairs(os.path.join(HERE, "ito-first.allow"))
    bad = offenders(packages, banned, allowed)
    missing = missing_required(packages, required, allowed)
    for name, what in missing:
        print(f"ito-first: no crate depends on `{name}` ({what}); add it, or say why not in scripts/ito-first.allow")
    for member, dep, instead in bad:
        print(f"ito-first: {member} depends on `{dep}`; use {instead}")
    stale = sorted(n for n in allowed if n not in banned and n not in required)
    for n in stale:
        print(f"ito-first: scripts/ito-first.allow names `{n}`, which ito-first.txt does not ban (drop the line)")
    if bad:
        print("ito-first: use the ito crate, or ask ito for what it lacks (an issue in uchmk/ito).")
        print("ito-first: to keep one on purpose, add `<crate> <reason>` to scripts/ito-first.allow.")
    return 1 if bad or stale or missing else 0


def self_test():
    banned = {"dirs": "ito-common", "notify": "ito-common"}
    pkgs = [
        {"name": "app", "dependencies": [{"name": "dirs"}, {"name": "serde"}]},
        {"name": "core", "dependencies": [{"name": "notify"}]},
    ]
    assert offenders(pkgs, banned, {}) == [("app", "dirs", "ito-common"), ("core", "notify", "ito-common")]
    assert offenders(pkgs, banned, {"dirs": "ito-common has no per-machine folder"}) == [("core", "notify", "ito-common")]
    assert offenders(pkgs, banned, {"dirs": ""}) == offenders(pkgs, banned, {})
    assert offenders([], banned, {}) == []
    need = {"ito-help": "key help", "ito-notify": "notices"}
    pk = [{"name": "app", "dependencies": [{"name": "ito-help"}]}]
    assert missing_required(pk, need, {}) == [("ito-notify", "notices")]
    assert missing_required(pk, need, {"ito-notify": "a library, no window"}) == []
    assert missing_required(pk, need, {"ito-notify": ""}) == [("ito-notify", "notices")]
    assert "toml_edit" in read_pairs(os.path.join(HERE, "ito-first.txt"))
    assert "ito-keyscript" in read_pairs(os.path.join(HERE, "ito-required.txt"))
    print("ito-first: self-test OK")


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    else:
        sys.exit(check())
