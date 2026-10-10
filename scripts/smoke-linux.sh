#!/usr/bin/env bash
# Start mimamori on a virtual X display, visit every screen, and check that
# each one drew: CI's Linux smoke, the way filer's scripts/smoke-linux.sh is.
#
#   scripts/smoke-linux.sh [OUTDIR]     (default target/smoke)
#
# Runs scripts/xrun.sh with a `--keys` script that presses 1..0, and after
# each a `<Shot:>` and a `<State:>`, then opens the settings screen (`<C-,>`)
# and does the same. Fails when the script did not finish,
# when mimamori panicked, when a screen's state names another screen (or the
# settings are not open), or when
# a screenshot is one flat colour (nothing was drawn). OUTDIR keeps the
# pictures, the states and the log whether it passed or not. Needs what
# xrun.sh needs, and a GL driver (Mesa's llvmpipe is enough).

set -u
out="${1:-target/smoke}"
rm -rf -- "$out"
screens="1:overview 2:cpu 3:mem 4:gpu 5:procs 6:svc 7:net 8:disk 9:sensors 0:sys F:fleet"
keys='<Wait:2000>'
for s in $screens; do
    keys="$keys${s%%:*}<Wait:1000><Shot:${s#*:}><State:${s#*:}>"
done
keys="$keys<C-,><Wait:1000><Shot:settings><State:settings><Esc><Wait:300>"
XRUN_KEYS_TIMEOUT="${XRUN_KEYS_TIMEOUT:-240}" scripts/xrun.sh "$out" --keys "$keys" || exit 1

fail=0
no() { echo "smoke: $*" >&2; fail=1; }
[ -e "$out/keys.done" ] || no "the key script did not finish"
grep -q "panicked" "$out/mimamori.log" && no "mimamori panicked: $(grep -m1 panicked "$out/mimamori.log")"
for s in $screens S:settings; do
    name=${s#*:}
    if [ ! -s "$out/$name.png" ]; then
        no "$name: no screenshot"
        continue
    fi
    if [ "$name" = settings ]; then
        grep -qx "mode: Settings" "$out/$name.txt" 2>/dev/null || no "settings: the state says $(grep -m1 '^mode' "$out/$name.txt" 2>/dev/null)"
    else
        grep -qx "screen: $name" "$out/$name.txt" 2>/dev/null || no "$name: the state says $(grep -m1 '^screen' "$out/$name.txt" 2>/dev/null)"
    fi
    # A drawn screen has text, lines and graphs; an empty one is all one colour.
    sd=$(identify -format '%[fx:standard_deviation]' "$out/$name.png")
    awk -v sd="$sd" 'BEGIN { exit !(sd > 0.01) }' || no "$name: the screenshot is blank (deviation $sd)"
done
[ "$fail" = 0 ] && echo "smoke: every screen and the settings drew ($out)"
exit "$fail"
