#!/usr/bin/env bash
# Run yagura once on a virtual X display and keep what it showed, the way
# kura's scripts/xrun.sh does.
#
#   scripts/xrun.sh OUTDIR [yagura arguments...]
#
# Starts its own Xvfb, launches yagura with the arguments (`--keys`
# included), and with `--keys` waits until the last step has run
# (YAGURA_KEYS_DONE, at most XRUN_KEYS_TIMEOUT seconds, default 120), then
# XRUN_WAIT seconds more (default 1). Without `--keys` it waits XRUN_WAIT
# seconds (default 3). Writes into OUTDIR:
#
#   shot.png       the whole screen
#   yagura.log     what yagura printed
#   keys.done      there when every `--keys` step ran (last line `keys: done`)
#   cpu.txt        yagura's own CPU time over the run (`ps` TIME, then the
#                  last XRUN_IDLE seconds of it, default 0 = skipped)
#
# `<Shot:name>` and `<State:name>` steps write name.png / name.txt into the
# folder YAGURA_KEYS_DONE is in, which is OUTDIR. YAGURA_BIN overrides the
# binary (default target/debug/yagura). The settings are read from
# OUTDIR/config (UCHMK_CONFIG_DIR), so the column widths a run leaves behind
# do not carry into the next one or into the machine's own config; put files
# there first to test them. Needs Xvfb, xdotool, ImageMagick.

set -u
out="${1:?usage: scripts/xrun.sh OUTDIR [yagura arguments...]}"
shift
mkdir -p -- "$out"
out=$(cd -- "$out" && pwd)
bin="${YAGURA_BIN:-target/debug/yagura}"
keyed=0
for a in "$@"; do [ "$a" = "--keys" ] && keyed=1; done
if [ "$keyed" = 1 ]; then wait_s="${XRUN_WAIT:-1}"; else wait_s="${XRUN_WAIT:-3}"; fi
keys_timeout="${XRUN_KEYS_TIMEOUT:-120}"
idle="${XRUN_IDLE:-0}"
rm -f -- "$out/keys.done"

for tool in Xvfb xdotool import; do
    command -v "$tool" >/dev/null || { echo "xrun: $tool is missing (apt-get install -y xvfb xdotool imagemagick)" >&2; exit 2; }
done

n=90
while [ -e "/tmp/.X11-unix/X$n" ] || [ -e "/tmp/.X$n-lock" ]; do n=$((n + 1)); done
Xvfb ":$n" -screen 0 1400x900x24 -nolisten tcp >/dev/null 2>&1 &
xvfb=$!
trap 'kill "$xvfb" 2>/dev/null' EXIT
export DISPLAY=":$n"
for _ in $(seq 1 50); do xdotool getdisplaygeometry >/dev/null 2>&1 && break; sleep 0.1; done

mkdir -p -- "$out/config"
UCHMK_CONFIG_DIR="$out/config" YAGURA_KEYS_DONE="$out/keys.done" "$bin" "$@" >"$out/yagura.log" 2>&1 &
pid=$!
win=""
for _ in $(seq 1 100); do
    win=$(xdotool search --pid "$pid" --name . 2>/dev/null | head -1)
    [ -n "$win" ] && break
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.2
done
if [ -z "$win" ]; then
    echo "xrun: no window appeared; see $out/yagura.log" >&2
    kill "$pid" 2>/dev/null
    exit 1
fi
if [ "$keyed" = 1 ]; then
    for _ in $(seq 1 $((keys_timeout * 5))); do
        [ -e "$out/keys.done" ] && break
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.2
    done
    [ -e "$out/keys.done" ] || echo "xrun: --keys did not finish within ${keys_timeout}s; this is a half-pressed result" >&2
fi
sleep "$wait_s"

import -window root "$out/shot.png"
if [ "$idle" -gt 0 ] 2>/dev/null && kill -0 "$pid" 2>/dev/null; then
    ticks() { awk '{print $14 + $15}' "/proc/$pid/stat"; }
    a=$(ticks)
    sleep "$idle"
    b=$(ticks)
    hz=$(getconf CLK_TCK)
    awk -v a="$a" -v b="$b" -v hz="$hz" -v s="$idle" \
        'BEGIN { printf "idle: %.2f%% of one core over %d s\n", (b - a) / hz / s * 100, s }' >"$out/cpu.txt"
fi
kill -0 "$pid" 2>/dev/null && ps -o time= -p "$pid" | sed 's/^/total: /' >>"$out/cpu.txt"

kill "$pid" 2>/dev/null
wait "$pid" 2>/dev/null
echo "xrun: $out"
