# Gather docs/decisions/*.md into RECOMMENDED.md at a release cut - thin wrapper over
# `cargo xtask decisions` (xtask/, #748). Usage: scripts/collect-decisions.ps1 -Version 0.14.0
param([Parameter(Mandatory = $true)][string]$Version)
Push-Location (Join-Path $PSScriptRoot '..')
try { & cargo xtask decisions --version $Version; $code = $LASTEXITCODE } finally { Pop-Location }
exit $code
