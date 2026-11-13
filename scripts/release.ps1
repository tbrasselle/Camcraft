# release.ps1 - thin wrapper over `cargo xtask release` (xtask/src/release*.rs, #749), kept so the old CLI
# and fork process-sync keep working. The cut itself (preflight, gate, version bump, commits, build,
# signing via sign.ps1, the zip, the installer via installer.ps1) lives once, in Rust.
# Provenance: our own release law (AGENTS.md "A cut goes PR review -> merge to main -> tag").
#
#   scripts/release.ps1 -Version X.Y.Z -NoTag [-Title T] [-SkipGate] [-SkipBuild] [-Installer] [-DryRun]
#   ==  cargo xtask release --version X.Y.Z --no-tag [--title T] [--skip-gate] [--skip-build] [--installer] [--dry-run]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Title = "release",
    [switch]$SkipGate,
    [switch]$SkipBuild,
    [switch]$Installer,
    [switch]$DryRun,
    [switch]$NoTag
)
$a = @('xtask', 'release', '--version', $Version, '--title', $Title)
if ($NoTag) { $a += '--no-tag' }
if ($SkipGate) { $a += '--skip-gate' }
if ($SkipBuild) { $a += '--skip-build' }
if ($Installer) { $a += '--installer' }
if ($DryRun) { $a += '--dry-run' }
Push-Location (Join-Path $PSScriptRoot '..')
try { & cargo @a; $code = $LASTEXITCODE } finally { Pop-Location }
exit $code
