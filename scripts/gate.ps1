# gate.ps1 - the three gate tiers (quick | batch | release) and single-test runs for the camcraft workspace.
# Provenance: our own build law (AGENTS.md "Three gate tiers"); drives only the public cargo / libtest CLI.
#
#   scripts/gate.ps1 -Tier quick [-Base main]        tests the diff touches (test fns where it can)
#   scripts/gate.ps1 -Tier batch                     every debug suite minus SLOW_BINARIES, ui, clippy
#   scripts/gate.ps1 -Tier release                   everything, plus the --ignored heavy lanes in release and the ui snapshot lane
#   scripts/gate.ps1 -Test <filter> [-Crate app] [-Binary name] [-Exact]   exactly the matching test(s)
#   -DryRun prints the plan without running it. -Heavy lets quick run changed #[ignore]d fns (release --ignored). Logs: gate-logs/<tier>-<timestamp>/.
[CmdletBinding()]
param(
    [ValidateSet('quick', 'batch', 'release')][string]$Tier = 'quick',
    [string]$Base = 'main',
    [string]$Test, [string]$Crate, [string]$Binary,
    [switch]$Exact, [switch]$Heavy, [switch]$DryRun
)
$ErrorActionPreference = 'Stop'

# Debug test binaries over ~40 s: skipped by batch, run by release (and by quick when the diff maps to them).
# Format crate-dir:binary. Measured 2026-11-07 (debug, -j 6, 6 test threads).
$SLOW_BINARIES = @(
    'motion:finish3d',             # ~50 s
    'motion:pivot5d',              # ~46 s
    'motion:flow',                 # ~42 s
    'app:center_cut_multiaxis'     # ~40 s
)
# Crate dirs in dependency order. kernel builds a C chain (truck dev-dep) for its own tests: release tier
# and quick-when-touched only. A touch to a shared crate (or the workspace manifest) makes quick a batch.
$BATCH_CRATES = @('geometry', 'cl', 'tools', 'post', 'sim', 'motion', 'app')
$ALL_CRATES = @('geometry', 'kernel', 'cl', 'tools', 'post', 'sim', 'motion', 'app', 'ui')
$SHARED = @('geometry', 'cl')
$CLIPPY = @('geometry', 'cl', 'tools', 'post', 'sim', 'motion', 'app', 'ui')
$J = '6'; $T = '--test-threads=6'

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$label = if ($Test) { 'test' } else { $Tier }
$LogDir = Join-Path $Root ("gate-logs/{0}-{1}" -f $label, (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $LogDir | Out-Null
$Steps = New-Object System.Collections.ArrayList
$Clock = [Diagnostics.Stopwatch]::StartNew()

function Pkg([string]$dir) { "camcraft-$dir" }
function Cwd-Of([string]$dir) { if ($dir -eq 'ui') { Join-Path $Root 'crates/ui' } else { $Root } }
function Is-Slow([string]$dir, [string]$bin) { $SLOW_BINARIES -contains "${dir}:$bin" }
function Binaries([string]$dir) {
    $td = Join-Path $Root "crates/$dir/tests"
    if (Test-Path $td) { @(Get-ChildItem $td -Filter *.rs -File | ForEach-Object { $_.BaseName } | Sort-Object) } else { @() }
}

# One cargo command, its output to its own log; records pass/fail and the failing binaries.
function Step([string]$Name, [string[]]$CargoArgs, [string]$Dir = $Root) {
    $n = $Steps.Count + 1
    $log = Join-Path $LogDir ('{0:D2}-{1}.log' -f $n, ($Name -replace '[^\w\-]+', '_'))
    Write-Host ('[{0:D2}] {1}' -f $n, $Name) -ForegroundColor Cyan
    Write-Host ("     (cwd {0}) cargo {1}" -f (Split-Path $Dir -Leaf), ($CargoArgs -join ' '))
    if ($DryRun) { [void]$Steps.Add([pscustomobject]@{ Name = $Name; Ok = $true; Secs = 0; Note = 'dry run'; Log = $log }); return }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    Push-Location $Dir
    $eap = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    try { & cargo @CargoArgs 2>&1 | ForEach-Object { "$_" } | Out-File -FilePath $log -Encoding utf8; $code = $LASTEXITCODE }
    finally { $ErrorActionPreference = $eap; Pop-Location }
    $sw.Stop()
    $cur = ''; $fails = @(); $compile = $false
    foreach ($l in Get-Content $log) {
        if ($l -match '^\s+Running (\S+)') { $cur = $Matches[1] }
        elseif ($l -match '^test result: FAILED') { $fails += $cur }
        elseif ($l -match '^error: could not compile') { $compile = $true }
    }
    $note = if ($compile) { 'compile error' } elseif ($fails) { 'failed: ' + ($fails -join ', ') } elseif ($code -ne 0) { "exit $code" } else { '' }
    [void]$Steps.Add([pscustomobject]@{ Name = $Name; Ok = ($code -eq 0); Secs = [int]$sw.Elapsed.TotalSeconds; Note = $note; Log = $log })
    Write-Host ('     {0} in {1}s {2}' -f $(if ($code -eq 0) { 'ok' } else { 'FAIL' }), [int]$sw.Elapsed.TotalSeconds, $note)
}

# The #[test] fns of a file: name, line span (doc/attr lines through the closing brace), ignored or not.
function Test-Fns([string]$file) {
    $lines = @(Get-Content $file)
    $out = @()
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -notmatch '^\s*#\[test\]') { continue }
        $s = $i; while ($s -gt 0 -and $lines[$s - 1] -match '^\s*(///|#\[)') { $s-- }
        $ign = $false; $k = $i
        while ($k -lt $lines.Count -and $lines[$k] -notmatch '^\s*(pub\s+)?(async\s+)?fn\s+(\w+)') { if ($lines[$k] -match '#\[ignore') { $ign = $true }; $k++ }
        if ($k -ge $lines.Count) { break }
        $name = $Matches[3]; $depth = 0; $e = $k; $opened = $false
        for (; $e -lt $lines.Count; $e++) {
            foreach ($ch in $lines[$e].ToCharArray()) { if ($ch -eq '{') { $depth++; $opened = $true } elseif ($ch -eq '}') { $depth-- } }
            if ($opened -and $depth -le 0) { break }
        }
        $out += [pscustomobject]@{ Name = $name; Start = $s + 1; End = $e + 1; Ignored = $ign }
        $i = $e
    }
    $out
}

