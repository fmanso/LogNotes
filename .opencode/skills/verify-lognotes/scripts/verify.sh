#!/usr/bin/env bash
set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
stamp="$(date +%Y%m%d-%H%M%S)"
out="${1:-${TMPDIR:-/tmp}/lognotes-verify/$stamp}"
mkdir -p "$out/shots" || exit 2

cd "$root" || exit 2
make livetest 2>&1 | tee "$out/livetest.log"
status="${PIPESTATUS[0]}"

shots="${TMPDIR:-/tmp}/lognotes-livetest/shots"
for png in "$shots"/*.png; do
    [ -e "$png" ] && cp "$png" "$out/shots/"
done

passed="$(grep -c '^PASS ' "$out/livetest.log" || true)"
failed="$(grep -c '^FAIL ' "$out/livetest.log" || true)"
{
    printf 'status: %s\n' "$status"
    printf 'passed: %s\n' "$passed"
    printf 'failed: %s\n' "$failed"
    printf 'log: %s\n' "$out/livetest.log"
    printf 'screenshots: %s\n' "$out/shots"
} | tee "$out/summary.txt"

[ "$status" -eq 0 ] && [ "$failed" -eq 0 ]
