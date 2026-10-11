# Windows x64 test: yagura v0.50.2 (9134f86)

Partial run on Ryzen 9 9950X3D / RX 9070 XT / Windows 11 Pro 26200. Every run used an isolated
config and state folder (`YAGURA_CONFIG_HOME`, `YAGURA_STATE_HOME`, `UCHMK_CONFIG_DIR`).
Evidence: `C:\dev\filer-evidence\2026-10-12\yagura\` (screenshots, state dumps, CSV, snapshot JSON).

## Ticked [x]
- 1.1 window opens and renders, no console window (k1/)
- 2.1 CPU screen shows `L3 #0` and `L3 #1` groups, two thread bars per tile (c2/cpu.png, cpu.txt)
- 3i.1 `--snapshot` prints valid JSON (now.json)
- 4.4 column widths persist: 217/500/283 -> 307/442/251 -> same after restart, saved in state.toml (w0/, w1/)
- 5.1 config values take effect (c2/cpu2.png)
- 5.3 broken config.toml: starts with defaults and shows what is wrong (b5/bad.png, bad.txt)

## Checked in part, not ticked
- 3i.2 `--log-metrics` CSV is correct (s1/, u2.csv); Ctrl+C stop not checked
- 3g.1 / 3n.1 agent: listens on 127.0.0.1 only, no window, HTTP 200 `text/plain; version=0.0.4` (n1/).
  Not checked: Ctrl+C stop, 404 at `/`, `--help` / `--version` in a console, numbers vs. screens.
  Port 7313 is held by tailscaled, so `[agent] port = 7399` was used.

## Not pressed (pressable here)
5.2, 3b.14, 3l.1, 2.6, 1.9, Windows rows of 3d / 3e / 3f / 3h.

## Cannot be pressed on this machine
GPU/NPU (NVIDIA, NPU), SMART, HWiNFO, administrator, Docker, other OSes and other machines:
1.2-1.4, 1.10, 2.2, 2.3, 3a.2, 3a.4, 3b.8, 3d.17, 3e.9-3e.11, 3g.3-3g.7, 3m.x, 3k.3, 3k.4 and similar.

## Note
An early test script ran `Get-Process yagura | Stop-Process -Force`; it may have stopped the
owner's `yagura-agent` (Run key). The script now kills only its own PID.
