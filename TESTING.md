# Manual tests

Things only a real machine with a screen can show. Cloud sessions check what
they can on Xvfb (`scripts/xrun.sh`); the rows here are what is left. Only a
machine session or the owner ticks `[x]`; a row whose behaviour changes goes
back to `[ ]`.

## 1. Start and idle

- [ ] 1.1 Windows x64: `mimamori.exe` opens on the overview within a second or two; the tabs, the three columns and the status bar are drawn; no console window appears.
- [ ] 1.2 Windows ARM64: the same.
- [ ] 1.3 macOS (Apple Silicon): the same; the window can be moved and resized normally.
- [ ] 1.4 Linux (X11 and Wayland): the same.
- [ ] 1.5 Idle CPU on a real GPU: with sampling every 1 s, mimamori stays under about 1% of one core in Task Manager / Activity Monitor / top; with `P` (paused) it drops to about 0%.
- [ ] 1.6 Minimised for a minute and restored: the graphs carry on from where they were, with no burst of CPU on restore.

## 2. Core layout (cpu screen, `2`)

- [ ] 2.1 Windows, Ryzen with two CCDs: two groups labelled `L3 #0` and `L3 #1`, each with its cores; every tile has two thread bars (SMT); `Topology from` says `win32`.
- [ ] 2.2 Windows, hybrid Intel (12th gen or later): groups `P-cores` and `E-cores`; P-core tiles have two thread bars, E-core tiles one.
- [ ] 2.3 Windows ARM64 (Snapdragon): note what groups and kinds are shown and whether they match the chip.
- [ ] 2.4 macOS: one group `CPU` with one tile per logical CPU, `Topology from` says `fallback` (expected until TODO 2).
- [ ] 2.5 A tile's bars follow load: run a single-threaded busy loop pinned to one CPU and see that one bar (and that core's tile) turn yellow / red while the others stay blue.
- [ ] 2.6 `h` `j` `k` `l` in the middle column move the highlighted tile; the right column follows with the core's group, kind, thread ids and clock.

## 3. Processes (`5`)

- [ ] 3.1 Windows: the `USER` column shows user names (not blank) for your own processes.
- [ ] 3.2 `/` then typing filters the list as you type; `Enter` keeps the filter, `Esc` drops it.
- [ ] 3.3 `s` cycles the sort column and the header shows which one; the toast says `Sort: …`.
- [ ] 3.4 The selected process's CPU and memory graphs keep moving while it is selected, and the selection stays on the same process as the list re-sorts.
- [ ] 3.5 Windows: start Notepad, select it in procs, `x` then `y`: Notepad closes at once and the toast says `notepad.exe (…) ended`.
- [ ] 3.6 Windows: `x` then `y` on a SYSTEM process (for example `services.exe`) without elevation: nothing ends and the toast says `Could not end … Access is denied.`
- [ ] 3.7 macOS / Linux: `sh -c "trap '' TERM; while :; do sleep 1; done"` in a terminal, then `x` `y` on it: the toast says `Ending …`, then about 3 s later `… ignored SIGTERM; killed`. A plain `sleep 600` ends at once with `… ended`.
- [ ] 3.8 `x` shows the question over a dimmed screen with the process's name, PID, user and command; `n`, `Esc` and `q` close it and the process keeps running; `x` on mimamori itself only says so.
- [ ] 3.9 `t` shows procs as a tree: children under their parent with `├ │ └` branch lines in line with the names, siblings in the sort order, and the left column says `View Tree`; `t` again goes back to the list. Windows: a child whose parent has exited is a root, not under a newer process that took the parent's PID.
- [ ] 3.10 `u` switches the scope between All and Mine (the left column's selection follows, the toast says `Scope: …`), and the selected process stays selected when it is in both.

## 4. Resizing the columns

- [ ] 4.1 Mouse: hovering a divider shows the resize cursor; dragging it moves it smoothly and the panels re-lay out while dragging.
- [ ] 4.2 Keyboard: `Alt+Shift+Left/Right` (macOS `Cmd+Ctrl+Left/Right`) moves the focused column's divider; `Alt+Shift+Up` says "No divider on that side".
- [ ] 4.3 `Ctrl+Shift+e` (macOS `Cmd+Shift+e`) puts the columns back to equal widths.
- [ ] 4.4 Quit and start again: each screen keeps the widths it was left with.
- [ ] 4.5 macOS: the `Cmd` bindings work and the `Ctrl` ones of other platforms do not fire by accident.

## 5. Settings

- [ ] 5.1 `common.toml` with `language = "en"` and a `mimamori/config.toml` with `interval = 2`: the status bar says `every 2 s`.
- [ ] 5.2 A `keymap.toml` with `prepend_keymap` adding a key: the key works and appears in `?` help.
- [ ] 5.3 A broken `config.toml` (bad TOML): mimamori still starts with defaults and says what was wrong.
- [ ] 5.4 Windows, AMD GPU: if the default backend crashes (see TODO 2), `backend = "dx12"` in `config.toml` starts cleanly.
