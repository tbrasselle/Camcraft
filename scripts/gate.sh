#!/usr/bin/env bash
# gate.sh - the three gate tiers (quick | batch | release) and single-test runs, for macOS / Linux shells.
# Provenance: our own build law (AGENTS.md "Three gate tiers"); a bash twin of scripts/gate.ps1 that drives
# only the public cargo / libtest CLI. Portable to the bash 3.2 macOS ships (no associative arrays, no mapfile).
#
#   scripts/gate.sh quick [base]          suites of the crates the diff touches (vs merge-base with base, default main)
#   scripts/gate.sh batch                 every debug suite minus SLOW_BINARIES, ui, clippy
#   scripts/gate.sh release               everything, plus the --ignored heavy lanes in release and the ui snapshot lane
#   scripts/gate.sh test <filter> [crate] [--exact] [--ignored]   tests whose name matches, in one crate or all
#   DRY_RUN=1 prints the plan without running it. Logs: gate-logs/<tier>-<timestamp>/.
#
# Quick is coarser than gate.ps1's: it runs every suite of a touched crate rather than mapping the diff down to
# single test fns. That runs more, never less. A touch to a shared crate or the workspace manifest makes it a batch.
set -u

# Debug test binaries over ~40 s: skipped by batch, run by release. Keep in step with gate.ps1.
SLOW_BINARIES="motion:finish3d motion:pivot5d motion:flow app:center_cut_multiaxis"
BATCH_CRATES="geometry cl tools post sim motion app"
ALL_CRATES="geometry kernel cl tools post sim motion app ui"
SHARED="geometry cl"
CLIPPY="geometry cl tools post sim motion app ui"
J=6
T=--test-threads=6

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TIER="${1:-quick}"
[ $# -gt 0 ] && shift
LOG_DIR="$ROOT/gate-logs/$TIER-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$LOG_DIR"
START=$(date +%s)
N=0
SUMMARY=""
BAD=0

contains() { case " $1 " in *" $2 "*) return 0 ;; esac; return 1; }
cwd_of() { if [ "$1" = ui ]; then echo "$ROOT/crates/ui"; else echo "$ROOT"; fi; }

# step <name> <cwd> <cargo args...>: one cargo command, its output to its own log; records pass/fail.
step() {
    name="$1"; dir="$2"; shift 2
    N=$((N + 1))
    log="$LOG_DIR/$(printf '%02d' "$N")-$(echo "$name" | sed 's/[^A-Za-z0-9_-]\{1,\}/_/g').log"
    printf '[%02d] %s\n     (cwd %s) cargo %s\n' "$N" "$name" "$(basename "$dir")" "$*"
    if [ -n "${DRY_RUN:-}" ]; then SUMMARY="$SUMMARY  ok        0s  $name  dry run\n"; return; fi
    t0=$(date +%s)
    (cd "$dir" && cargo "$@") >"$log" 2>&1
    code=$?
    secs=$(( $(date +%s) - t0 ))
    note=""
    if grep -q '^error: could not compile' "$log"; then note="compile error"
    else
        fails=$(awk '/^ +Running /{cur=$2} /^test result: FAILED/{printf "%s ", cur}' "$log")
        if [ -n "$fails" ]; then note="failed: $fails"; elif [ $code -ne 0 ]; then note="exit $code"; fi
    fi
    # A filtered run that matched no test is not a pass (REQUIRE_TESTS=1 in test mode).
    if [ $code -eq 0 ] && [ -n "${REQUIRE_TESTS:-}" ]; then
        ran=$(awk '/^test result:/{for(i=1;i<=NF;i++) if($i ~ /^(passed|failed);?$/) n+=$(i-1)} END{print n+0}' "$log")
        if [ "$ran" -eq 0 ]; then code=1; note="no test ran - filter matched nothing"; fi
    fi
    if [ $code -eq 0 ]; then st=ok; else st=FAIL; BAD=$((BAD + 1)); fi
    printf '     %s in %ss %s\n' "$st" "$secs" "$note"
    SUMMARY="$SUMMARY$(printf '  %-4s %5ss  %s  %s' "$st" "$secs" "$name" "$note")\n"
}

summary() {
    printf '\n== gate summary (%s) - logs in %s\n' "$TIER" "$LOG_DIR"
    printf '%b' "$SUMMARY"
    printf '  wall %s s, %s step(s), %s failed\n' "$(( $(date +%s) - START ))" "$N" "$BAD"
    [ $BAD -eq 0 ] && exit 0 || exit 1
}

