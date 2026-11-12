# gate.ps1 - thin wrapper over `cargo xtask gate` (xtask/, #748), kept so the old CLI and fork process-sync
# keep working. The gate itself (tiers, SLOW_BINARIES, quick's diff-to-test selection) lives once, in Rust.
# Provenance: our own build law (AGENTS.md "Three gate tiers").
#
#   scripts/gate.ps1 -Tier quick [-Base main] [-Heavy]   ==  cargo xtask gate quick [--base main] [--heavy]
#   scripts/gate.ps1 -Tier batch | release               ==  cargo xtask gate batch | release
#   scripts/gate.ps1 -Test <filter> [-Crate app] [-Binary name] [-Exact]   ==  cargo xtask gate test <filter> ...
#   -DryRun                                              ==  --dry-run
[CmdletBinding()]
param(
    [ValidateSet('quick', 'batch', 'release')][string]$Tier = 'quick',
    [string]$Base = 'main',
    [string]$Test, [string]$Crate, [string]$Binary,
    [switch]$Exact, [switch]$Heavy, [switch]$DryRun
)
$a = @('xtask', 'gate')
if ($Test) {
    $a += @('test', $Test)
    if ($Crate) { $a += @('--crate', $Crate) }
    if ($Binary) { $a += @('--binary', $Binary) }
    if ($Exact) { $a += '--exact' }
} else { $a += @($Tier, '--base', $Base) }
if ($Heavy) { $a += '--heavy' }
if ($DryRun) { $a += '--dry-run' }
Push-Location (Join-Path $PSScriptRoot '..')
try { & cargo @a; $code = $LASTEXITCODE } finally { Pop-Location }
exit $code
