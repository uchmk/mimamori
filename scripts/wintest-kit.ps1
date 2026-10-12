# The tools a Windows test run uses, written once so a run does not write
# them again. Each PowerShell call of a run is a process of its own, so
# dot-source it at the top of each, and the kit keeps what it must remember
# (the windows it started, the backups) in files under $KitDir:
#
#   . .\scripts\wintest-kit.ps1
#
#   Start-Yagura [-ArgumentList ...]    the window, isolated (below); returns its process
#   Test-InputReady                     input reaches our window: the Default desktop, ours in front
#   Set-Foreground                      brings our window to the front
#   Send-Keys 'Ctrl+Shift+P'            one chord through SendInput ('Alt+Shift++' is the + key)
#   Send-Keys -Hold 'Ctrl+C'            the same with 120 ms between steps
#   Send-Keys 'Ctrl+vk:0xBB'            a raw virtual key, whatever the keyboard layout types with it
#   Send-Text 'text'                    characters through SendInput (KEYEVENTF_UNICODE)
#   Send-KeysAtOnce 'Ctrl+P' 'Esc'      the chords in one SendInput
#   Send-Click -X 120 -Y 40 [-Right|-Middle]  a click at egui points from the window's client corner
#   Send-DoubleClick -X 120 -Y 40       two clicks in one SendInput
#   Send-Drag -X 10 -Y 40 -ToX 200 -ToY 40     the left button down, moved in steps, up
#   Send-Wheel -X 120 -Y 200 -Notches -3       the wheel there; negative is towards you (down)
#   Get-KitCursor [-X 120 -Y 40]        the cursor's shape (Arrow, IBeam, SizeWE, SizeNS, Hand, ...), there
#   Set-KitWindow -Width 900 -Height 600 [-X 0 -Y 0]  the window's outer size (and place) in pixels
#   Save-Shot -Name before              PrintWindow of our window to <kit dir>\shotsefore.png
#   Backup-UserFile / Restore-UserFile  a person's file: copy and hash first, put back and compare
#   Stop-Mine                           every window this kit started; never the owner's
#
# Isolation: auto-wintest.ps1 sets YAGURA_STATE_HOME and YAGURA_CONFIG_HOME
# for the whole run, and WINTEST_KIT (the kit's folder) and WINTEST_EXE, so
# the owner's settings and state are never touched. By hand, the kit sets
# them itself (a folder under TEMP).

$ErrorActionPreference = 'Stop'

