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

> **v0.19** has all ten screens: overview, cpu, mem, gpu·npu, procs, svc, net, disk, sensors and sys.
> What is still missing on each OS is in [TODO.md](TODO.md).

## Screens

| Key | Screen   | What it shows |
| --- | -------- | ------------- |
| `1` | overview | CPU / memory / swap gauges and the system; a big graph of the selected gauge over the compact core tiles; the top processes by CPU and by memory |
| `2` | cpu      | Groups and counts; the core tiles, the total over the last minute and a thread heatmap; the selected core's threads and the top CPU users |
| `3` | mem      | Used / available / swap; the usage bar and graph; the processes using the most memory |
| `4` | gpu·npu  | The GPUs and NPUs found, each with its usage over the last minute (and why a missing one is missing); the selected device's usage, engines, dedicated and shared memory and VRAM graph; its temperature, fan, power and clock, the reason for any it cannot read, and the processes using it (`Enter` opens one in procs) |
| `5` | procs    | Scope and sort; the process list (filter with `/`, sort with `s`, a tree of parents and children with `t`, only your own with `u`, end one with `x`); the selected process's details and graphs |
| `6` | svc      | Services: Windows' services, systemd's units, launchd's jobs (with their plists, so disabled jobs show too). Show all / running / stopped / failed, with counts by start type; the list (state, name, start, PID; filter with `/`; on Windows `u` switches it to the kernel and file-system drivers and back); the selected one's description, state, start, PID, last exit, account, command and file (`Enter` opens its process in procs). `S` starts, `x` stops and `R` restarts the selected one after asking; the question shows the same thing as a command line (`systemctl restart foo.service`, `launchctl kickstart -k gui/501/…`, `Restart-Service foo`, or `sc.exe stop foo` for a driver). Read every 5 s, and only while the screen shows |
| `7` | net      | The network interfaces, each with its traffic over the last minute, and counts of the sockets; every TCP and UDP socket (protocol, local and remote address, state, PID, process; filter with `/`, only the listening ones with `u`; `Enter` on an interface keeps only the sockets on its addresses and those bound to any address, until `Esc`); the selected interface's received and sent graphs, adapter, kind, link speed, addresses, MAC, MTU and totals, and the selected socket (`Enter` opens its process in procs). Interfaces are read every interval; sockets every 2 s, and only while the screen shows |
| `8` | disk     | The drives, each with its read and write traffic over the last minute, and the totals of all of them; every mounted volume (mount point, device, file system, size, used, free, use % turning yellow over 75 % and red over 90 %, read and write per second); the selected drive's read, write and busy graphs, SMART health (green when good, red when the drive predicts its failure, and why when it cannot be read), temperature with the drive's warning mark, what SMART counts (life used, turning yellow from 80 % and red at 100 %; hours powered on; lifetime writes; remapped, pending and media errors, yellow from the first), model, kind (SSD / HDD), bus, size, totals since boot and volumes, and the selected volume with a bar of how full it is. `Enter` on a drive goes to its first volume, on a volume back to its drive. Read every interval |
| `9` | sensors  | The chips with sensors (the CPU, the drives, the board's monitor chip, the GPU), each with its hottest temperature over the last minute, and the hottest of all; every temperature and fan (chip, sensor, now, the lowest and highest of the last minute, the chip's high mark and critical point; a temperature turns yellow within 10 °C of its high mark and red past it, a fan below its least turns red); the selected sensor's graph with its marks, and where it was read. `Enter` on a chip goes to its first sensor, on a sensor back to its chip. Read every interval |
| `0` | sys      | What does not change while running: the system's maker and model, the OS, kernel, uptime and boot time, the processor (model, maker, socket, cores and threads, top speed, L1 / L2 / L3 cache), the firmware (maker, version, date, UEFI or legacy boot, a virtual machine's), the board, and the memory (installed, slots used, the most the board takes, error checking), the GPUs and NPUs (name, VRAM) and the drives (model, size), each list with the key of its own screen; every memory slot, filled or empty (size, type, speed, maker); the selected slot's bank, form factor, rated and configured speed, rank, voltage and part number. Read once at start, on a thread of its own |

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
| `Enter` | Open the selection (a gauge's screen, a process in procs, a service's or a socket's process). On a net interface: only the sockets on it. On disk: a drive's first volume, or a volume's drive. On sensors: a chip's first sensor, or a sensor's chip |
| `/` | Filter processes, or on svc services by name and description, or on net sockets by protocol, address, state, PID and process (`Enter` keeps it, `Esc` drops it). One filter serves all three |
| `s` | Next sort column |
| `t` | Procs as a tree of parents and children, or a flat list. Siblings keep the sort; a process whose parent is filtered out hangs under its nearest shown ancestor |
| `u` | Procs' scope: every process, or only yours (the same as picking All / Mine in the left column). On net: every socket, or only the listening ones (TCP `LISTEN` and unconnected UDP). On svc (Windows): services, or drivers |
| `x` | End the selected process, after asking (`y` / `Enter` ends it, `n` / `Esc` keeps it). Windows uses `TerminateProcess`; macOS and Linux send SIGTERM, then SIGKILL if it is still there after 3 s. Works on procs and on the top lists of overview, cpu and mem. On svc it stops the selected service instead |
| `S`, `R` | On svc: start, restart the selected service, after asking. Windows asks the Service Control Manager (run mimamori as administrator); Linux runs `systemctl` (root or polkit); macOS runs `launchctl kickstart` / `kill` in the domain the list came from (system jobs need `sudo`); a job that is not loaded is loaded from its plist first (`launchctl bootstrap`), while a disabled one is left disabled and the toast says the `launchctl enable` line. A start on a running service, or a stop on a stopped one, only says so |
| `P` | Pause / resume sampling |
| `+` / `=`, `-` | Sample less often, more often (0.5–10 s) |
| `?` | Help: every key of the keymap |
| `Esc` | Clear the filter, and on net the interface the sockets are narrowed to |
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
`GetLogicalProcessorInformationEx` on Windows. On Apple silicon it comes
from the device tree's `cpus` node in IOKit: a group per cluster (P or E
cores sharing one L2; there is no L3), with the L2's size from
`sysctl hw.perflevelN.l2cachesize`. Intel Macs use `hw.physicalcpu` /
`hw.logicalcpu` (threads side by side) and `hw.l3cachesize`.