# The Gate map paragraph of a suite header, as one string.
function Gate-Map([string]$file) {
    $on = $false; $acc = @()
    foreach ($l in (Get-Content $file -TotalCount 150)) {
        if (-not $on -and $l -match 'Gate map') { $on = $true }
        if ($on) { if ($l -notmatch '^//!' -or $l -match '^//!\s*$') { break }; $acc += $l }
    }
    $acc -join ' '
}

function Run-Suites([string[]]$dirs, [bool]$skipSlow) {
    foreach ($d in $dirs) {
        if ($d -eq 'ui') { Step 'ui suites (cwd crates/ui)' @('test', '-j', $J, '-p', (Pkg 'ui'), '--no-fail-fast', '--', $T) (Cwd-Of 'ui'); continue }
        if (-not $skipSlow) { Step "$d suites" @('test', '-j', $J, '-p', (Pkg $d), '--no-fail-fast', '--', $T); continue }
        $sel = @('--lib'); $skipped = @()
        foreach ($b in (Binaries $d)) { if (Is-Slow $d $b) { $skipped += $b } else { $sel += @('--test', $b) } }
        $nm = "$d suites" + $(if ($skipped) { " (slow, release tier: $($skipped -join ', '))" } else { '' })
        Step $nm (@('test', '-j', $J, '-p', (Pkg $d), '--no-fail-fast') + $sel + @('--', $T))
    }
}
function Run-Clippy { Step 'clippy -D warnings' (@('clippy', '-j', $J) + ($CLIPPY | ForEach-Object { '-p', (Pkg $_) }) + @('--all-targets', '--', '-D', 'warnings')) }

function Summary {
    $Clock.Stop()
    Write-Host "`n== gate summary ($label) - logs in $LogDir" -ForegroundColor Cyan
    foreach ($s in $Steps) { Write-Host ('  {0,-4} {1,5}s  {2}  {3}' -f $(if ($s.Ok) { 'ok' } else { 'FAIL' }), $s.Secs, $s.Name, $s.Note) -ForegroundColor $(if ($s.Ok) { 'Green' } else { 'Red' }) }
    $bad = @($Steps | Where-Object { -not $_.Ok }).Count
    Write-Host ('  wall {0:N1} min, {1} step(s), {2} failed' -f $Clock.Elapsed.TotalMinutes, $Steps.Count, $bad)
    if ($bad -gt 0) { exit 1 } else { exit 0 }
}

