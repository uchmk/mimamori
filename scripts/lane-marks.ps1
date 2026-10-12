# Source is uchmk/ito scripts/lanes/; an app's copy is written by its scripts/lanes.sh sync, so edit ito's.
#
# A Windows run's checklist marks, made again on a main that moved under it.
# Dot-sourced by auto-wintest.ps1 (Sync-LaneBranch) and checked by
# check-ps1.ps1.

# The marks a run set in one checklist: each `- [x] …` or `- [~] …` line of
# the run's copy whose line at the run's base was `- [ ] …` with the same
# words, as @{ Mark; Rest }.
function Get-LaneMarks([string]$Base, [string]$Branch) {
    $open = [Collections.Generic.HashSet[string]]::new([string[]]@($Base -split "`r?`n"))
    foreach ($line in $Branch -split "`r?`n") {
        if ($line -match '^- \[([x~])\] (.*)$' -and $open.Contains("- [ ] $($Matches[2])")) {
            [pscustomobject]@{ Mark = $Matches[1]; Rest = $Matches[2] }
        }
    }
}

# main's checklist with a run's marks made again (merge-lanes.py's
# reapply_marks does the same on the workflow's side). A run takes an hour,
# and main's copy moves under it: a row reworded and its mark taken back, a
# row added, another lane's ticks. Git calls a mark beside such a line a
# conflict, so instead each mark goes on main's line with the same words while
# it is still `[ ]`. A row main reworded or took out keeps no mark: what the
# run checked is not what the row says now. Returns @{ Text; Dropped } with
# the dropped rows' ids (or a key's line).
function Set-LaneMarks([string]$Main, $Marks) {
    $lines = $Main -split "`n"
    $where = @{}
    for ($i = 0; $i -lt $lines.Count; $i++) { $where[$lines[$i].TrimEnd("`r")] = $i }
    $dropped = foreach ($m in @($Marks)) {
        $i = $where["- [ ] $($m.Rest)"]
        if ($null -ne $i) {
            $lines[$i] = "- [$($m.Mark)] $($m.Rest)" + $(if ($lines[$i].EndsWith("`r")) { "`r" })
        } elseif ($null -eq $where["- [$($m.Mark)] $($m.Rest)"]) {
            if ($m.Rest -match '^\*\*([^*]+)\*\*') { $Matches[1] } else { $m.Rest }
        }
    }
    [pscustomobject]@{ Text = $lines -join "`n"; Dropped = @($dropped) }
}