if (-not ('YaguraKit.Native' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

namespace YaguraKit {
public static class Native {
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk, wScan; public uint dwFlags, time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    // 40 bytes on a 64-bit process (x64 and ARM64): the union is 8-aligned.
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public InputUnion u; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct CURSORINFO { public int cbSize, flags; public IntPtr hCursor; public POINT ptScreenPos; }

    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern short VkKeyScan(char c);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool GetCursorInfo(ref CURSORINFO info);
    [DllImport("user32.dll")] static extern IntPtr LoadCursor(IntPtr instance, IntPtr name);
    [DllImport("user32.dll")] static extern uint GetDoubleClickTime();
    [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll", SetLastError = true)] public static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
    [DllImport("user32.dll")] public static extern bool CloseDesktop(IntPtr d);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern bool GetUserObjectInformation(IntPtr obj, int index, StringBuilder info, int length, out int needed);

    public static int InputSize() { return Marshal.SizeOf(typeof(INPUT)); }

    static uint Send(INPUT[] inputs) { return SendInput((uint)inputs.Length, inputs, InputSize()); }

    static INPUT Key(ushort vk, ushort scan, uint flags) {
        INPUT i = new INPUT();
        i.type = 1;   // INPUT_KEYBOARD
        i.u.ki.wVk = vk; i.u.ki.wScan = scan; i.u.ki.dwFlags = flags;
        return i;
    }

    static bool Extended(ushort vk) {
        // Arrows, Insert/Delete/Home/End/PageUp/PageDown, the right-hand Ctrl/Alt, Win.
        return (vk >= 0x21 && vk <= 0x28) || vk == 0x2D || vk == 0x2E || vk == 0x5B || vk == 0x5C || vk == 0xA3 || vk == 0xA5;
    }

    // The modifiers down in order, the key down and up, the modifiers up in
    // reverse: one SendInput, so nothing else can come between them.
    public static uint Chord(ushort[] mods, ushort vk) {
        INPUT[] all = new INPUT[mods.Length * 2 + 2];
        int n = 0;
        foreach (ushort m in mods) all[n++] = Key(m, 0, Extended(m) ? 1u : 0u);
        uint ext = Extended(vk) ? 1u : 0u;
        all[n++] = Key(vk, 0, ext);
        all[n++] = Key(vk, 0, ext | 2u);   // KEYEVENTF_KEYUP
        for (int k = mods.Length - 1; k >= 0; k--) all[n++] = Key(mods[k], 0, (Extended(mods[k]) ? 1u : 0u) | 2u);
        return Send(all);
    }

    // Several chords in one SendInput, so no other window can take the
    // foreground between them: `spec` is, for each,
    // the number of modifiers, the modifiers, and the key.
    public static uint ChordsAtOnce(ushort[] spec) {
        System.Collections.Generic.List<INPUT> all = new System.Collections.Generic.List<INPUT>();
        int at = 0;
        while (at < spec.Length) {
            int n = spec[at++];
            ushort[] mods = new ushort[n];
            Array.Copy(spec, at, mods, 0, n);
            at += n;
            ushort vk = spec[at++];
            foreach (ushort m in mods) all.Add(Key(m, 0, Extended(m) ? 1u : 0u));
            uint ext = Extended(vk) ? 1u : 0u;
            all.Add(Key(vk, 0, ext));
            all.Add(Key(vk, 0, ext | 2u));
            for (int k = n - 1; k >= 0; k--) all.Add(Key(mods[k], 0, (Extended(mods[k]) ? 1u : 0u) | 2u));
        }
        return Send(all.ToArray());
    }

    public static uint Text(string s) {
        INPUT[] all = new INPUT[s.Length * 2];
        for (int k = 0; k < s.Length; k++) {
            all[2 * k] = Key(0, s[k], 4u);              // KEYEVENTF_UNICODE
            all[2 * k + 1] = Key(0, s[k], 4u | 2u);
        }
        return Send(all);
    }

    // As Chord, but each step a SendInput of its own with `ms` between: an
    // app that reads the modifiers' state when the key comes (egui's copy)
    // can miss them when the chord is one burst.
    public static uint ChordHeld(ushort[] mods, ushort vk, int ms) {
        uint sent = 0;
        foreach (ushort m in mods) { sent += Send(new INPUT[] { Key(m, 0, Extended(m) ? 1u : 0u) }); System.Threading.Thread.Sleep(ms); }
        uint ext = Extended(vk) ? 1u : 0u;
        sent += Send(new INPUT[] { Key(vk, 0, ext) });
        System.Threading.Thread.Sleep(ms);
        sent += Send(new INPUT[] { Key(vk, 0, ext | 2u) });
        System.Threading.Thread.Sleep(ms);
        for (int k = mods.Length - 1; k >= 0; k--) sent += Send(new INPUT[] { Key(mods[k], 0, (Extended(mods[k]) ? 1u : 0u) | 2u) });
        return sent;
    }

    static INPUT Mouse(uint flags, uint data) {
        INPUT i = new INPUT();
        i.type = 0;   // INPUT_MOUSE
        i.u.mi.dwFlags = flags; i.u.mi.mouseData = data;
        return i;
    }

    // Down and up where the cursor is (SetCursorPos first): 0 left, 1 right, 2 middle.
    public static uint Click(int button) {
        uint down = button == 1 ? 0x0008u : button == 2 ? 0x0020u : 0x0002u;
        return Send(new INPUT[] { Mouse(down, 0), Mouse(down << 1, 0) });
    }

    // Two left clicks in one SendInput: well inside the double-click time,
    // which separate calls from PowerShell were not always.
    public static uint DoubleClick() {
        return Send(new INPUT[] { Mouse(0x0002u, 0), Mouse(0x0004u, 0), Mouse(0x0002u, 0), Mouse(0x0004u, 0) });
    }

    public static uint DoubleClickTime() { return GetDoubleClickTime(); }

    // The cursor shown now, by the system cursor it is (winit and egui load
    // the shared ones), or its handle when it is none of them.
    public static string CursorName() {
        CURSORINFO info = new CURSORINFO();
        info.cbSize = Marshal.SizeOf(typeof(CURSORINFO));
        if (!GetCursorInfo(ref info)) return "(unknown)";
        if ((info.flags & 1) == 0) return "Hidden";   // CURSOR_SHOWING
        string[] names = { "Arrow", "IBeam", "Wait", "Cross", "SizeNWSE", "SizeNESW", "SizeWE", "SizeNS", "SizeAll", "No", "Hand", "AppStarting", "Help" };
        int[] ids = { 32512, 32513, 32514, 32515, 32642, 32643, 32644, 32645, 32646, 32648, 32649, 32650, 32651 };
        for (int k = 0; k < ids.Length; k++) {
            if (LoadCursor(IntPtr.Zero, new IntPtr(ids[k])) == info.hCursor) return names[k];
        }
        return "0x" + info.hCursor.ToString("X");
    }

    // The left button alone, down or up.
    public static uint Button(bool down) { return Send(new INPUT[] { Mouse(down ? 0x0002u : 0x0004u, 0) }); }

    // One wheel turn of `notches` (120 each; negative towards the user).
    public static uint Wheel(int notches) { return Send(new INPUT[] { Mouse(0x0800u, unchecked((uint)(notches * 120))) }); }

    public static string InputDesktop() {
        IntPtr h = OpenInputDesktop(0, false, 0x0001);   // DESKTOP_READOBJECTS
        if (h == IntPtr.Zero) return "(none)";
        try {
            StringBuilder name = new StringBuilder(256);
            int needed;
            GetUserObjectInformation(h, 2, name, 512, out needed);   // UOI_NAME
            return name.ToString();
        } finally { CloseDesktop(h); }
    }
}
}
'@
    # Physical pixels everywhere (cursor, window rectangles, screenshots), as
    # per-monitor aware v2; fails harmlessly when already set.
    [void][YaguraKit.Native]::SetProcessDpiAwarenessContext([IntPtr]::new(-4))
}

# Sets the isolation when the run's script has not (by hand). Idempotent.
function Use-Isolation {
    if (-not $env:WINTEST_KIT) { $env:WINTEST_KIT = Join-Path $env:TEMP 'yagura-kit' }
    $script:KitDir = $env:WINTEST_KIT
    New-Item -ItemType Directory -Force -Path $script:KitDir | Out-Null
    if (-not $env:YAGURA_STATE_HOME -or $env:YAGURA_STATE_HOME -notlike '*yagura-wintest*') {
        $env:YAGURA_STATE_HOME = Join-Path $script:KitDir 'state'
        $env:YAGURA_CONFIG_HOME = Join-Path $script:KitDir 'config'
    }
    if (-not $env:WINTEST_EXE) { $env:WINTEST_EXE = (Resolve-Path 'targetelease\yagura.exe').Path }
}
Use-Isolation

# The window. Waits for it to show and returns the process. The newest window
# is the one the other functions act on, in this call and the later ones.
function Start-Yagura([string[]]$ArgumentList = @(), [int]$TimeoutSec = 20, [string]$Exe = $env:WINTEST_EXE) {
    $n = @(Get-ChildItem -LiteralPath $script:KitDir -Filter 'stderr-*.txt' -ErrorAction SilentlyContinue).Count + 1
    $errLog = Join-Path $script:KitDir "stderr-$n.txt"
    $p = if ($ArgumentList) {
        Start-Process -FilePath $Exe -ArgumentList $ArgumentList -WorkingDirectory (Get-Location) -RedirectStandardError $errLog -PassThru
    } else {
        Start-Process -FilePath $Exe -WorkingDirectory (Get-Location) -RedirectStandardError $errLog -PassThru
    }
    # With its start time: a process id can be reused.
    Add-Content -LiteralPath (Join-Path $script:KitDir 'windows.txt') -Value "$($p.Id)`t$($p.StartTime.Ticks)"
    @{ Pid = $p.Id; ErrLog = $errLog } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $script:KitDir 'window.json')
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $p.Refresh()
        if ($p.HasExited) { throw "yagura exited ($($p.ExitCode)) before showing a window; $errLog says: $(Get-Content -Raw -LiteralPath $errLog -ErrorAction SilentlyContinue)" }
        if ($p.MainWindowHandle -ne [IntPtr]::Zero) { return $p }
        Start-Sleep -Milliseconds 100
    }
    throw "no window from yagura (pid $($p.Id)) in $TimeoutSec s"
}

