# Which rows the next unattended run takes. Dot-sourced by auto-wintest.ps1;
# plain functions over the checklist's text, so scripts/check-ps1.ps1 tests
# them on Linux too.
#
# A run gets one chunk: up to -Rows rows of one section of TESTING.md, or the
# re-tests of one `retest` issue. It is told those rows and nothing else,
# which is most of what keeps a run small. A row a run was given and left
# `[ ]` (a look, a failure, a row it could not reach) is written to the
# attempted list with a hash of its words, so the next run moves on; changing
# the row's words in TESTING.md, or naming it in a new retest issue, offers it
# again.
#
# yagura's rows are `- [ ] 3d.8 text` (the id is the section's number, its
# letter if it has one, a dot and the row's number). The marks a run set are
# made again on main by lane-marks.ps1 (ito's).

# Rows a lane never takes by id, for a row that cannot be done unattended
# on that machine (none yet).
$WintestSkip = @{
    win = @()
    arm = @()
}

# A row an unattended Windows run can do. The x64 lane does not take a row for
# another OS (`Linux …`, `macOS …`), for a person (`A person: …`: a UAC
# prompt, a sign-out, a plug), or for the ARM64 machine (`Windows ARM64 …`).
# The ARM64 lane takes only the rows that begin `Windows ARM64`.
function Test-Unattended([string]$Lane, $Row) {
    if ($Row.Id -in $WintestSkip[$Lane]) { return $false }
    if ($Lane -eq 'arm') { return $Row.Text -match '^Windows ARM64\b' }
    $Row.Text -notmatch '^(Linux|macOS|A person:|Windows ARM64\b)'
}

# Every `- [ ] 3d.8 text` of TESTING.md, in file order, with its section's id
# (`3d`) and title.
function Get-OpenRows([string]$Checks) {
    $title = @{}
    foreach ($line in $Checks -split "`r?`n") {
        if ($line -match '^## (\d+[a-z]?)\. (.*)$') { $title[$Matches[1]] = $Matches[2]; continue }
        if ($line -match '^- \[ \] ((\d+[a-z]?)\.\d+) (.*)$') {
            $sec = $Matches[2]
            [pscustomobject]@{ Id = $Matches[1]; Section = $sec; Title = $title[$sec]; Text = $Matches[3] }
        }
    }
}

# The row ids a retest issue's body still names `[ ]`: `- [ ] 3d.8` or
# `- [ ] **3d.8** …` or `- [ ] 3d.8 …`.
function Get-RetestIds([string]$Body) {
    @([regex]::Matches($Body, '(?m)^\s*- \[ \] \**(\d+[a-z]?\.\d+)\b') | ForEach-Object { $_.Groups[1].Value } | Select-Object -Unique)
}

function Get-TextHash([string]$Text) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($Text)
    -join ([Security.Cryptography.SHA256]::HashData($bytes)[0..7] | ForEach-Object { $_.ToString('x2') })
}

# The attempted list: `<id>\t<hash>` lines -> @{ id = hash }.
function Read-Attempted([string]$Path) {
    $seen = @{}
    if (Test-Path -LiteralPath $Path) {
        foreach ($line in Get-Content -LiteralPath $Path) {
            $id, $hash = $line -split "`t", 2
            if ($id) { $seen[$id] = $hash }
        }
    }
    $seen
}

function Add-Attempted([string]$Path, $Chunk) {
    $seen = Read-Attempted $Path
    foreach ($r in $Chunk.Rows) { $seen[$r.Id] = $r.Hash }
    $lines = $seen.Keys | Sort-Object | ForEach-Object { "$_`t$($seen[$_])" }
    Set-Content -LiteralPath $Path -Value $lines
}

# The next chunk for a lane, or $null when there is nothing left to give.
# Each row comes back with the hash it is remembered by.
function Select-Chunk {
    param(
        [ValidateSet('win', 'arm')] [string]$Lane,
        [string]$Checks,
        # the open retest issues, oldest first: @{ Number; Body }
        $Retests = @(),
        [hashtable]$Attempted = @{},
        [int]$Rows = 15
    )
    $open = @(Get-OpenRows $Checks)

    # 1. Re-tests: the rows an open retest issue names, still `[ ]`, the
    # oldest issue first, x64 only (the ARM64 lane keeps to its own rows). The
    # issue's number is in the hash, so a row named again in a new issue is
    # offered again.
    if ($Lane -eq 'win') {
        foreach ($issue in @($Retests)) {
            $ids = Get-RetestIds $issue.Body
            $picked = @($open | Where-Object { $_.Id -in $ids -and (Test-Unattended $Lane $_) } | ForEach-Object {
                    $r = $_.PSObject.Copy()
                    $r | Add-Member Hash (Get-TextHash ($r.Text + "`n#" + $issue.Number))
                    $r
                } | Where-Object { $Attempted[$_.Id] -ne $_.Hash } | Select-Object -First $Rows)
            if ($picked) {
                return [pscustomobject]@{ Kind = 'retest'; Title = "Re-tests of changed behaviour (#$($issue.Number))"; Rows = $picked; Issue = [int]$issue.Number; Branch = "test/$Lane-retest-$($issue.Number)" }
            }
        }
    }

    # 2. The ARM64 lane: its own rows, from whichever sections have them.
    if ($Lane -eq 'arm') {
        $picked = @($open | Where-Object { Test-Unattended $Lane $_ } | ForEach-Object { $_ | Add-Member Hash (Get-TextHash $_.Text) -PassThru -Force } |
            Where-Object { $Attempted[$_.Id] -ne $_.Hash } | Select-Object -First $Rows)
        if ($picked) {
            return [pscustomobject]@{ Kind = 'rows'; Title = 'Windows ARM64 rows'; Rows = $picked; Branch = "test/arm-$($picked[0].Id -replace '\.', '-')" }
        }
        return $null
    }

    # 3. The sections, in number order (`3` before `3a`, `3b` ... `4`).
    $order = $open | ForEach-Object Section | Select-Object -Unique | Sort-Object { [int]($_ -replace '\D', '') }, { $_ -replace '\d', '' }
    foreach ($sec in $order) {
        $picked = @($open | Where-Object { $_.Section -eq $sec -and (Test-Unattended $Lane $_) } | ForEach-Object { $_ | Add-Member Hash (Get-TextHash $_.Text) -PassThru -Force } |
            Where-Object { $Attempted[$_.Id] -ne $_.Hash } | Select-Object -First $Rows)
        if ($picked) {
            return [pscustomobject]@{ Kind = 'rows'; Title = "$sec. $($picked[0].Title)"; Rows = $picked; Branch = "test/$Lane-$($picked[0].Id -replace '\.', '-')" }
        }
    }
    $null
}

# The chunk as the prompt gives it: one line per row.
function Format-Chunk($Chunk) {
    $lines = foreach ($r in $Chunk.Rows) { "- **$($r.Id)** $($r.Text)" }
    "## $($Chunk.Title)`n`n" + ($lines -join "`n")
}
