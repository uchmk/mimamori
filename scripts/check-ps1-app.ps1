# yagura's own checks, dot-sourced by scripts/check-ps1.ps1 (ito's, the same
# for every app) after its own: Expect and $bad are its. wintest-queue.ps1
# picks the chunks it should from a small sample.

. (Join-Path $PSScriptRoot 'wintest-queue.ps1')

$checks = @'
## 1. Start and idle

- [x] 1.1 Windows x64: done already
- [ ] 1.2 Windows ARM64: the same
- [ ] 1.3 macOS (Apple Silicon): the same
- [ ] 1.5 Idle CPU stays low
- [ ] 1.11 `Ctrl+Shift+P` opens the palette

## 3b. Services (`6`)

- [ ] 3b.1 Windows: the list matches services.msc
- [ ] 3b.2 A person: the UAC prompt
- [ ] 3b.3 Linux with systemd: the list

## 3d. Disks (`8`)

- [ ] 3d.1 Windows: the volumes
- [ ] 3d.2 Windows ARM64: the same
- [ ] 3d.3 `Enter` opens the volume
'@
# Two retest issues, the oldest first: #7's rows are done (one ticked in the
# issue, one ticked in the checklist), so #9's are given.
$retests = @(
    [pscustomobject]@{ Number = 7; Body = "Changed in v1.0.0.`n`n- [x] 3d.3`n- [ ] 1.1`n" },
    [pscustomobject]@{ Number = 9; Body = "## Rows`r`n`r`n- [ ] 3d.3`r`n- [ ] 3b.1 the list`r`n- [x] 1.5`r`n" }
)

Expect 'the ids an issue names' ((Get-RetestIds $retests[1].Body) -join ',') '3d.3,3b.1'
$c = Select-Chunk -Lane win -Checks $checks -Retests $retests
Expect 'the re-test chunk' "$($c.Kind) $($c.Issue) $($c.Branch) $($c.Rows.Id -join ',')" 'retest 9 test/win-retest-9 3b.1,3d.3'
Expect 'the re-tests as given' (Format-Chunk $c) "## Re-tests of changed behaviour (#9)`n`n- **3b.1** Windows: the list matches services.msc`n- **3d.3** ``Enter`` opens the volume"
# The ARM64 lane leaves the re-tests and takes the rows that name Windows ARM64.
$c = Select-Chunk -Lane arm -Checks $checks -Retests $retests
Expect 'the ARM64 lane' "$($c.Kind) $($c.Branch) $($c.Rows.Id -join ',')" 'rows test/arm-1-2 1.2,3d.2'
# The x64 lane takes the sections in order, not the rows for other machines.
$c = Select-Chunk -Lane win -Checks $checks
Expect 'the first section' "$($c.Branch) $($c.Rows.Id -join ',')" 'test/win-1-5 1.5,1.11'
Expect 'the chunk as given' (Format-Chunk $c) "## 1. Start and idle`n`n- **1.5** Idle CPU stays low`n- **1.11** ``Ctrl+Shift+P`` opens the palette"

$tmp = Join-Path ([IO.Path]::GetTempPath()) "check-ps1-$PID.txt"
try {
    # A re-test tried and left `[ ]` is not given again by the same issue,
    # and is by a new one.
    Add-Attempted $tmp (Select-Chunk -Lane win -Checks $checks -Retests $retests)
    $c = Select-Chunk -Lane win -Checks $checks -Retests $retests -Attempted (Read-Attempted $tmp)
    Expect 'after the attempted re-tests' $c.Kind 'rows'
    $again = @($retests) + @([pscustomobject]@{ Number = 12; Body = '- [ ] 3b.1' })
    $c = Select-Chunk -Lane win -Checks $checks -Retests $again -Attempted (Read-Attempted $tmp)
    Expect 'a row named in a new issue' "$($c.Branch) $($c.Rows.Id -join ',')" 'test/win-retest-12 3b.1'
    Remove-Item -LiteralPath $tmp

    $c = Select-Chunk -Lane win -Checks $checks -Rows 1
    Add-Attempted $tmp $c
    $c = Select-Chunk -Lane win -Checks $checks -Rows 1 -Attempted (Read-Attempted $tmp)
    Expect 'after the attempted rows' "$($c.Branch) $($c.Rows.Id -join ',')" 'test/win-1-11 1.11'
    # A row whose words changed is offered again.
    $c = Select-Chunk -Lane win -Checks ($checks -replace 'stays low', 'stays low, reworded') -Rows 1 -Attempted (Read-Attempted $tmp)
    Expect 'a reworded row' "$($c.Branch) $($c.Rows.Id -join ',')" 'test/win-1-5 1.5'
} finally {
    Remove-Item -LiteralPath $tmp -ErrorAction SilentlyContinue
}
$c = Select-Chunk -Lane win -Checks '- [x] 1.1 all done'
Expect 'nothing left' ($null -eq $c) $true
# Section ids with a letter sort after the number's own section.
$c = Select-Chunk -Lane win -Checks "## 3. Processes`n`n- [x] 3.1 done`n`n## 3a. GPU`n`n- [ ] 3a.1 a gpu row`n"
Expect 'a lettered section' "$($c.Title)|$($c.Rows.Id -join ',')" '3a. GPU|3a.1'

if (-not $bad) { Write-Output 'The wintest queue picks the chunks it should.' }
