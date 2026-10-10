# mimamori

A keyboard-driven system monitor for Windows, macOS and Linux, written in Rust
with [egui](https://github.com/emilk/egui). It is the sibling of
[filer](https://github.com/uchmk/filer) (a yazi-style file manager) and
[tsumugi](https://github.com/uchmk/tsumugi) (a terminal for AI CLI sessions),
and shares their look, their keys and their settings folder.

What it is for: seeing at a glance how the chip is busy. The CPU screen draws
one tile per physical core, grouped by the L3 cache they share (a CCD on
Ryzen, the P- and E-cores on a hybrid Intel chip), with a bar per SMT thread
inside each tile.

> **v0.7** has seven screens: overview, cpu, mem, gpu·npu, procs, svc and net. The other three tabs
> are there with their final numbers, so the keys will not move, and say
> "Not yet". See [TODO.md](TODO.md).

## Screens

| Key | Screen   | What it shows |
| --- | -------- | ------------- |
| `1` | overview | CPU / memory / swap gauges and the system; a big graph of the selected gauge over the compact core tiles; the top processes by CPU and by memory |
| `2` | cpu      | Groups and counts; the core tiles, the total over the last minute and a thread heatmap; the selected core's threads and the top CPU users |
| `3` | mem      | Used / available / swap; the usage bar and graph; the processes using the most memory |
| `4` | gpu·npu  | The GPUs and NPUs found, each with its usage over the last minute (and why a missing one is missing); the selected device's usage, engines, dedicated and shared memory and VRAM graph; its temperature, fan, power and clock, the reason for any it cannot read, and the processes using it (`Enter` opens one in procs) |
| `5` | procs    | Scope and sort; the process list (filter with `/`, sort with `s`, a tree of parents and children with `t`, only your own with `u`, end one with `x`); the selected process's details and graphs |
| `6` | svc      | Services: Windows' services, systemd's units, launchd's jobs (with their plists, so disabled jobs show too). Show all / running / stopped / failed, with counts by start type; the list (state, name, start, PID; filter with `/`; on Windows `u` switches it to the kernel and file-system drivers and back); the selected one's description, state, start, PID, last exit, account, command and file (`Enter` opens its process in procs). `S` starts, `x` stops and `R` restarts the selected one after asking; the question shows the same thing as a command line (`systemctl restart foo.service`, `launchctl kickstart -k gui/501/…`, `Restart-Service foo`, or `sc.exe stop foo` for a driver). Read every 5 s, and only while the screen shows |
| `7` | net      | The network interfaces, each with its traffic over the last minute, and counts of the sockets; every TCP and UDP socket (protocol, local and remote address, state, PID, process; filter with `/`, only the listening ones with `u`); the selected interface's received and sent graphs, adapter, kind, link speed, addresses, MAC, MTU and totals, and the selected socket (`Enter` opens its process in procs). Interfaces are read every interval; sockets every 2 s, and only while the screen shows |
| `8`–`0` | disk, sensors, sys | Not yet |

Every screen is three columns. Their widths change with the mouse (drag a
divider) or the keyboard, and are kept per screen in `state.toml`.

## Keys

| Key | Does |
| --- | ---- |
| `1`–`0`, `Tab`, `Shift+Tab` | Go to a screen, next screen, previous screen |
| `j` `k` / `↓` `↑` | Down, up |
| `gg` / `Home`, `G` / `End` | First row, last row |
| `Ctrl+d`, `Ctrl+u`, `PageDown`, `PageUp` | Half a page, a page |
| `h` `l` / `←` `→` | Column to the left, to the right |
| `Alt+Shift+←→↑↓` (macOS `Cmd+Ctrl+arrow`) | Move the focused column's divider (tsumugi's keys) |
| `Ctrl+Shift+e` (macOS `Cmd+Shift+e`) | Columns back to equal widths |
| `Enter` | Open the selection (a gauge's screen, a process in procs, a service's or a socket's process) |
| `/` | Filter processes, or on svc services by name and description, or on net sockets by protocol, address, state, PID and process (`Enter` keeps it, `Esc` drops it). One filter serves all three |
| `s` | Next sort column |
| `t` | Procs as a tree of parents and children, or a flat list. Siblings keep the sort; a process whose parent is filtered out hangs under its nearest shown ancestor |
| `u` | Procs' scope: every process, or only yours (the same as picking All / Mine in the left column). On net: every socket, or only the listening ones (TCP `LISTEN` and unconnected UDP). On svc (Windows): services, or drivers |
| `x` | End the selected process, after asking (`y` / `Enter` ends it, `n` / `Esc` keeps it). Windows uses `TerminateProcess`; macOS and Linux send SIGTERM, then SIGKILL if it is still there after 3 s. Works on procs and on the top lists of overview, cpu and mem. On svc it stops the selected service instead |
| `S`, `R` | On svc: start, restart the selected service, after asking. Windows asks the Service Control Manager (run mimamori as administrator); Linux runs `systemctl` (root or polkit); macOS runs `launchctl kickstart` / `kill` in the domain the list came from (system jobs need `sudo`); a job that is not loaded is loaded from its plist first (`launchctl bootstrap`), while a disabled one is left disabled and the toast says the `launchctl enable` line. A start on a running service, or a stop on a stopped one, only says so |
| `P` | Pause / resume sampling |
| `+` / `=`, `-` | Sample less often, more often (0.5–10 s) |
| `?` | Help: every key of the keymap |
| `Esc` | Clear the filter |
| `q` | Quit |

## Settings

Everything lives in one folder shared by uchmk's apps:

```text
<config dir>/uchmk/common.toml           settings every uchmk app reads
<config dir>/uchmk/mimamori/config.toml  mimamori's own settings
<config dir>/uchmk/mimamori/keymap.toml  laid over the built-in keymap
<config dir>/uchmk/mimamori/state.toml   the column widths (mimamori writes it)
```

`<config dir>` is `%APPDATA%` on Windows, `~/Library/Application Support` on
macOS and `~/.config` on Linux. `UCHMK_CONFIG_DIR` moves the whole `uchmk`
folder.

`common.toml`:

```toml
language = "auto"   # "auto" follows the OS; "en" (more to come)
```

`config.toml`:

```toml
interval = 1.0      # seconds between samples, 0.5 to 10
language = "en"     # overrides common.toml for mimamori alone
backend = "auto"    # graphics: "auto", "gl", "vulkan", "dx12", "metal"
```

`keymap.toml` uses yazi's format, section by section (`[main]`, `[input]`,
`[help]`, `[confirm]`): `keymap` replaces a section, `prepend_keymap` and `append_keymap`
add before and after it. `on_mac` gives a binding its own keys on macOS, and
`<P-…>` is the primary modifier (Cmd on macOS, Ctrl elsewhere). The built-in
keymap is [src/defaults/keymap.toml](src/defaults/keymap.toml).

```toml
[main]
prepend_keymap = [
    { on = "<C-r>", run = "pause", desc = "Pause / resume sampling" },
]
```

## Commands

What `run` can say in a keymap:

| Command | Does |
| ------- | ---- |
| `screen overview` … `screen sys`, `screen next`, `screen prev` | Change screen |
| `arrow N`, `arrow top`, `arrow bot`, `arrow 50%` | Move the selection by rows, to an end, by a share of the page |
| `focus left`, `focus right` | Move between columns |
| `resize left` / `right` / `up` / `down` | Move the focused column's divider |
| `equalize` | Columns back to equal widths |
| `enter`, `filter`, `sort`, `pause` | As the keys above |
| `interval +`, `interval -` | Sample less / more often |
| `kill` | Ask, then end the selected process; on svc, stop the selected service |
| `service start`, `service stop`, `service restart` | On svc: ask, then start, stop or restart the selected service |
| `tree` | Procs as a tree, or a flat list |
| `scope`, `scope all`, `scope mine` | Switch procs' scope, or set it |
| `help`, `escape`, `quit` | As the keys above |
| `confirm`, `backspace` | The filter line's keys (`[input]`); `confirm` and `escape` also answer the question before a process is ended or a service is started, stopped or restarted (`[confirm]`) |

## Languages

Every word on screen goes through one table: [src/lang/en.toml](src/lang/en.toml).
English is the only one in v0.1; a translation is another file with the same
keys, and any key it leaves out reads in English. The language is picked from
`config.toml`, then `common.toml`, then the OS.

## Building

```sh
cargo build --release
```

Pure Rust apart from the OS's own libraries; the six targets (Windows, macOS
and Linux on x86_64 and ARM64) need no C toolchain beyond the platform's
linker. The core layout comes from sysfs on Linux and
`GetLogicalProcessorInformationEx` on Windows; macOS draws one tile per
logical CPU for now.

The gpu·npu screen reads, without linking anything at build time:

| OS | GPUs | NPUs |
| --- | --- | --- |
| Windows | The `GPU Engine` and `GPU Adapter Memory` performance counters (PDH) for usage, engines, memory and per-process use, named through DXGI | One with an MCDM driver, the kind Task Manager shows, from the same counters |
| Linux | `/sys/class/drm`: amdgpu has usage, VRAM and hwmon sensors; i915 / xe have the clock and sensors but no busy counter | `/sys/class/accel`: `intel_vpu` has usage and clock; `amdxdna` is listed only |
| macOS | Not yet (IOKit) | The Neural Engine reports no usage to other programs |

The svc screen reads:

| OS | From | Notes |
| --- | --- | --- |
| Windows | The service control manager (`EnumServicesStatusExW`, and `QueryServiceConfigW` for the start type, account and command, kept a minute) | Drivers are read too, and shown instead of the services with `u`; a failed service shows its exit code |
| Linux | `systemctl list-unit-files` and `systemctl show '*.service'` | Without systemd running (a container, WSL without systemd) only the unit files are listed, their state unknown |
| macOS | `launchctl list` | As a user, that user's jobs; the system's need `sudo` |

The net screen reads:

| OS | Interfaces | Sockets |
| --- | --- | --- |
| Windows | The IP helper (`GetIfTable2`, `GetUnicastIpAddressTable`): the physical adapters, even unplugged, and any other that is up with an address (VPNs, Hyper-V switches), without the filter layers; Bluetooth PAN told apart by `PhysicalMediumType` | `GetExtendedTcpTable` / `GetExtendedUdpTable` with the owning PID |
| Linux | sysinfo for the counters and addresses; `/sys/class/net` for the kind (Bluetooth PAN from `DEVTYPE` in `uevent`), link speed and driver | `/proc/net/{tcp,tcp6,udp,udp6}`, the owner from the `socket:[inode]` links in `/proc/*/fd`. Another user's sockets show no process without root |
| macOS | sysinfo; the kind is guessed from the name | Not yet |

On Windows and Linux, NVIDIA cards are read through NVML, loaded from the
driver at run time (`nvml.dll` / `libnvidia-ml.so.1`); without it they are
listed from sysfs on Linux with nothing read, and have no sensors on Windows.

`mimamori --keys SCRIPT` presses keys once the window is up (as filer's
`--keys`), and `scripts/xrun.sh` runs it on a virtual X display and keeps the
screenshots: that is how changes are checked without a screen.

## License

MIT or Apache-2.0, at your option.
