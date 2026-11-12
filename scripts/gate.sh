#!/usr/bin/env bash
# gate.sh - thin wrapper over `cargo xtask gate` (xtask/, #748), kept so the old CLI and fork process-sync
# keep working. The gate itself (tiers, SLOW_BINARIES, quick's diff-to-test selection) lives once, in Rust.
# Provenance: our own build law (AGENTS.md "Three gate tiers").
#
#   scripts/gate.sh quick [base]                          ==  cargo xtask gate quick [--base base]
#   scripts/gate.sh batch | release                       ==  cargo xtask gate batch | release
#   scripts/gate.sh test <filter> [crate] [--exact]       ==  cargo xtask gate test <filter> [--crate c] [--exact]
#   DRY_RUN=1 ...                                         ==  --dry-run
# --ignored is accepted and dropped: the gate reads #[ignore] from the source and runs those in release.
# Any other --flag is passed through to `cargo xtask gate` as is.
set -u
cd "$(dirname "$0")/.." || exit 2
tier="${1:-quick}"
[ $# -gt 0 ] && shift
args=(xtask gate "$tier")
case "$tier" in
quick) if [ $# -gt 0 ] && [ "${1#-}" = "$1" ]; then args+=(--base "$1"); shift; fi ;;
test) if [ $# -gt 0 ]; then args+=("$1"); shift; fi ;;
esac
for a in "$@"; do
    case "$a" in
        --ignored) ;;
        -*) args+=("$a") ;;
        *) args+=(--crate "$a") ;;
    esac
done
[ -n "${DRY_RUN:-}" ] && args+=(--dry-run)
exec cargo "${args[@]}"
