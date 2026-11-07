#!/usr/bin/env bash
# Tests dev/macos-sdk.sh with tiny binaries whose recorded SDK is rewritten by `vtool`: an SDK
# of 26 or newer passes, an older one is refused, and a universal binary passes only when
# every slice does. The release workflow's macOS packaging job runs it before building.
# Usage: bash dev/test-macos-sdk.sh   (macOS only: needs clang, lipo and vtool)
set -euo pipefail

[[ "$(uname -s)" == "Darwin" ]] || { echo "test-macos-sdk.sh runs on macOS only" >&2; exit 1; }
script="$(cd "$(dirname "$0")" && pwd)/macos-sdk.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

echo 'int main(void) { return 0; }' > "$work/main.c"
# One slice per architecture, each with the given SDK version recorded.
slice() { # <arch> <sdk> <out>
  clang -arch "$1" -mmacosx-version-min=11.0 "$work/main.c" -o "$work/raw-$1"
  vtool -set-build-version macos 11.0 "$2" -replace -output "$3" "$work/raw-$1"
}

slice arm64 26.0 "$work/arm-26"
slice arm64 15.5 "$work/arm-15"
slice x86_64 27.2 "$work/x86-27"
slice x86_64 15.5 "$work/x86-15"
lipo -create "$work/arm-26" "$work/x86-27" -output "$work/universal-new"
lipo -create "$work/arm-26" "$work/x86-15" -output "$work/universal-mixed"

expect_pass() {
  bash "$script" "$1" >/dev/null 2>&1 || fail "$(basename "$1") was refused"
  echo "ok: $(basename "$1") passes"
}
expect_refusal() {
  if out="$(bash "$script" "$1" 2>&1)"; then fail "$(basename "$1") passed"; fi
  [[ "$out" == *"SDK"* ]] || fail "$(basename "$1") refused without naming the SDK: $out"
  echo "ok: $(basename "$1") refused"
}

expect_pass "$work/arm-26"
expect_pass "$work/x86-27"
expect_pass "$work/universal-new"
expect_refusal "$work/arm-15"
expect_refusal "$work/x86-15"
expect_refusal "$work/universal-mixed"
echo "all macos-sdk checks passed"
