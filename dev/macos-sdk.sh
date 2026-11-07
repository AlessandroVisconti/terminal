#!/usr/bin/env bash
# Refuses a macOS binary built against an SDK older than macOS 26, in every slice of a
# universal binary. From macOS 26 the system draws a window with the new design (its corner
# radius among it) only for an app built against that SDK or newer; an older one keeps the old
# look whatever the app asks for, so a release built with an old Xcode would quietly lose it.
# `dev/package-macos.sh` runs it on the app's binary; `dev/test-macos-sdk.sh` tests it.
# Usage: bash dev/macos-sdk.sh <binary>
set -euo pipefail

bin="${1:?usage: macos-sdk.sh <binary>}"
min=26

# One "sdk X.Y" line per slice, from each LC_BUILD_VERSION load command.
sdks="$(otool -l "$bin" | awk '/LC_BUILD_VERSION/ {b=1} b && $1 == "sdk" {print $2; b=0}')"
[[ -n "$sdks" ]] || { echo "no SDK version recorded in $bin" >&2; exit 1; }

status=0
while read -r sdk; do
  if (( ${sdk%%.*} < min )); then
    echo "$bin was built against macOS SDK $sdk; macOS $min or newer is needed for the current window design (build with Xcode $min or newer)" >&2
    status=1
  fi
done <<< "$sdks"
exit "$status"