# ---- single tests: -Test <filter> ----------------------------------------------------------------------
if ($Test) {
    $dirs = if ($Crate) { @($Crate -replace '^camcraft-', '') } else { $ALL_CRATES }
    $hits = @()   # dir, kind (test|lib), binary, ignored
    foreach ($d in $dirs) {
        $files = @()
        if ($Binary) { $f = Join-Path $Root "crates/$d/tests/$Binary.rs"; if (Test-Path $f) { $files += Get-Item $f } }
        else {
            $files += @(Get-ChildItem (Join-Path $Root "crates/$d/tests") -Filter *.rs -File -ErrorAction SilentlyContinue)
            $files += @(Get-ChildItem (Join-Path $Root "crates/$d/src") -Filter *.rs -File -Recurse -ErrorAction SilentlyContinue)
        }
        $pat = if ($Exact) { "fn\s+$([regex]::Escape($Test))\s*[\(<]" } else { "fn\s+\w*$([regex]::Escape($Test))\w*\s*[\(<]" }
        foreach ($f in $files) {
            if (-not $Binary -and -not (Select-String -Path $f.FullName -Pattern $pat -Quiet)) { continue }
            $kind = if ($f.FullName -match '[\\/]tests[\\/][^\\/]+\.rs$' -and $f.DirectoryName -match '[\\/]tests$') { 'test' } else { 'lib' }
            foreach ($fn in (Test-Fns $f.FullName)) {
                $ok = if ($Exact) { $fn.Name -eq $Test } else { $fn.Name.Contains($Test) }
                if ($ok) { $hits += [pscustomobject]@{ Dir = $d; Kind = $kind; Bin = $f.BaseName; Fn = $fn.Name; Ignored = $fn.Ignored } }
            }
        }
    }
    if (-not $hits) { Write-Host "no #[test] fn matches '$Test'" -ForegroundColor Red; exit 1 }
    Write-Host "matched:"; $hits | ForEach-Object { Write-Host ("  {0} {1} {2}::{3}{4}" -f $_.Dir, $_.Kind, $_.Bin, $_.Fn, $(if ($_.Ignored) { '  [ignored -> release]' } else { '' })) }
    foreach ($g in ($hits | Group-Object Dir, Kind, Bin, Ignored)) {
        $h = $g.Group[0]
        $target = if ($h.Kind -eq 'test') { @('--test', $h.Bin) } else { @('--lib') }
        $prof = if ($h.Ignored) { @('--release') } else { @() }
        $tail = @('--', $T); if ($h.Ignored) { $tail += '--ignored' }
        # --exact needs the full test path: top-level fns in suites; lib unit tests filter by substring.
        if ($Exact -and $h.Kind -eq 'test') { $tail += @('--exact') + @($g.Group | ForEach-Object { $_.Fn }) } else { $tail += $Test }
        Step ("{0} {1}{2}" -f $h.Dir, $(if ($h.Kind -eq 'test') { $h.Bin } else { 'lib' }), $(if ($h.Ignored) { ' (ignored, release)' } else { '' })) `
            (@('test') + $prof + @('-j', $J, '-p', (Pkg $h.Dir)) + $target + $tail) (Cwd-Of $h.Dir)
    }
    Summary
}

# ---- quick: what the diff touches ------------------------------------------------------------------------
if ($Tier -eq 'quick') {
    $mb = (git -C $Root merge-base $Base HEAD); if ($LASTEXITCODE -ne 0 -or -not $mb) { throw "no merge base with '$Base'" }
    $changed = @(git -C $Root diff --name-only $mb) + @(git -C $Root ls-files --others --exclude-standard) |
        Where-Object { $_ } | ForEach-Object { $_ -replace '\\', '/' } | Sort-Object -Unique
    $untracked = @(git -C $Root ls-files --others --exclude-standard) | ForEach-Object { $_ -replace '\\', '/' }
    Write-Host ("changed since {0} ({1}): {2} file(s)" -f $Base, $mb.Substring(0, 7), $changed.Count)
    $shared = @($changed | Where-Object { $_ -match '^(Cargo\.(toml|lock)$|\.cargo/|rust-toolchain)' -or $_ -match ('^crates/(' + ($SHARED -join '|') + ')/') })
    if ($shared) { Write-Host "shared crate / workspace touched ($($shared[0])): quick falls back to batch" -ForegroundColor Yellow; $Tier = 'batch' }
    else {
        $whole = @{}; $fns = @{}; $ign = @{}; $libs = @{}; $mods = @(); $why = @{}
        foreach ($f in $changed) {
            if ($f -notmatch '^crates/([^/]+)/(.+)$') { continue }
            $d = $Matches[1]; $rest = $Matches[2]
            if ($ALL_CRATES -notcontains $d) { continue }
            $abs = Join-Path $Root $f
            if ($rest -match '^tests/([^/]+)\.rs$') {
                $b = $Matches[1]; $key = "${d}:$b"
                if (-not (Test-Path $abs)) { continue }
                if ($untracked -contains $f) { $whole[$key] = $true; $why[$key] = 'new suite'; continue }
                $hunks = @(git -C $Root diff -U0 $mb -- $f | Where-Object { $_ -match '^@@ ' })
                $all = Test-Fns $abs; $outside = $false
                foreach ($hk in $hunks) {
                    if ($hk -notmatch '^@@ -\S+ \+(\d+)(?:,(\d+))? @@') { continue }
                    $s = [int]$Matches[1]; $c = if ($Matches[2]) { [int]$Matches[2] } else { 1 }
                    $e = [Math]::Max($s, $s + $c - 1)
                    $in = @($all | Where-Object { $_.Start -le $e -and $_.End -ge [Math]::Max($s, 1) })
                    if (-not $in) { $outside = $true }
                    foreach ($tf in $in) { if ($tf.Ignored) { $ign["$key|$($tf.Name)"] = $true } else { $fns["$key|$($tf.Name)"] = $true } }
                }
                if ($outside) { $whole[$key] = $true; $why[$key] = 'suite changed outside its test fns' }
            }
            elseif ($rest -match '^tests/' -or $rest -eq 'Cargo.toml' -or $rest -eq 'build.rs') {
                foreach ($b in (Binaries $d)) { $whole["${d}:$b"] = $true; $why["${d}:$b"] = "shared test input $rest" }; $libs[$d] = $true
            }
            elseif ($rest -match '^src/([^/]+?)(\.rs)?(/([^/]+?)(\.rs)?(/.*)?)?$') {
                $libs[$d] = $true
                $m = $Matches[1]; $sub = if ($Matches[4] -and @('mod', 'tests') -notcontains $Matches[4]) { $Matches[4] } else { '' }
                if (@('lib', 'main', 'bin') -notcontains $m) { $mods += [pscustomobject]@{ Dir = $d; Mod = $m; Sub = $sub } }
            }
        }
        $mods = @($mods | Sort-Object Dir, Mod, Sub -Unique)
        # src modules -> the suites whose Gate map names them (own crate: ::mod or {..mod..}; others: crate::mod).
        # A file in a submodule picks the suites naming mod::sub or mod (.. sub ..) when there are any,
        # else every suite naming the module.
        foreach ($m in $mods) {
            $mm = [regex]::Escape($m.Mod); $ms = [regex]::Escape($m.Sub); $bySub = @(); $byMod = @()
            foreach ($d in $ALL_CRATES) {
                foreach ($b in (Binaries $d)) {
                    $gm = Gate-Map (Join-Path $Root "crates/$d/tests/$b.rs"); if (-not $gm) { continue }
                    $pre = if ($m.Dir -eq $d) { '' } else { "\b(camcraft_)?$($m.Dir)" }
                    $modPat = if ($m.Dir -eq $d) { "::$mm\b|\{[^}]*\b$mm\b" } else { "$pre::$mm\b" }
                    if ($gm -notmatch $modPat) { continue }
                    $byMod += "${d}:$b"
                    if ($m.Sub -and $gm -match "(::|\{[^}]*\b)$mm(::$ms\b|\s*\([^)]*\b$ms\b)") { $bySub += "${d}:$b" }
                }
            }
            $pick = if ($bySub) { $bySub } else { $byMod }
            $lbl = if ($bySub) { "$($m.Dir)::$($m.Mod)::$($m.Sub)" } else { "$($m.Dir)::$($m.Mod)" }
            Write-Host ("  src {0}::{1}{2} -> {3} suite(s) by gate map ({4})" -f $m.Dir, $m.Mod, $(if ($m.Sub) { "::$($m.Sub)" } else { '' }), $pick.Count, $(if ($bySub) { 'submodule' } else { 'module' }))
            foreach ($k in $pick) { $whole[$k] = $true; if (-not $why[$k]) { $why[$k] = "gate map names $lbl" } }
        }
        Write-Host 'quick plan:'
        foreach ($d in ($libs.Keys | Sort-Object)) { Write-Host "  $d lib (unit tests)" }
        foreach ($k in ($whole.Keys | Sort-Object)) { Write-Host "  $k  whole suite - $($why[$k])" }
        $ignNote = if ($Heavy) { '  [ignored -> release --ignored]' } else { '  [ignored: heavy lane, skipped - pass -Heavy or use -Test <name>]' }
        foreach ($k in (@($fns.Keys) + @($ign.Keys) | Sort-Object)) { $kk, $fn = $k -split '\|'; if (-not $whole[$kk]) { Write-Host ("  {0}::{1}{2}" -f $kk, $fn, $(if ($ign[$k]) { $ignNote } else { '' })) } }
        $buildOnly = @{}
        if (-not $Heavy) { foreach ($k in $ign.Keys) { $buildOnly[($k -split '\|')[0]] = $true }; $ign = @{} }
        if (-not ($libs.Count + $whole.Count + $fns.Count + $ign.Count + $buildOnly.Count)) { Write-Host '  nothing in crates/ changed: no tests to run' }
        foreach ($d in $ALL_CRATES) {
            $sel = @(); if ($libs[$d]) { $sel += '--lib' }
            foreach ($k in ($whole.Keys | Sort-Object)) { $kd, $kb = $k -split ':'; if ($kd -eq $d) { $sel += @('--test', $kb) } }
            if ($sel) { Step "$d suites" (@('test', '-j', $J, '-p', (Pkg $d), '--no-fail-fast') + $sel + @('--', $T)) (Cwd-Of $d) }
            # A suite whose only changed fns are skipped heavy ones still has to compile.
            $bo = @(); foreach ($k in ($buildOnly.Keys | Sort-Object)) { $kd, $kb = $k -split ':'; if ($kd -eq $d -and -not $whole[$k] -and -not ($fns.Keys | Where-Object { $_ -like "$k|*" })) { $bo += @('--test', $kb) } }
            if ($bo) { Step "$d build-only (changed fns are heavy lane)" (@('test', '-j', $J, '-p', (Pkg $d), '--no-run') + $bo) (Cwd-Of $d) }
            foreach ($pair in @(@($fns, $false), @($ign, $true))) {
                $map = $pair[0]; $isIgn = $pair[1]
                foreach ($g in ($map.Keys | Where-Object { ($_ -split '\|')[0] -like "${d}:*" -and -not $whole[($_ -split '\|')[0]] } | Group-Object { ($_ -split '\|')[0] })) {
                    $b = ($g.Name -split ':')[1]; $names = @($g.Group | ForEach-Object { ($_ -split '\|')[1] })
                    $prof = if ($isIgn) { @('--release') } else { @() }; $extra = if ($isIgn) { @('--ignored') } else { @() }
                    Step ("{0} {1} ({2} fn{3})" -f $d, $b, $names.Count, $(if ($isIgn) { ', ignored, release' } else { '' })) `
                        (@('test') + $prof + @('-j', $J, '-p', (Pkg $d), '--test', $b, '--', $T) + $extra + @('--exact') + $names) (Cwd-Of $d)
                }
            }
        }
        Summary
    }
}