# The newest window Start-Yagura started: {Pid, ErrLog}.
function Get-KitWindow {
    $f = Join-Path $script:KitDir 'window.json'
    if (-not (Test-Path -LiteralPath $f)) { throw 'Start-Yagura first' }
    Get-Content -Raw -LiteralPath $f | ConvertFrom-Json
}

function Get-KitHandle {
    $w = Get-KitWindow
    $p = Get-Process -Id $w.Pid -ErrorAction SilentlyContinue
    if (-not $p) { throw "the window (pid $($w.Pid)) has gone" }
    $p.MainWindowHandle
}

# Keys and clicks reach our window: the input desktop is Default (not the
# lock screen, not a screen saver) and the window in front is ours. Nothing
# measured after this turned false counts.
function Test-InputReady {
    $desk = [YaguraKit.Native]::InputDesktop()
    $fg = [YaguraKit.Native]::GetForegroundWindow()
    $pid_ = [uint32]0
    [void][YaguraKit.Native]::GetWindowThreadProcessId($fg, [ref]$pid_)
    $ours = (Test-Path -LiteralPath (Join-Path $script:KitDir 'window.json')) -and $pid_ -eq (Get-KitWindow).Pid
    if ($desk -ne 'Default' -or -not $ours) { "Input not ready: desktop '$desk', foreground pid $pid_" | Out-Host; return $false }
    $true
}

