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

> **v0.1** has four screens: overview, cpu, mem and procs. The other six tabs
> are there with their final numbers, so the keys will not move, and say
> "Not yet". See [TODO.md](TODO.md).

## Screens

| Key | Screen   | What it shows |
| --- | -------- | ------------- |
| `1` | overview | CPU / memory / swap gauges and the system; a big graph of the selected gauge over the compact core tiles; the top processes by CPU and by memory |
| `2` | cpu      | Groups and counts; the core tiles, the total over the last minute and a thread heatmap; the selected core's threads and the top CPU users |
| `3` | mem      | Used / available / swap; the usage bar and graph; the processes using the most memory |
| `4` | gpu·npu  | Not yet |
| `5` | procs    | Scope and sort; the process list (filter with `/`, sort with `s`, end one with `x`); the selected process's details and graphs |
| `6`–`0` | svc, net, disk, sensors, sys | Not yet |

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
| `Enter` | Open the selection (a gauge's screen, a process in procs) |
| `/` | Filter processes (`Enter` keeps it, `Esc` drops it) |
| `s` | Next sort column |
| `x` | End the selected process, after asking (`y` / `Enter` ends it, `n` / `Esc` keeps it). Windows uses `TerminateProcess`; macOS and Linux send SIGTERM, then SIGKILL if it is still there after 3 s. Works on procs and on the top lists of overview, cpu and mem |
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
| `kill` | Ask, then end the selected process |
| `help`, `escape`, `quit` | As the keys above |
| `confirm`, `backspace` | The filter line's keys (`[input]`); `confirm` and `escape` also answer the question before a process is ended (`[confirm]`) |

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

`mimamori --keys SCRIPT` presses keys once the window is up (as filer's
`--keys`), and `scripts/xrun.sh` runs it on a virtual X display and keeps the
screenshots: that is how changes are checked without a screen.

## License

MIT or Apache-2.0, at your option.
