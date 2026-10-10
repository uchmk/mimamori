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

## 3a. GPU and NPU (`4`)

- [ ] 3a.1 Windows, AMD or Intel GPU: the device is listed with its name from DXGI; usage follows a game or a video in Task Manager's GPU page within a second or two; the engines (3D, Copy, Video Decode…) and the dedicated / shared memory match Task Manager; `Source` says `pdh`.
- [ ] 3a.2 Windows, NVIDIA: as 3a.1, and temperature, fan (%), power and clock are filled in from NVML and `Source` says `pdh+nvml`; with NVML missing (rename `nvml.dll` for the test) the four say `—` and the reason is listed under `Unavailable`.
- [ ] 3a.3 Windows: the `Processes on it` list shows the game / browser using the GPU with its usage and dedicated memory; `Enter` on the device moves to the list, `Enter` on a process opens procs with it selected.
- [ ] 3a.4 Windows, Intel Core Ultra or Snapdragon X: the NPU is listed as `NPU0` (magenta) with its usage while something runs on it (Windows Studio Effects, for example); with no NPU the left column says why.
- [ ] 3a.5 Linux, amdgpu: usage, VRAM, GTT (shared), temperature, fan (rpm), power and clock are filled in and move with load; `Source` says `sysfs`.
- [ ] 3a.6 Linux, Intel iGPU (i915 or xe): the device is listed with its clock; usage says why it is missing (no busy counter).
- [ ] 3a.7 Linux, NVIDIA with the proprietary driver: the card comes from NVML (`Source` says `nvml`), once, not also from sysfs; its processes are listed with their memory.
- [ ] 3a.8 Linux, Intel NPU (`intel_vpu`): `NPU0` shows usage while an OpenVINO job runs on it, and its clock.
- [ ] 3a.9 macOS: the left column says `No GPU` / `No NPU` with the reasons (not built yet; the Neural Engine reports nothing).
- [ ] 3a.10 A machine with two GPUs (an iGPU and a dGPU): both are listed, `j` / `k` switch between them, and the crumb names the selected one.

## 3b. Services (`6`)

- [ ] 3b.1 Windows: the list matches services.msc (names are the service names, `Description` the display name); running ones show their PID; `Starts` says `auto (delayed)` for a delayed one; `Source` says `scm`.
- [ ] 3b.2 Windows: start and stop a service in services.msc; within 5 s the state follows (`starting` / `stopping` on the way), and a service that failed shows `failed` with its exit code.
- [ ] 3b.3 Linux with systemd: the list matches `systemctl list-units --type=service --all` plus the unit files; `failed` ones are red with their exit status; `Starts` says `enabled` / `static` / `masked`; `File` is the unit file.
- [ ] 3b.4 macOS: `launchctl list`'s jobs are listed with their PIDs and last exit; started without `sudo` the left column says only the user's jobs are shown.
- [ ] 3b.5 The left column's rows (All / running / stopped / failed) narrow the list and their counts match it; `/` filters by name or description and the selection stays on the same service while the list refreshes.
- [ ] 3b.6 `Enter` on a running service opens procs with its process selected; on one without a process the toast says so.
- [ ] 3b.7 Leaving svc stops the reading (no `systemctl` / `launchctl` in procs a few seconds after going to another screen).

## 3c. Network (`7`)

- [ ] 3c.1 Windows: the interfaces are the adapters in Settings → Network (an unplugged Ethernet shows `down`, a VPN shows while connected), without the `-WFP` / `-QoS` filter rows; `Adapter` is the adapter's name, `Link` its speed, and IPv4 / IPv6 / MAC match `ipconfig /all`.
- [ ] 3c.2 Windows: download a large file; the interface's `Received` graph and the rate in the left column follow Task Manager's Ethernet / Wi-Fi graph within a second or two.
- [ ] 3c.3 Windows: the sockets match `netstat -ano` (protocol, addresses, state, PID); `Process` names the owner, `Source` says `iphlpapi`, and run as a normal user the system's sockets still show their PIDs.
- [ ] 3c.4 Linux: the sockets match `ss -tuanp`; run without root, the left column says some sockets are another user's and those rows have no PID; run with `sudo` every row has one.
- [ ] 3c.5 Linux: a Wi-Fi interface reads `wireless`, a WireGuard or tun one `VPN / tunnel`, `docker0` `virtual`; `Link` is the speed `ethtool` reports, or `up` when it reports none.
- [ ] 3c.6 macOS: the interfaces are listed with traffic (`en0`, `utun*` as VPN / tunnel, `lo0` as loopback); the socket list says it is not read on macOS yet.
- [ ] 3c.7 `u` keeps only the listening sockets and back; `/` filters by address, port, state or process name, and the selection stays on the same socket while the list refreshes; `Enter` on a socket with a PID opens procs with that process selected, on one without a toast says so.
- [ ] 3c.8 Leaving net stops reading the sockets (mimamori's own CPU in procs drops back a few seconds after going to another screen); coming back shows the last list at once and refreshes it.

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