function Set-Foreground {
    $h = Get-KitHandle
    if ([YaguraKit.Native]::IsIconic($h)) { [void][YaguraKit.Native]::ShowWindow($h, 9) }   # SW_RESTORE
    # Windows lets a process take the foreground right after a key of its own:
    # a lone Alt tap first.
    [void][YaguraKit.Native]::Chord([uint16[]]@(), [uint16]0x12)
    [void][YaguraKit.Native]::SetForegroundWindow($h)
    Start-Sleep -Milliseconds 200
    Test-InputReady
}

$script:KitVk = @{
    ctrl = 0x11; control = 0x11; shift = 0x10; alt = 0x12; win = 0x5B
    enter = 0x0D; return = 0x0D; esc = 0x1B; escape = 0x1B; tab = 0x09; space = 0x20; backspace = 0x08
    delete = 0x2E; del = 0x2E; insert = 0x2D; home = 0x24; end = 0x23; pageup = 0x21; pagedown = 0x22
    left = 0x25; up = 0x26; right = 0x27; down = 0x28
    arrowleft = 0x25; arrowup = 0x26; arrowright = 0x27; arrowdown = 0x28
}

# 'Ctrl+Shift+Z', 'Alt+Shift++', 'F2', 'Ctrl+,'. A punctuation key is the key
# that types it on this keyboard; if that needs Shift (or AltGr), it is added.
# 'Ctrl+vk:0xBB' is that virtual key as it is, nothing added: under a JIS
# layout `=` is Shift+-, and `Ctrl+=` went as Ctrl+Shift+- (the real machine).
function ConvertTo-Chord([string]$Chord) {
    $parts = if ($Chord.Length -gt 1 -and $Chord.EndsWith('++')) { @($Chord.Substring(0, $Chord.Length - 2) -split '\+') + '+' } else { $Chord -split '\+' }
    $parts = @($parts | Where-Object { $_ -ne '' })
    if ($Chord -eq '+') { $parts = @('+') }
    $key = $parts[-1]
    $mods = [Collections.Generic.List[uint16]]::new()
    if ($parts.Count -gt 1) {
        foreach ($m in $parts[0..($parts.Count - 2)]) {
            $vk = $script:KitVk[$m.ToLower()]
            if (-not $vk) { throw "unknown modifier '$m' in '$Chord'" }
            $mods.Add([uint16]$vk)
        }
    }
    $lower = $key.ToLower()
    if ($lower -match '^vk:(0x[0-9a-f]{1,2}|\d{1,3})$') { $vk = [uint16]$Matches[1] }
    elseif ($script:KitVk.ContainsKey($lower)) { $vk = [uint16]$script:KitVk[$lower] }
    elseif ($lower -match '^f([1-9]|1\d|2[0-4])$') { $vk = [uint16](0x6F + [int]$Matches[1]) }
    elseif ($key -match '^[A-Za-z0-9]$') { $vk = [uint16][char]$key.ToUpper() }
    elseif ($key.Length -eq 1) {
        $scan = [YaguraKit.Native]::VkKeyScan($key[0])
        if ($scan -eq -1) { throw "no key types '$key' on this keyboard" }
        $vk = [uint16]($scan -band 0xFF)
        $shiftState = ($scan -shr 8) -band 0xFF
        foreach ($need in @(@(1, 0x10), @(2, 0x11), @(4, 0x12))) {
            if (($shiftState -band $need[0]) -and -not $mods.Contains([uint16]$need[1])) { $mods.Add([uint16]$need[1]) }
        }
    } else { throw "unknown key '$key' in '$Chord'" }
    [pscustomobject]@{ Mods = [uint16[]]$mods.ToArray(); Vk = $vk }
}