binaries() { [ -d "$ROOT/crates/$1/tests" ] && for f in "$ROOT/crates/$1/tests"/*.rs; do [ -f "$f" ] && basename "$f" .rs; done; }

# run_suites <skip_slow 0|1> <crate dirs...>
run_suites() {
    skip=$1; shift
    for d in "$@"; do
        if [ "$d" = ui ]; then step "ui suites (cwd crates/ui)" "$(cwd_of ui)" test -j $J -p camcraft-ui --no-fail-fast -- $T; continue; fi
        if [ "$skip" = 0 ]; then step "$d suites" "$ROOT" test -j $J -p "camcraft-$d" --no-fail-fast -- $T; continue; fi
        sel="--lib"; skipped=""
        for b in $(binaries "$d"); do
            if contains "$SLOW_BINARIES" "$d:$b"; then skipped="$skipped $b"; else sel="$sel --test $b"; fi
        done
        nm="$d suites"; [ -n "$skipped" ] && nm="$nm (slow, release tier:$skipped)"
        # shellcheck disable=SC2086
        step "$nm" "$ROOT" test -j $J -p "camcraft-$d" --no-fail-fast $sel -- $T
    done
}

run_clippy() {
    pk=""; for d in $CLIPPY; do pk="$pk -p camcraft-$d"; done
    # shellcheck disable=SC2086
    step "clippy -D warnings" "$ROOT" clippy -j $J $pk --all-targets -- -D warnings
}

case "$TIER" in
test)
    filter="${1:-}"; [ -z "$filter" ] && { echo "usage: gate.sh test <filter> [crate] [--exact] [--ignored]"; exit 2; }
    shift
    case "$filter" in *[!A-Za-z0-9_]*) echo "test filter must be a fn name (letters, digits, _): '$filter'"; exit 2 ;; esac
    REQUIRE_TESTS=1
    crates="$ALL_CRATES"; exact=""; ign=""
    for a in "$@"; do
        case "$a" in --exact) exact=--exact ;; --ignored) ign=1 ;; *) crates="${a#camcraft-}" ;; esac
    done
    for d in $crates; do
        # Only crates that hold a matching #[test] fn name are built and run.
        pat="fn +$( [ -n "$exact" ] && echo "$filter" || echo "[A-Za-z0-9_]*$filter[A-Za-z0-9_]*" ) *[(<]"
        in_src=""; in_tests=""
        grep -rqsE "$pat" "$ROOT/crates/$d/src" && in_src=1
        grep -rqsE "$pat" "$ROOT/crates/$d/tests" && in_tests=1
        [ -z "$in_src$in_tests" ] && continue
        rel=""; tag=""; [ -n "$ign" ] && { rel=--release; tag=" (ignored, release)"; }
        if [ -z "$exact" ]; then
            step "$d $filter$tag" "$(cwd_of "$d")" test $rel -j $J -p "camcraft-$d" -- $T ${ign:+--ignored} "$filter"
        else
            # Integration-test fns are top-level, so --exact matches them; lib unit tests are
            # named module::tests::fn, so --exact <fn> never matches - they take "::<fn>" instead.
            [ -n "$in_tests" ] && step "$d $filter (tests, exact)$tag" "$(cwd_of "$d")" test $rel -j $J -p "camcraft-$d" --tests -- $T ${ign:+--ignored} --exact "$filter"
            if [ -n "$in_src" ]; then
                # Resolve the full lib test path(s) ending in ::<fn>, then run them --exact.
                names=$(cd "$(cwd_of "$d")" && cargo test $rel -q -j $J -p "camcraft-$d" --lib -- ${ign:+--ignored} --list --format terse 2>/dev/null \
                    | sed -n "s/^\(\(.*::\)\{0,1\}$filter\): test\$/\1/p")
                [ -n "$names" ] && step "$d $filter (lib, exact)$tag" "$(cwd_of "$d")" test $rel -j $J -p "camcraft-$d" --lib -- $T ${ign:+--ignored} --exact $names
            fi
        fi
    done
    [ $N -eq 0 ] && { echo "no #[test] fn matches '$filter'"; exit 1; }
    summary
    ;;
quick)
    base="${1:-main}"
    mb=$(git -C "$ROOT" merge-base "$base" HEAD) || { echo "no merge base with '$base'"; exit 1; }
    changed=$( { git -C "$ROOT" diff --name-only "$mb"; git -C "$ROOT" ls-files --others --exclude-standard; } | sort -u)
    touched=""; widen=""
    for f in $changed; do
        case "$f" in
            Cargo.toml|Cargo.lock|.cargo/*|rust-toolchain*) widen=1 ;;
            crates/*/*)
                d=$(echo "$f" | cut -d/ -f2)
                contains "$SHARED" "$d" && widen=1
                contains "$touched" "$d" || touched="$touched $d" ;;
        esac
    done
    if [ -n "$widen" ]; then echo "quick: a shared crate or the manifest changed - running batch"; TIER=batch
    else
        [ -z "$touched" ] && { echo "quick: no crate touched - nothing to test"; exit 0; }
        echo "quick: suites of the touched crates:$touched"
        ordered=""; for d in $ALL_CRATES; do contains "$touched" "$d" && ordered="$ordered $d"; done
        # shellcheck disable=SC2086
        run_suites 0 $ordered
        summary
    fi
    ;;
esac

if [ "$TIER" = batch ]; then
    echo "batch: debug suites minus SLOW_BINARIES ($SLOW_BINARIES), ui, clippy"
    # shellcheck disable=SC2086
    run_suites 1 $BATCH_CRATES
    run_suites 0 ui
    run_clippy
    summary
fi
[ "$TIER" = release ] || { echo "unknown tier '$TIER' (quick | batch | release | test)"; exit 2; }
echo "release: every debug suite, ui, clippy, then the --ignored heavy lanes in release and the ui snapshot lane"
for d in $ALL_CRATES; do [ "$d" = ui ] || run_suites 0 "$d"; done
run_suites 0 ui
run_clippy
for d in motion app; do step "$d heavy lane (release --ignored)" "$ROOT" test --release -j $J -p "camcraft-$d" --no-fail-fast -- $T --ignored; done
# The ui pixel snapshot goldens (#255): #[ignore]d out of quick/batch, run here only, debug like the goldens
# were made, CWD crates/ui so they resolve tests/snapshots/. Regenerate at the cut with UPDATE_SNAPSHOTS=1.
step "ui snapshot lane (debug --ignored, cwd crates/ui)" "$(cwd_of ui)" test -j $J -p camcraft-ui --lib --no-fail-fast -- $T --ignored snapshot
summary
