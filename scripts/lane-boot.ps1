# Source is uchmk/ito scripts/lanes/; an app's copy is written by its scripts/lanes.sh sync, so edit ito's.
#
# The scheduled task's entry point for a Windows test lane: runs origin/main's
# copy of auto-wintest.ps1, whatever state the worktree is in.
#
# Until v0.101.0 the task ran the worktree's own copy, and moved the worktree
# to origin/main first only when it was clean. A worktree that a cut-off run
# had left dirty kept its old copy, so a fix on main never reached it: on
# 2026-10-10 the status issue of v0.100.2, which would have said the lane was
# stuck, could not be written by the lane that was stuck. This fetches main,
# writes main's scripts/ and Cargo.toml into the state folder with git archive
# (the bytes are main's, whatever the console's code page), checks that they
# parse, and runs auto-wintest.ps1 from there with the same arguments.
# auto-wintest.ps1 then puts the worktree itself back on main (it saves a dirty
# one to a rescue/ branch first).
#
# Kept small so that it hardly ever changes: everything else is in
# auto-wintest.ps1, which this takes fresh from main at every firing. When
# main cannot be read or does not parse, it runs the copy it wrote last time,
# or else the copy beside it, and says so in lane-boot<suffix>.log.
#
#   pwsh -NoProfile -File C:\dev\filer-wintest\scripts\lane-boot.ps1 [-Lane arm] [auto-wintest.ps1's other arguments]
#
# The task's registration is at the top of auto-wintest.ps1. The state folder
# under %LOCALAPPDATA% is scripts\lanes.conf's `state` (kura: filer-wintest).

param(
    [string]$Lane = 'win',
    [Parameter(ValueFromRemainingArguments)] [object[]]$Rest
)

$ErrorActionPreference = 'Stop'
$app = 'filer-wintest'
$conf = Join-Path $PSScriptRoot 'lanes.conf'
if (Test-Path -LiteralPath $conf) {
    $line = Get-Content -LiteralPath $conf | Where-Object { $_ -match '^state=.' } | Select-Object -First 1
    if ($line) { $app = $line.Substring(6) }
}
$script = 'auto-wintest.ps1'

$suffix = if ($Lane -eq 'win') { '' } else { "-$Lane" }
$state = Join-Path $env:LOCALAPPDATA $app
New-Item -ItemType Directory -Force -Path $state | Out-Null
$log = Join-Path $state "lane-boot$suffix.log"
$boot = Join-Path $state "boot$suffix"
$checkout = Split-Path -Parent $PSScriptRoot
function Say([string]$line) { Add-Content -Path $log -Value ('[{0:yyyy-MM-dd HH:mm:ss}] {1}' -f (Get-Date), $line) }

# auto-wintest.ps1's other arguments, as -Name value or a bare -Switch.
$named = @{}
for ($i = 0; $i -lt $Rest.Count; $i++) {
    $token = "$($Rest[$i])"
    if ($token -notmatch '^-(\w+)$') { Say "Left out '$token': not a -Name."; continue }
    $key = $Matches[1]
    $next = if ($i + 1 -lt $Rest.Count) { "$($Rest[$i + 1])" } else { $null }
    if ($null -ne $next -and $next -notmatch '^-\w+$') { $named[$key] = $next; $i++ } else { $named[$key] = $true }
}

$run = Join-Path $PSScriptRoot $script
$commit = $null
try {
    git -C $checkout fetch -q origin main
    if ($LASTEXITCODE -ne 0) { throw 'git fetch failed' }
    $sha = (git -C $checkout rev-parse origin/main | Out-String).Trim()
    $zip = Join-Path $state "boot$suffix.zip"
    git -C $checkout archive -o $zip $sha scripts Cargo.toml
    if ($LASTEXITCODE -ne 0) { throw 'git archive failed' }
    $new = "$boot.new"
    if (Test-Path $new) { Remove-Item -Recurse -Force -LiteralPath $new }
    Expand-Archive -Path $zip -DestinationPath $new
    foreach ($f in Get-ChildItem -Path (Join-Path $new 'scripts\*.ps1')) {
        $errors = $null
        [void][Management.Automation.Language.Parser]::ParseFile($f.FullName, [ref]$null, [ref]$errors)
        if ($errors) { throw "main's $($f.Name) does not parse (line $($errors[0].Extent.StartLineNumber): $($errors[0].Message))" }
    }
    Set-Content -NoNewline -Path (Join-Path $new 'commit') -Value $sha
    if (Test-Path $boot) { Remove-Item -Recurse -Force -LiteralPath $boot }
    Move-Item -LiteralPath $new -Destination $boot
} catch {
    Say "Could not take main's copy: $_"
}
$fresh = Join-Path $boot "scripts\$script"
if (Test-Path $fresh) {
    $run = $fresh
    $commit = (Get-Content -Raw -LiteralPath (Join-Path $boot 'commit') -ErrorAction SilentlyContinue | Out-String).Trim()
} else {
    Say "Running $run instead."
}
if ($commit) { $named['Booted'] = $commit }
$named['From'] = $checkout
& $run -Lane $Lane @named
exit $LASTEXITCODE
