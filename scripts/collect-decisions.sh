#!/usr/bin/env bash
# Gather docs/decisions/*.md into RECOMMENDED.md at a release cut - thin wrapper over
# `cargo xtask decisions` (xtask/, #748). Usage: scripts/collect-decisions.sh 0.14.0
set -eu
version="${1:?usage: collect-decisions.sh <version>}"
cd "$(dirname "$0")/.."
exec cargo xtask decisions --version "$version"
