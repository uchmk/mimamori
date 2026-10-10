#!/usr/bin/env bash
# Every check a push to main must pass, in the order CI would fail them, from
# a Linux machine (a cloud session). The first failure stops with its output.
#
#   scripts/verify.sh
#
# The same steps as filer's, plus a macOS clippy. The Windows and macOS
# clippy runs are type checks only -- they never run that code.

set -o pipefail
cd "$(dirname "$0")/.." || exit 1

for t in x86_64-pc-windows-msvc aarch64-apple-darwin; do
    rustup target list --installed 2>/dev/null | grep -qx "$t" || rustup target add "$t" >/dev/null 2>&1
done

run() {
    local out rc
    out=$("$@" 2>&1)
    rc=$?
    if [ $rc -ne 0 ]; then
        echo "FAILED: $*"
        echo "$out" | tail -30
        exit 1
    fi
    last=$out
}

# `--locked` first: CI builds that way, and a plain cargo command quietly
# rewrites a stale Cargo.lock. When this fails, run `cargo build` and commit
# Cargo.lock with the version bump.
run cargo build -q --locked
run cargo fmt --check
run cargo test -q
tests=$(echo "$last" | grep -m1 'test result')
run cargo +stable clippy -q --all-targets -- -D warnings
run cargo +stable clippy -q --all-targets --target x86_64-pc-windows-msvc -- -D warnings
run cargo +stable clippy -q --all-targets --target aarch64-apple-darwin -- -D warnings
echo "ALL OK: $tests"