# Each chord in turn: Send-Keys 'Ctrl+Shift+T' 'Esc'. Throws when input would
# not reach the window, so a key never goes to whatever else is in front.
# -Hold sends each step on its own, 120 ms apart.
function Send-Keys([switch]$Hold, [Parameter(ValueFromRemainingArguments)] [string[]]$Chords) {
    foreach ($c in $Chords) {
        if (-not (Wait-InputReady)) { throw "input does not reach yagura; '$c' not sent" }
        $k = ConvertTo-Chord $c
        $sent = if ($Hold) { [YaguraKit.Native]::ChordHeld($k.Mods, $k.Vk, 120) } else { [YaguraKit.Native]::Chord($k.Mods, $k.Vk) }
        if ($sent -eq 0) { throw "SendInput sent nothing for '$c' (blocked by UIPI or the desktop)" }
        Start-Sleep -Milliseconds 150
    }
}

# The chords in one SendInput: Send-KeysAtOnce 'Ctrl+Shift+L' 'Esc'. For two
# inputs that must reach yagura together, with no window taking the
# foreground between them; egui may miss a chord sent this quickly, so use
# Send-Keys for the rest.
function Send-KeysAtOnce([Parameter(ValueFromRemainingArguments)] [string[]]$Chords) {
    if (-not (Wait-InputReady)) { throw 'input does not reach yagura; nothing sent' }
    $spec = [Collections.Generic.List[uint16]]::new()
    foreach ($c in $Chords) {
        $k = ConvertTo-Chord $c
        $spec.Add([uint16]$k.Mods.Count)
        foreach ($m in $k.Mods) { $spec.Add($m) }
        $spec.Add($k.Vk)
    }
    if ([YaguraKit.Native]::ChordsAtOnce($spec.ToArray()) -eq 0) { throw "SendInput sent nothing for $($Chords -join ', ')" }
    Start-Sleep -Milliseconds 150
}

function Send-Text([string]$Text) {
    if (-not (Wait-InputReady)) { throw 'input does not reach yagura; text not sent' }
    if ([YaguraKit.Native]::Text($Text) -eq 0) { throw 'SendInput sent nothing' }
    Start-Sleep -Milliseconds 150
}

# Test-InputReady, waiting up to -Seconds for it: a toast, the lock screen
# going or another window in front for a moment no longer fails the step.
# Another window that took the foreground and kept it (the owner's Claude
# desktop, between two sends) is sent back once a second has passed.
function Wait-InputReady([int]$Seconds = 10) {
    $until = (Get-Date).AddSeconds($Seconds)
    $retake = (Get-Date).AddSeconds(1)
    while ($true) {
        if (Test-InputReady) { return $true }
        if ((Get-Date) -ge $until) { return $false }
        if ((Get-Date) -ge $retake -and [YaguraKit.Native]::InputDesktop() -eq 'Default') {
            "Taking the foreground back" | Out-Host
            [void](Set-Foreground)
            $retake = (Get-Date).AddSeconds(2)
            continue
        }
        Start-Sleep -Milliseconds 500
    }
}

# The cursor to X, Y in egui points from the client area's top-left.
function Move-KitCursor([double]$X, [double]$Y, [double]$Scale = 1.0) {
    $h = Get-KitHandle
    $px = [YaguraKit.Native]::GetDpiForWindow($h) / 96.0 * $Scale
    $pt = [YaguraKit.Native+POINT]::new()
    $pt.X = [int][Math]::Round($X * $px)
    $pt.Y = [int][Math]::Round($Y * $px)
    [void][YaguraKit.Native]::ClientToScreen($h, [ref]$pt)
    [void][YaguraKit.Native]::SetCursorPos($pt.X, $pt.Y)
}

