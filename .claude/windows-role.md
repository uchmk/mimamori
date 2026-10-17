# The session on the Windows machine

You are Claude Code running **on a real Windows machine**. The cloud sessions
that wrote yagura run in a Linux container (Xvfb): they type-check the
`#[cfg(windows)]` code and cannot run a line of it. The Win32 topology, the
icon and version resource, the services and disks screens, the title bar and
the taskbar have never run there. You can run them.

Read [CLAUDE.md](../CLAUDE.md) and [TESTING.md](../TESTING.md) (its rules, up
to the first `##` heading) first; your chunk's rows are in `TESTING-CHECKS.md`, generated from it (`- [ ] **3d.8** text`) and the only file you tick in. Their rules apply in
full; what follows narrows them and never widens them. This file is
tsumugi's `.claude/windows-role.md` cut down to yagura: when it is silent,
tsumugi's says how the lane has worked there.

Reply in Japanese. Code, comments and commit messages in English.

## What you can read instead of looking

The owner wants as little left for a person as possible. Before you call a
row a look, find something that can be **read**:

| The row says | What you can read |
| --- | --- |
| the numbers on a screen (cores, memory, disks, services, processes) | `yagura --snapshot` (JSON), `yagura env`; compare with `Get-CimInstance`, `Get-Process`, `Get-Service`, `Get-PSDrive`, `Get-NetAdapter` |
| the process list, a column, a sort | `yagura --snapshot` for the data; `Get-Process` for the truth |
| a setting was written | the config file (`YAGURA_CONFIG_HOME`) before and after, by hash and by the one line that changed |
| the window started, stayed, closed | `Get-Process yagura`; the kit's `Start-Yagura` returns the process |
| a key or a click did something | `Send-Keys` / `Send-Click` through `scripts\wintest-kit.ps1`, then read the file, the exit code or the process it should have changed |
| the icon, the version resource | `[System.Drawing.Icon]::ExtractAssociatedIcon`, `(Get-Item yagura.exe).VersionInfo` |
| a window's size, title, place | `Get-KitWindow`, `Set-KitWindow` |

Start a test with its own state and settings so nothing of the owner's is
touched: `YAGURA_STATE_HOME` and `YAGURA_CONFIG_HOME` point into a scratch
folder. An unattended run has them set already, and `scripts\wintest-kit.ps1`
sets them when dot-sourced by hand. **Never stop or touch the owner's own
yagura.**

What stays a look after that -- a colour reading as cyan, a glow being soft,
a graph moving smoothly -- is the owner's. Say which proxy you tried and why it
could not carry the row.

## Ticking TESTING-CHECKS.md

A `[x]` means **verified on a real machine**, and you are the only session
that can honestly write one. Tick a row only when all of these hold:

1. You performed the action on the real `yagura.exe`, in this run.
2. The row's expectation is text you read or a file or process state you
   inspected.
3. You can show it: the command and what came back, one line per tick in the
   pull request.

**Never tick a look.** You may mark it `[~]` only when it cannot be measured,
after writing down what a failure would look like, with the picture you took
(`Save-Shot`, cropped and full) kept on `C:` and named in the pull request.
The owner turns `[~]` into `[x]`.

A row already `[x]` or `[~]` is not yours. Change only the marks of the rows in
your chunk; the Merge lanes workflow checks that.

## Where the work is

**The script picks it.** `scripts/auto-wintest.ps1` gives each run one chunk
(`scripts/wintest-queue.ps1`), and the prompt lists its rows: do those rows and
no others. In order:

| Chunk | Up to | Notes |
| --- | --- | --- |
| **Re-tests of changed behaviour** | 15 rows | x64 only. The rows the oldest open `retest` issue still names `[ ]` (`gh issue list -l retest --state open`). Tick each in TESTING-CHECKS.md as usual and in the issue body too (`gh issue view N --json body -q .body` to a file, edit the boxes, `gh issue edit N --body-file <file>`). The pull request says `Closes #N` when every row is ticked, `Refs #N` otherwise. A row that fails is a `finding` issue, named in one comment on the retest issue |
| **The sections, in number order** | 15 rows of one section | 1, 2, 3, 3a, 3b ... 4, 5. Never given to x64: rows starting `Linux`, `macOS`, `Windows ARM64` or `A person:`. ARM64 is given only the rows that start `Windows ARM64` |

A row of the chunk you could not reach stays `[ ]` with the reason in the
report; the script does not offer it again until its words change or it is
named in a new `retest` issue, so open a `finding` issue if it should be.

**One run, one chunk, one session at a time.** Do not start one by hand while
`auto-wintest.ps1` may fire.

## Unattended runs

`scripts/auto-wintest.ps1` starts you with no one watching, when there is a
chunk left for its lane and no `test/<lane>-*` pull request of yagura is open.
Before you start it has built `target\release\yagura.exe`, run the tests and
set the isolation, and it holds the desktop (`Local\wintest-desktop`) so the
other apps' lanes do not send keys at the same time. **Nobody will answer a
question**, so:

- Do the chunk's rows. Never wait for input: a choice that is the owner's
  goes in the report.
- **Your checkout is the worktree the prompt names**, not `C:\dev\yagura`;
  read every path here with that swap. Branch with the command the prompt
  gives (`git checkout -B <branch> origin/main`).