The gpu·npu screen reads, without linking anything at build time:

| OS | GPUs | NPUs |
| --- | --- | --- |
| Windows | The `GPU Engine` and `GPU Adapter Memory` performance counters (PDH) for usage, engines, memory and per-process use, named through DXGI; temperature, fan and clock from D3DKMT (`D3DKMTQueryAdapterInfo`, what Task Manager shows) where the driver keeps them, and from NVML for NVIDIA | One with an MCDM driver, the kind Task Manager shows, from the same counters |
| Linux | `/sys/class/drm`: amdgpu has usage, VRAM and hwmon sensors; i915 / xe have the clock and sensors but no busy counter. Per-process use and memory come from the DRM fdinfo (`/proc/<pid>/fdinfo`, written by i915, xe, amdgpu, panfrost, msm…), and so does the usage of a GPU sysfs gives none for: only the processes mimamori may read count, so run it as root to see everyone's | `/sys/class/accel`: `intel_vpu` has usage and clock; `amdxdna` is listed only |
| macOS | IOKit's `IOAccelerator` services and their `PerformanceStatistics`: Apple silicon has usage, the renderer and tiler engines and the unified memory in use; Intel GPUs have usage; AMD cards also VRAM and, on some, temperature, fan, power and clock. No per-process use; the SMC's GPU temperatures are on the sensors screen | The Neural Engine reports no usage to other programs |

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
| macOS | sysinfo for the counters and addresses; `networksetup -listallhardwareports` for the kind and adapter (Wi-Fi, Ethernet, Bluetooth PAN, Thunderbolt Bridge), read once a minute; the rest (`utun`, `awdl`) by name | `netstat -anvW`, with the PID of the process that last used each socket |

The disk screen reads:

| OS | Drives | Volumes |
| --- | --- | --- |
| Windows | The `PhysicalDisk` performance counters (PDH) for the traffic and busy time; `IOCTL_STORAGE_QUERY_PROPERTY` and `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` on `\\.\PhysicalDriveN` for the model, bus, SSD / HDD and size, asked once per drive without administrator. Volumes are linked by their drive letters. If the counters are off (`diskperf -n`), the screen says so and shows the volumes alone | sysinfo |
| Linux | `/proc/diskstats` for the traffic, totals and busy time; `/sys/block` for the whole drives (not `loop`, `ram`, `zram` or partitions), their size, model, bus and `rotational`. Volumes are linked through the device they were mounted from | sysinfo |
| macOS | IOKit: each `IOBlockStorageDriver`'s `Statistics` for the traffic and totals since boot, the device above it for the model, SSD / HDD and bus, the whole disk below it for the BSD name (`disk0`) and size. Disk images are left out; macOS keeps no busy time, so there is no busy graph. Volumes are linked by walking up from the device they were mounted from (an APFS volume through its container to the partition it lives on) | sysinfo |

Each drive's SMART health is asked once a minute (and when the drives
change) in a thread of its own, so a slow drive never holds up the screen:

| OS | Health | Temperature | What SMART counts |
| --- | --- | --- | --- |
| Windows | `IOCTL_STORAGE_PREDICT_FAILURE`, skipped while the drive is spun down (`GetDevicePowerState`) so it is left asleep. Some drivers answer it only for an administrator, and the row says so. An NVMe drive's critical warning counts as failing too | `StorageDeviceTemperatureProperty` through `IOCTL_STORAGE_QUERY_PROPERTY`, without administrator: an SSD's every 5 seconds, any other drive's with the minute's check; else the NVMe health log's | NVMe: the SMART / health log through `IOCTL_STORAGE_QUERY_PROPERTY`'s `StorageDeviceProtocolSpecificProperty` (life used, hours, writes, media errors). SATA: the attributes from `SMART_RCV_DRIVE_DATA`'s SMART READ DATA (hours, remapped, pending, an SSD's life used, and the temperature from attribute 194 where the property gave none). Windows answers it only for an administrator; otherwise the drive shows `SMART counts: needs administrator` |
| Linux | `smartctl -H -A -n standby` (smartmontools), which leaves a spun-down drive asleep. It needs root or the `disk` group, and the row says so, or that smartctl is not installed; virtual disks (`virtio`, `xen`) have no SMART data | The drive's hwmon chip (`nvme`, `drivetemp`), else what smartctl read | The same smartctl output: NVMe's percentage used, hours, data units written and media errors; ATA's attributes 5 (reallocated), 9 (hours), 197 (pending) and the wear attribute (`Wear_Leveling_Count`, `SSD_Life_Left`…); SCSI's endurance, hours and grown defects. A drive left asleep keeps its last counts |
| macOS | `diskutil info -plist`'s `SMARTStatus` (internal drives; most USB enclosures pass nothing). An NVMe drive's critical warning, or an ATA attribute past its threshold, counts as failing too | The NVMe health log's; a SATA drive's attribute 194 (190 where there is none) | NVMe (the internal SSD, and enclosures that pass it through): the SMART / health log from IOKit's `IONVMeSMARTInterface`, without sudo (life used, hours, writes, media errors). SATA (older Macs' internal drives, enclosures that pass it through): the attributes from `IOATASMARTInterface`'s `SMARTReadData`, without sudo (hours, remapped, pending, an SSD's life used) |

The sensors screen reads:

| OS | Sensors |
| --- | --- |
| Windows | The ACPI thermal zones from the `Thermal Zone Information` performance counters (PDH), without administrator. Fans and the cores' own temperatures need the board maker's driver, so the screen says so; GPUs add their temperature and fan from D3DKMT (and NVML for NVIDIA), and the drives theirs from the disk reading |
| Linux | `/sys/class/hwmon`: every chip's `temp*` (with `max` and `crit`) and `fan*` (with `min`), named by the chip and, for drives, their model. Where no chip has a temperature (many ARM boards), the `/sys/class/thermal` zones and their trip points instead. NVIDIA GPUs add theirs from NVML; an AMD GPU's own chip is not listed twice. A drive with no hwmon chip (SATA without `drivetemp`) adds the temperature smartctl read |
| macOS | The SMC itself, through the `AppleSMC` user client: the temperatures with a known key, named (`TC0P` is "CPU proximity" on Intel Macs; Apple silicon's `Tp…` / `Te…` / `Tg…` families are numbered "CPU 1", "GPU 1"…), and every fan with its least and most rpm. On Apple silicon also the HID temperatures sysinfo reads, under their own chip with friendlier names ("CPU die 3", "SSD", "Battery") |

The sys screen reads:

| OS | Machine and memory slots |
| --- | --- |
| Windows | The firmware's SMBIOS table from `GetSystemFirmwareTable('RSMB')`, which needs no administrator (the processors from its types 4 and 7, the slots from 17), and the boot mode from `GetFirmwareType` |
| Linux | The SMBIOS table from `/sys/firmware/dmi/tables/DMI`, which only root may read. As a normal user, the world-readable `/sys/class/dmi/id` files give the system, board and firmware but no slots or processor details (the cores and threads then come from the core layout), and the screen says to run as root for them; boards without SMBIOS (many ARM boards) give the device tree's `model`. UEFI boot from `/sys/firmware/efi` |
| macOS | `system_profiler -xml SPHardwareDataType SPMemoryDataType`: the chip or CPU and its cores (and an Intel Mac's L3); an Intel Mac lists its slots; Apple silicon has one block of unified memory in the chip's package. Serial numbers are not shown |

On Windows and Linux, NVIDIA cards are read through NVML, loaded from the
driver at run time (`nvml.dll` / `libnvidia-ml.so.1`). On Linux NVML also
gives each process's memory and use (its busiest of the SM, encoder and
decoder, averaged since the last reading; Maxwell or newer); without it they are
listed from sysfs on Linux with nothing read, and on Windows keep only what
D3DKMT gives (temperature, fan and clock, no power).

`mimamori --keys SCRIPT` presses keys once the window is up (as filer's
`--keys`), and `scripts/xrun.sh` runs it on a virtual X display and keeps the
screenshots: that is how changes are checked without a screen.

## License

MIT or Apache-2.0, at your option.