# X, Y in egui points from the client area's top-left, as the focus log
# gives them; -Scale when yagura's zoom is not 100%.
function Send-Click([double]$X, [double]$Y, [switch]$Right, [switch]$Middle, [double]$Scale = 1.0) {
    if (-not (Wait-InputReady)) { throw 'input does not reach yagura; click not sent' }
    Move-KitCursor $X $Y $Scale
    Start-Sleep -Milliseconds 50
    $button = if ($Right) { 1 } elseif ($Middle) { 2 } else { 0 }
    if ([YaguraKit.Native]::Click($button) -eq 0) { throw 'SendInput sent nothing for the click' }
    Start-Sleep -Milliseconds 150
}

# The left button down at X, Y, moved to ToX, ToY in -Steps steps, and up.
function Send-Drag([double]$X, [double]$Y, [double]$ToX, [double]$ToY, [int]$Steps = 12, [double]$Scale = 1.0) {
    if (-not (Wait-InputReady)) { throw 'input does not reach yagura; drag not sent' }
    Move-KitCursor $X $Y $Scale
    Start-Sleep -Milliseconds 50
    if ([YaguraKit.Native]::Button($true) -eq 0) { throw 'SendInput sent nothing for the drag' }
    try {
        for ($k = 1; $k -le $Steps; $k++) {
            Start-Sleep -Milliseconds 30
            Move-KitCursor ($X + ($ToX - $X) * $k / $Steps) ($Y + ($ToY - $Y) * $k / $Steps) $Scale
        }
        Start-Sleep -Milliseconds 50
    } finally { [void][YaguraKit.Native]::Button($false) }
    Start-Sleep -Milliseconds 150
}

# The wheel turned -Notches at X, Y; negative is towards the user (down).
function Send-Wheel([double]$X, [double]$Y, [int]$Notches = -1, [double]$Scale = 1.0) {
    if (-not (Wait-InputReady)) { throw 'input does not reach yagura; wheel not sent' }
    Move-KitCursor $X $Y $Scale
    Start-Sleep -Milliseconds 50
    if ([YaguraKit.Native]::Wheel($Notches) -eq 0) { throw 'SendInput sent nothing for the wheel' }
    Start-Sleep -Milliseconds 150
}

# Two clicks at X, Y in one SendInput (see Send-Click for the points).
function Send-DoubleClick([double]$X, [double]$Y, [double]$Scale = 1.0) {
    if (-not (Wait-InputReady)) { throw 'input does not reach yagura; double-click not sent' }
    Move-KitCursor $X $Y $Scale
    Start-Sleep -Milliseconds 50
    if ([YaguraKit.Native]::DoubleClick() -eq 0) { throw 'SendInput sent nothing for the double-click' }
    # Past the double-click time, so the next click is not a third.
    Start-Sleep -Milliseconds ([YaguraKit.Native]::DoubleClickTime() + 50)
}

# The cursor's shape: Arrow, IBeam, SizeWE (a divider between columns),
# SizeNS (between rows), Hand (a link), ... With -X and -Y it is moved there
# first and given a few frames to change.
function Get-KitCursor([Nullable[double]]$X, [Nullable[double]]$Y, [double]$Scale = 1.0) {
    if ($null -ne $X -and $null -ne $Y) {
        Move-KitCursor $X $Y $Scale
        Start-Sleep -Milliseconds 300
    }
    [YaguraKit.Native]::CursorName()
}

# The window's outer size (and, with -X and -Y, its place) in physical
# pixels, restored first when it is maximized. Returns the client area as
# {Width, Height} in pixels and in egui points (100% zoom).
function Set-KitWindow([int]$Width, [int]$Height, [Nullable[int]]$X, [Nullable[int]]$Y) {
    $h = Get-KitHandle
    [void][YaguraKit.Native]::ShowWindow($h, 9)   # SW_RESTORE
    $flags = 0x0004 -bor 0x0010   # SWP_NOZORDER, SWP_NOACTIVATE
    if ($null -eq $X -or $null -eq $Y) { $flags = $flags -bor 0x0002; $X = 0; $Y = 0 }   # SWP_NOMOVE
    if (-not [YaguraKit.Native]::SetWindowPos($h, [IntPtr]::Zero, $X, $Y, $Width, $Height, $flags)) { throw 'SetWindowPos failed' }
    Start-Sleep -Milliseconds 300
    $r = [YaguraKit.Native+RECT]::new()
    [void][YaguraKit.Native]::GetClientRect($h, [ref]$r)
    $px = [YaguraKit.Native]::GetDpiForWindow($h) / 96.0
    [pscustomobject]@{ Width = $r.Right; Height = $r.Bottom; PointsWidth = [Math]::Round($r.Right / $px); PointsHeight = [Math]::Round($r.Bottom / $px) }
}

