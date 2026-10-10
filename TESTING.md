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
- [ ] 3b.4 macOS: `launchctl list`'s jobs are listed with their PIDs and last exit; started without `sudo` the left column says only the user's jobs are shown. The agents in `~/Library/LaunchAgents`, `/Library/LaunchAgents` and `/System/Library/LaunchAgents` (with `sudo`: the daemons in `/Library/LaunchDaemons` and `/System/Library/LaunchDaemons`) show their `Command`, `File` and `Starts` (`at load`, `keep alive`, `on demand` …); a job switched off (`launchctl print-disabled`) is listed as stopped and `disabled` even though `launchctl list` leaves it out; with `sudo`, daemons without `UserName` say `Runs as root`.
- [ ] 3b.5 The left column's rows (All / running / stopped / failed) narrow the list and their counts match it; `/` filters by name or description and the selection stays on the same service while the list refreshes.
- [ ] 3b.6 `Enter` on a running service opens procs with its process selected; on one without a process the toast says so.
- [ ] 3b.7 Leaving svc stops the reading (no `systemctl` / `launchctl` in procs a few seconds after going to another screen).
- [ ] 3b.8 Windows, mimamori run as administrator: `x` on a running service (e.g. `Spooler`) asks "Stop service?" with `Same as: Stop-Service Spooler`; `y` stops it, the toast says stopped and the list shows `stopped` at once. `S` starts it again; `R` on a running one stops and starts it (the PID changes).
- [ ] 3b.9 Windows, mimamori run without administrator: `x` / `S` / `R` and `y` give the toast "needs administrator — run mimamori as administrator", and the service is unchanged.
- [ ] 3b.10 Linux with systemd: as an ordinary user on a desktop, `R` and `y` on a system unit brings up the polkit password window; cancelling it gives the "needs root or polkit" toast. Run with `sudo`, `x`, `S` and `R` work and the list follows.
- [ ] 3b.11 macOS: without `sudo`, `R` on one of the user's own jobs (gui domain) restarts it (the PID changes); with `sudo`, `x` on a system job stops it, and the question says a KeepAlive job starts again by itself.
- [ ] 3b.12 Every OS: `S` on a running service and `x` on a stopped one only show a toast (no question); `n` / `Esc` / `q` at the question leave the service as it was; `x` on procs still ends a process.
- [ ] 3b.13 macOS: a job listed as `stopped (not loaded)` (for example after `launchctl bootout gui/$UID/<label>`): `S` asks with `launchctl bootstrap gui/<uid> <plist> && launchctl kickstart gui/<uid>/<label>` on the question, and `y` loads and starts it (it gets a PID; `launchctl list` shows it again). On one that is also `disabled` (`launchctl disable gui/$UID/<label>`), `S` only shows a toast with the `launchctl enable …` line, and `launchctl print-disabled gui/$UID` still says it is disabled.
- [ ] 3b.14 Windows: `u` on svc switches the list to drivers (for example `Tcpip`, `disk`, `Ntfs`; the left column says Kind: drivers, with their start counts) and back to services, with a toast each time; the selection goes to the top. As administrator, `x` on a driver that can stop asks with `sc.exe stop <name>` on the question. On macOS and Linux `u` on svc only shows the "Only Windows lists drivers here" toast.

## 3c. Network (`7`)

