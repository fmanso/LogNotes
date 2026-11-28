#!/usr/bin/env bash
set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
fail=0
ok() { printf 'ok   %s\n' "$1"; }
bad() { printf 'FAIL %s\n' "$1"; fail=1; }

[ -f "$root/Cargo.toml" ] && ok "repo root $root" || bad "not a LogNotes checkout: $root"
command -v cargo >/dev/null && ok "cargo on PATH" || bad "cargo missing"
command -v make >/dev/null && ok "make on PATH" || bad "make missing"

if [ -n "${DISPLAY:-}" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; then
    ok "display ${DISPLAY:-$WAYLAND_DISPLAY}"
else
    bad "no DISPLAY or WAYLAND_DISPLAY"
fi

grep -qx 'driver = \[\]' "$root/lognotes-qt/Cargo.toml" &&
    ok "lognotes-qt declares the driver feature" ||
    bad "driver feature missing from lognotes-qt/Cargo.toml"

if command -v systemd-run >/dev/null && systemd-run --user --scope --quiet true >/dev/null 2>&1; then
    ok "systemd user scope available"
else
    bad "systemd user scope unavailable; run the direct cargo command from Launch"
fi

if pgrep -f -- 'lognotes-qt .*--livetest' >/dev/null 2>&1; then
    bad "a livetest is already running; it owns ${TMPDIR:-/tmp}/lognotes-livetest"
else
    ok "no livetest running"
fi

bin="$root/target/debug/lognotes-qt"
if [ -x "$bin" ]; then
    missing="$(ldd "$bin" 2>/dev/null | grep 'not found' || true)"
    [ -z "$missing" ] && ok "debug binary resolves its libraries" || bad "unresolved libraries: $missing"
else
    ok "no debug binary yet; verify.sh builds it"
fi

exit "$fail"