# The window as it is drawn (PrintWindow with PW_RENDERFULLCONTENT), to
# <kit dir>\shots\<name>.png; returns the path. Crop it with -Crop x,y,w,h
# in physical pixels from the window's corner.
function Save-Shot([string]$Name, [int[]]$Crop) {
    Add-Type -AssemblyName System.Drawing
    $h = Get-KitHandle
    $r = [YaguraKit.Native+RECT]::new()
    [void][YaguraKit.Native]::GetWindowRect($h, [ref]$r)
    $bmp = [Drawing.Bitmap]::new($r.Right - $r.Left, $r.Bottom - $r.Top)
    $g = [Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    try { [void][YaguraKit.Native]::PrintWindow($h, $hdc, 2) } finally { $g.ReleaseHdc($hdc); $g.Dispose() }
    if ($Crop) {
        $part = $bmp.Clone([Drawing.Rectangle]::new($Crop[0], $Crop[1], $Crop[2], $Crop[3]), $bmp.PixelFormat)
        $bmp.Dispose()
        $bmp = $part
    }
    $dir = Join-Path $script:KitDir 'shots'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $path = Join-Path $dir "$Name.png"
    $bmp.Save($path, [Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    $path
}

# A person's file (~/.claude/settings.json, a profile): its hash and a copy
# before a row touches it. Restore-UserFile puts it back and says whether the
# hash matches again.
function Get-BackupName([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path).ToLowerInvariant()
    $hash = -join ([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($full))[0..7] | ForEach-Object { $_.ToString('x2') })
    Join-Path $script:KitDir "backup\$([IO.Path]::GetFileName($Path)).$hash"
}

function Backup-UserFile([string]$Path) {
    $base = Get-BackupName $Path
    if (Test-Path -LiteralPath "$base.json") { throw "$Path is backed up already ($base.json): Restore-UserFile it first" }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $base) | Out-Null
    $had = Test-Path -LiteralPath $Path
    if ($had) { Copy-Item -LiteralPath $Path -Destination "$base.copy" }
    $hash = if ($had) { (Get-FileHash -LiteralPath $Path).Hash } else { 'absent' }
    @{ Path = $Path; Copy = "$base.copy"; Hash = $hash; Had = $had } | ConvertTo-Json | Set-Content -LiteralPath "$base.json"
    "Backed up $Path ($hash) to $base.copy" | Out-Host
}

function Restore-UserFile([string]$Path) {
    $base = Get-BackupName $Path
    if (-not (Test-Path -LiteralPath "$base.json")) { throw "no backup of $Path" }
    $b = Get-Content -Raw -LiteralPath "$base.json" | ConvertFrom-Json
    if ($b.Had) { Copy-Item -LiteralPath $b.Copy -Destination $Path -Force } else { Remove-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue }
    $now = if (Test-Path -LiteralPath $Path) { (Get-FileHash -LiteralPath $Path).Hash } else { 'absent' }
    "Restored ${Path}: $now (was $($b.Hash)) $(if ($now -eq $b.Hash) { 'MATCH' } else { 'DIFFERENT' })" | Out-Host
    if ($now -eq $b.Hash) { Remove-Item -LiteralPath "$base.json" }
    $now -eq $b.Hash
}

# Every window this kit started; never the owner's.
function Stop-Mine {
    $list = Join-Path $script:KitDir 'windows.txt'
    $windows = @(if (Test-Path -LiteralPath $list) { Get-Content -LiteralPath $list })
    foreach ($line in $windows) {
        # Only the process that was started: a process id can be reused.
        $id, $ticks = $line -split "`t", 2
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($p -and "$($p.StartTime.Ticks)" -eq $ticks) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
    }
    Remove-Item -LiteralPath $list -ErrorAction SilentlyContinue
    "Stopped $($windows.Count) window(s)" | Out-Host
}