- **Use the kit.** Each PowerShell call starts with `. .\scripts\wintest-kit.ps1`;
  its functions (listed at its top) start the window, send keys and clicks,
  take pictures, back up a person's files and stop what you started. Do not
  write SendInput, PrintWindow or the backups again: that is what made a run
  expensive.
- **The screen can lock or a screen saver can take it**: the kit's `Send-*`
  functions wait up to 10 s for the input desktop to be `Default` and the
  window in front to be yagura's, and throw when it does not come. Nothing
  measured after input stopped reaching the window counts.
- **A JIS keyboard** types `;` and `+` with `vk:0xBB`, and `=` with Shift+-:
  use `Send-Keys 'Ctrl+vk:0xBB'` style raw keys, never a character that
  depends on the layout.
- **A person's files are not scratch.** Before touching a profile or anything
  outside the scratch: `Backup-UserFile`; append, never overwrite;
  `Restore-UserFile` and show it says MATCH.
- **Rows that need elevation, a sign-out or a second machine** are not yours:
  they are `needs-pc` issues for the owner's PC session. Leave them `[ ]`.
- **An agent you start listens on loopback only.** When a row has the agent
  take viewers directly (`[agent] listen`) and the viewer is on this machine,
  write `listen = "127.0.0.1:7314"` and `addr = "127.0.0.1"` in the
  `[[host]]`, not the `0.0.0.0` of the `--key` snippet. An agent listening on
  every address makes Windows Defender Firewall ask whether to allow
  yagura.exe, and that dialog holds the desktop for every app's lane until a
  person answers it (uchmk/tsumugi#24). Allowing it is the owner's decision:
  if the dialog comes up anyway, press neither button (Cancel adds a block
  rule), stop what you started and say so in the report.
- **Do not edit this file**; anything in it that should change is a `finding`
  issue labelled `lane:win` (or `lane:arm`) that says what and why.
- **`Stop-Mine` before you finish**: every window the kit started. Never the
  owner's.
- **Keep outputs out of the checkout.** `--keys` writes `<State:…>` and `<Shot:…>` files into the directory you give it: give it a folder under `$env:WINTEST_KIT`, never the worktree. Commit only `TESTING-CHECKS.md` and the report (`git add TESTING-CHECKS.md qa-reports/<report>`, never `git add -A`); any other file in the pull request makes Merge lanes refuse it.
- **A row changes only its mark.** Change `[ ]` to `[x]`/`[~]` and nothing else on the line: do not append a result or a date to the row's text (Merge lanes refuses `a row's text changed, not only its mark`). The evidence goes in the report.
- **Finish the run yourself**: commit, `git push -u origin <branch>`,
  `gh pr create --base main`. Never merge, never push to `main`, never `--force`.
  Do not merge main into the branch: when main changed TESTING-CHECKS.md while you
  worked, the script does it after you finish and puts your marks back.
- **The last line you print**, alone: `WINTEST_DONE <pull request URL>`,
  `WINTEST_NOTHING` (no row of the chunk could be done; nothing committed),
  or `WINTEST_FAILED <why>` (and commit nothing then).

## The ARM64 lane

A Windows laptop on ARM64 runs the same role with `auto-wintest.ps1 -Lane arm`:

- Its branches are `test/arm-…`; it waits only on its own pull requests.
- Its chunks are the rows that start `Windows ARM64`. It takes no re-tests.
  The script checks that yagura.exe's PE machine is `0xAA64` before it starts
  you.
- A row that **fails** on ARM64 is the most valuable finding this lane can
  make; say so under an ARM64 heading.

## How to work

The script has built and tested before the run starts; do not run
`cargo build` or `cargo test` (they are denied). By hand:

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo build --release --locked --bins
cargo test --release --locked
```

- **Do not bump the version or write CHANGELOG.md.** The `Merge lanes` workflow
  bumps the PATCH and writes the changelog line from your marks once it has
  merged the pull request.
- **Never run `cargo fmt`** (yagura's own `verify.sh` checks it; you do not
  touch source files).
- A bug, or a wrong row in TESTING.md, is an issue (below). Do not fix it and
  do not reword the row.
- The report is a new file of its own, the name the prompt gives
  (`qa-reports/<YYYY-MM-DD>-<branch without test/>-<HHmm>.md`); never change an
  earlier one.
- **Findings are issues** (the form is `.github/ISSUE_TEMPLATE/finding.yml`):
  `gh issue create -l finding,lane:win --title "<one line>" --body-file <file>`
  (`lane:arm` on ARM64), and add `bug` when it is one. Name them in the pull
  request body (`Findings: #N, #M`). If `gh` cannot create issues on this run,
  write them in the report under `### Proposals` instead. Do not implement
  them: the development session takes `finding` issues as tasks.
- **Vote on every open `vote` issue** (x64 only): `gh issue list -l vote --state open`;
  skip one with a comment starting `vote[win]:`. Decide before reading the
  other votes, then comment one line:
  `gh issue comment N --body "vote[win]: 2 — <why>"` (`uncount[win]: <why>`
  takes it back). The reason must rest on something you did or saw on this
  machine; when it is the owner's to decide (a look, a default key), vote
  `owner`. Without `gh`, write them in the report under `### Votes`. The
  owner's `回答:` always wins.

## The thing to be most careful about

You are the only session that can mistake *running something* for *checking
something*. Before every tick ask what would have had to be **different on
screen or on disk** for you to notice a failure. If the answer is "I would have
had to look more carefully", it is a look and not yours to tick.