- [ ] 3c.1 Windows: the interfaces are the adapters in Settings → Network (an unplugged Ethernet shows `down`, a VPN shows while connected, `Bluetooth Network Connection` shows with the kind `Bluetooth`), without the `-WFP` / `-QoS` filter rows; `Adapter` is the adapter's name, `Link` its speed, and IPv4 / IPv6 / MAC match `ipconfig /all`.
- [ ] 3c.2 Windows: download a large file; the interface's `Received` graph and the rate in the left column follow Task Manager's Ethernet / Wi-Fi graph within a second or two.
- [ ] 3c.3 Windows: the sockets match `netstat -ano` (protocol, addresses, state, PID); `Process` names the owner, `Source` says `iphlpapi`, and run as a normal user the system's sockets still show their PIDs.
- [ ] 3c.4 Linux: the sockets match `ss -tuanp`; run without root, the left column says some sockets are another user's and those rows have no PID; run with `sudo` every row has one.
- [ ] 3c.5 Linux: a Wi-Fi interface reads `wireless`, a WireGuard or tun one `VPN / tunnel`, `docker0` `virtual`, a Bluetooth PAN (`bnep0`, while a phone tethers) `Bluetooth`; `Link` is the speed `ethtool` reports, or `up` when it reports none.
- [ ] 3c.6 macOS: the interfaces are listed with traffic; `Adapter` is the hardware port of System Settings → Network (`Wi-Fi`, `Ethernet`, `Thunderbolt Bridge`) and the kind follows it (`wireless`, `wired`, `virtual`; a Bluetooth PAN while an iPhone tethers over Bluetooth reads `Bluetooth`); `utun*` is VPN / tunnel and `lo0` loopback.
- [ ] 3c.9 macOS: the sockets match `sudo netstat -anv` (protocol, addresses, state) and `Source` says `netstat`; `Process` names the owner (compare `lsof -i -P -n` for a few). Run as a normal user, the rows carry PIDs too and the left column says nothing about other users unless some row lacks one; `u` keeps the `LISTEN` ones and the unconnected UDP.
- [ ] 3c.7 `u` keeps only the listening sockets and back; `/` filters by address, port, state or process name, and the selection stays on the same socket while the list refreshes; `Enter` on a socket with a PID opens procs with that process selected, on one without a toast says so.
- [ ] 3c.10 `Enter` on an interface narrows the sockets to those whose local address is one of its addresses, plus those bound to any address (`0.0.0.0`, `::`); the middle title says the interface's name and a toast says how to undo it. Moving to another interface follows it, and `Esc` shows every socket again. Compare with `netstat -ano` (Windows) / `ss -tuan` (Linux) / `netstat -anv` (macOS) filtered by the interface's IP.
- [ ] 3c.8 Leaving net stops reading the sockets (mimamori's own CPU in procs drops back a few seconds after going to another screen); coming back shows the last list at once and refreshes it.

## 3d. Disks (`8`)

- [ ] 3d.1 Windows: the drives are `Disk 0`, `Disk 1`… as in Disk Management; the right column's model, bus (`NVMe`, `SATA`, `USB`), kind (`SSD` / `HDD`) and size match Task Manager's Performance → Disk page and `Get-PhysicalDisk`, run as a normal user; `Volumes` lists the drive letters on it.
- [ ] 3d.2 Windows: copy a large file to a drive; its `Written` graph and the rate in the left column follow Task Manager's disk graph within a second or two, and `Busy` follows its active time.
- [ ] 3d.3 Windows: the volumes are the drive letters with the size, free space and file system of Explorer's This PC; a USB stick plugged in appears within a second or two (as a removable drive and volume) and goes when ejected.
- [ ] 3d.4 Linux: the drives match `lsblk -d` without the `loop` devices; an NVMe drive reads `NVMe` and `SSD`, a spinning one `HDD`; the totals since boot match `/proc/diskstats`; `dd if=/dev/zero of=big bs=1M count=2000 oflag=direct` shows in `Written` and `Busy` (compare `iostat -x 1`).
- [ ] 3d.5 Linux: the volumes match `df -h` for the real file systems; `/` and `/home` on LVM or LUKS (`/dev/mapper/…`) are linked to the drive under them (`Drive` in the volume rows, `Enter` goes there).
- [ ] 3d.6 macOS: the drives match `diskutil list physical` (`disk0` and any external drive, no disk images from a mounted `.dmg`); the internal one reads its model (`APPLE SSD …`), `SSD` and `Apple Fabric` (or `PCIe` on an Intel Mac), a USB stick `USB` and removable; there is no busy graph; copying a large file moves `Read` / `Written` and the totals since boot grow; `/` and `/System/Volumes/Data` are linked to `disk0` (`Drive` in the volume rows, `Enter` goes there), and the volumes still match `df -h` / Disk Utility.
- [ ] 3d.8 Windows: as a normal user, an NVMe or SATA SSD shows `Health: good` and a `Temperature` that matches CrystalDiskInfo (or `Get-PhysicalDisk | Get-StorageReliabilityCounter` as administrator) within a degree or two, with its warning mark; the temperature follows a long copy within a few seconds. Note any drive that says `needs administrator` or `no SMART data`, then run mimamori as administrator and note what it says.
- [ ] 3d.9 Windows: a spinning HDD left idle until it spins down (Power Options → Turn off hard disk after 1 minute) shows `spun down, left asleep` within a minute and does not spin up again on its own (listen, or watch its power state in CrystalDiskInfo); after it is woken (open a file on it) its health and temperature come back within a minute. A USB stick says `no SMART data`.
- [ ] 3d.10 Linux: without smartmontools the health says `needs smartctl (smartmontools)`; with it, as a normal user, a SATA or NVMe drive says `needs root`; as root (or in the `disk` group) it says `good`, matching `smartctl -H`. A SATA drive without the `drivetemp` module shows smartctl's temperature (`smartctl -A`, attribute 194); with `modprobe drivetemp` it shows the hwmon one. A spun-down drive (`hdparm -y /dev/sdX`) says `spun down, left asleep` and stays down (`hdparm -C`).
- [ ] 3d.11 macOS: the internal drive shows `Health: good`, matching `diskutil info disk0 | grep SMART` (`Verified`); a USB drive says `no SMART data` and has no temperature row.
- [ ] 3d.15 macOS (Apple Silicon and an Intel Mac if at hand): without sudo, the internal SSD's right column shows `Temperature` and `Life used`, `Powered on`, `Lifetime writes` and `Media errors`, matching `smartctl -a disk0` (smartmontools: Temperature, Percentage Used, Power On Hours, Data Units Written, Media and Data Integrity Errors). The sensors screen lists the drive's temperature as `disk0`; note if it repeats an SMC sensor of the same SSD. An NVMe drive in a USB or Thunderbolt enclosure shows the rows only if the enclosure passes SMART through.
- [ ] 3d.16 macOS (a Mac with an internal SATA drive, or an enclosure that passes SMART through): as a normal user, the drive shows a `Temperature`, `Powered on`, `Remapped sectors`, `Pending sectors` and, for an SSD, `Life used`, matching `smartctl -a diskN` (attributes 194, 9, 5, 197 and the wear attribute); the temperature is listed once on the sensors screen. A USB enclosure that passes nothing shows none of these rows and the health as before.
- [ ] 3d.13 Windows: as a normal user, an NVMe drive's right column shows `Life used`, `Powered on`, `Lifetime writes` and `Media errors` matching CrystalDiskInfo's Percentage Used, Power On Hours, Total Host Writes and Media and Data Integrity Errors. Note if the rows appear only as administrator. A SATA drive shows `SMART counts: needs administrator` instead (see 3d.17); a drive whose life used is 80 % or more reads it in yellow.
- [ ] 3d.17 Windows (a SATA drive): run mimamori as administrator; the drive shows `Powered on`, `Remapped sectors`, `Pending sectors` and, for an SSD, `Life used`, matching CrystalDiskInfo's Power On Hours, Reallocated Sectors Count, Current Pending Sector Count and the wear attribute (Wear Leveling Count, SSD Life Left…). As a normal user the same drive shows one dim row `SMART counts: needs administrator` and nothing else changes. A USB enclosure that passes nothing shows no SMART rows.
- [ ] 3d.14 Linux: as root, an NVMe drive shows `Life used`, `Powered on`, `Lifetime writes` and `Media errors` matching `smartctl -A`; a SATA drive shows `Powered on`, `Remapped sectors` and `Pending sectors` (attributes 9, 5 and 197) and, for an SSD, `Life used` from its wear attribute. After the drive spins down (`hdparm -y`), the counts stay on screen.
- [ ] 3d.12 The health reads `checking…` for the first moment after start; a drive that predicts its failure (if one is at hand) shows its name in red in the left column and `failing: back it up` in red on the right.
- [ ] 3d.7 `Enter` on a drive selects its first volume in the middle; `Enter` on a volume selects its drive on the left. A volume over 90 % full shows its `USE%` in red and a red bar on the right; a narrow window drops the device and file system columns first.

## 3e. Sensors (`9`)

- [ ] 3e.1 Windows: as a normal user, the left column says only the thermal zones are read; the zones (`TZ00`, `THRM`…) match `Get-CimInstance -Namespace root/wmi MSAcpi_ThermalZoneTemperature` run as administrator (Kelvin × 10); a machine without zones lists nothing and says so. An NVIDIA card adds its own chip with its temperature and fan, matching `nvidia-smi`.
- [ ] 3e.2 Windows: a long CPU load (a build, `Prime95`) raises the zone temperature on its graph within a few seconds, and it falls back after.
- [ ] 3e.3 Linux: the chips and values match `sensors` (lm-sensors): `coretemp` or `k10temp` with `Package id 0` / `Tctl`, `nvme` with the drive's model, the board chip (`nct6798`, `it87`…) with its fans in rpm; the high mark and critical point match `sensors`' `high` and `crit`.
- [ ] 3e.4 Linux: an AMD GPU is listed once (its `amdgpu` chip), an NVIDIA GPU once (from NVML); on a Raspberry Pi or another board with no hwmon temperature, the thermal zones (`cpu-thermal`) show with the note.
- [ ] 3e.5 macOS: the SMC temperatures show under the `SMC` chip with names (Intel: "CPU proximity", "CPU core 1"…; Apple silicon: "CPU 1", "GPU 1"…) and match a tool such as `stats` or `iStat Menus`; on Apple silicon the HID temperatures show under `HID sensors` as "CPU die 1", "SoC 1", "SSD", "Battery" (no "PMU tcal"); the source line names the SMC key or the HID sensor; with no sensors (a virtual machine) the screen says there is nothing to read.
- [ ] 3e.8 macOS: a Mac with fans (an Intel MacBook Pro, a MacBook Pro or Mac Studio with Apple silicon) lists "Fan 1"… under `SMC` in rpm with the least and most matching `stats`; a load raises the rpm within a few seconds. A MacBook Air (no fan) lists none.
- [ ] 3e.7 Windows and Linux: each drive with a temperature is listed once on the sensors screen: on Linux the `nvme` / `drivetemp` chip, and a SATA drive without `drivetemp` under its model (read by smartctl, as root); on Windows each SSD under its model, with the same value as the disk screen.
- [ ] 3e.6 `Enter` on a chip selects its first sensor in the middle; `Enter` on a sensor selects its chip on the left. A temperature within 10 °C of its high mark is yellow and past it red; a stopped fan is dim; a narrow window drops the chip, low, critical and high columns in that order.

## 3f. Sys (`0`)

- [ ] 3f.1 Windows: as a normal user, the system, firmware and board match `msinfo32` (System Manufacturer / Model, BIOS Version/Date, BaseBoard), `Boot` matches its BIOS Mode (UEFI / Legacy); the slots match Task Manager's Memory page (`Slots used: 2 of 4`, speed) and `Get-CimInstance Win32_PhysicalMemory` (bank, maker, part number, configured speed). The crumb reads `… · 32.0 GiB in 2 of 4 slots · SMBIOS 3.x (GetSystemFirmwareTable)`.
- [ ] 3f.2 Windows: on a self-built PC whose system says `System Product Name`, the machine is named by its board (`ASUSTeK COMPUTER INC. ROG STRIX …`); on a laptop by its maker and model. A Hyper-V or VirtualBox guest shows `Virtual machine: yes` under Firmware.
- [ ] 3f.3 Linux: as a normal user, the left column matches `cat /sys/class/dmi/id/{sys_vendor,product_name,bios_version,board_name}`, the middle says the slots need root, and nothing is blank. Run with `sudo`, the slots match `sudo dmidecode -t memory` (locator, size, type, speed, manufacturer, part number, rank, configured voltage) and the error checking matches `dmidecode -t 16`.
- [ ] 3f.4 Linux: on a Raspberry Pi or another device-tree board, the machine is the `/proc/device-tree/model` (`Raspberry Pi 5 Model B Rev 1.0`) and the middle says there is no SMBIOS table.
- [ ] 3f.5 macOS (Apple silicon): the machine and model match About This Mac and `system_profiler SPHardwareDataType`; the middle shows one row in the package (`LPDDR5`, the size, the maker) with the note that the memory is in the chip's package; no serial number appears anywhere.
- [ ] 3f.6 macOS (Intel): the slots match `system_profiler SPMemoryDataType` (`BANK 0/ChannelA-DIMM0`, size, speed, maker); an empty slot is dim and says `empty`.
- [ ] 3f.8 Windows and Linux (with `sudo`): the Processor section's socket, maker, cores, threads, top speed and L1 / L2 / L3 match CPU-Z's or `sudo dmidecode -t 4` and `-t 7` (the L1 and L2 may be the whole chip's or one core's, as the firmware adds them up); a two-socket server says `2 × <socket>` and the cores of both. As a normal user on Linux, the section shows only the model, cores and threads.
- [ ] 3f.9 macOS: the Processor section names the chip (`Apple M2 Pro`) or the CPU (`Quad-Core Intel Core i7`) and its cores match About This Mac; an Intel Mac shows its L3.
- [ ] 3f.10 The Graphics section lists each GPU and NPU the gpu·npu screen lists (with its VRAM where known) and the Drives section each drive the disk screen lists (model and size); each ends with `Details  Press 4` / `Press 8`, and those keys open those screens. Rebinding the screen in `keymap.toml` changes the key shown.
- [ ] 3f.7 The screen opens on the slots; `j`/`k` move through them and the right column follows; `h` to the left column scrolls its facts. A narrow window drops the maker, then the type columns.

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