# ---- batch / release ---------------------------------------------------------------------------------
if ($Tier -eq 'batch') {
    Write-Host ("batch: debug suites minus SLOW_BINARIES ({0}), ui, clippy" -f ($SLOW_BINARIES -join ', '))
    Run-Suites $BATCH_CRATES $true
    Run-Suites @('ui') $false
    Run-Clippy
    Summary
}
Write-Host 'release: every debug suite, ui, clippy, then the --ignored heavy lanes in release and the ui snapshot lane'
Run-Suites ($ALL_CRATES | Where-Object { $_ -ne 'ui' }) $false
Run-Suites @('ui') $false
Run-Clippy
foreach ($d in @('motion', 'app')) { Step "$d heavy lane (release --ignored)" @('test', '--release', '-j', $J, '-p', (Pkg $d), '--no-fail-fast', '--', $T, '--ignored') }
# The ui pixel snapshot goldens (#255): #[ignore]d out of quick/batch, run here only, debug like the goldens
# were made, CWD crates/ui so they resolve tests/snapshots/. Regenerate at the cut with UPDATE_SNAPSHOTS=1.
Step 'ui snapshot lane (debug --ignored, cwd crates/ui)' @('test', '-j', $J, '-p', (Pkg 'ui'), '--lib', '--no-fail-fast', '--', $T, '--ignored', 'snapshot') (Cwd-Of 'ui')
Summary
