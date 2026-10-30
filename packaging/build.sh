#!/bin/sh
# The release binaries, and their SHA256SUMS, into packaging/out/:
#
#   packaging/out/breeze-token-VERSION-LABEL[.exe]
#   packaging/out/SHA256SUMS
#
#   sh packaging/build.sh [LABEL...]        # all of them by default
#
# Linux is static musl, linked by zig (cargo zigbuild); macOS too, with the
# CoreFoundation link stub in packaging/macos/; Windows is a native MSVC
# build, so this runs on Windows (Git Bash) for a full set. The same pins as
# Breeze Core's own builds: zig 0.16.0, from BT_ZIG or PATH.
set -eu

REPO=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO"
OUT=packaging/out
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

ZIG="${BT_ZIG:-zig}"
zig_have=$("$ZIG" version 2>/dev/null || echo none)
if [ "$zig_have" != "0.16.0" ]; then
  echo "!! zig $zig_have at '$ZIG'; this build is pinned to 0.16.0. Run with BT_ZIG=/path/to/zig-0.16.0/zig"
  exit 1
fi
export CARGO_ZIGBUILD_ZIG_PATH="$ZIG"

# label | rust target
TARGETS="
linux-amd64|x86_64-unknown-linux-musl
linux-arm64|aarch64-unknown-linux-musl
linux-armv7|armv7-unknown-linux-musleabihf
linux-armv6|arm-unknown-linux-musleabihf
linux-riscv64|riscv64gc-unknown-linux-musl
macos-arm64|aarch64-apple-darwin
macos-x86_64|x86_64-apple-darwin
windows-x86_64|x86_64-pc-windows-msvc
"

wanted=${*:-$(printf '%s\n' "$TARGETS" | sed -n 's/|.*//p')}
mkdir -p "$OUT"
for label in $wanted; do
  target=$(printf '%s\n' "$TARGETS" | sed -n "s/^$label|//p")
  [ -n "$target" ] || { echo "!! unknown label $label"; exit 1; }
  echo "== $label ($target)"
  exe=""
  case "$target" in
    *windows-msvc)
      cargo build --release --locked --target "$target"
      exe=".exe"
      ;;
    *-apple-darwin)
      # zig has no Apple frameworks, and iana-time-zone (through
      # breeze-store's chrono) needs CoreFoundation: see packaging/macos/.
      fw="$REPO/packaging/macos"
      command -v cygpath >/dev/null 2>&1 && fw=$(cygpath -m "$fw")
      env "CARGO_TARGET_$(echo "$target" | tr 'a-z-' 'A-Z_')_RUSTFLAGS=-C link-arg=-F$fw" \
        cargo zigbuild --release --locked --target "$target"
      ;;
    *)
      cargo zigbuild --release --locked --target "$target"
      ;;
  esac
  cp "target/$target/release/breeze-token$exe" "$OUT/breeze-token-$VERSION-$label$exe"
done

# This version's binaries; the runners pick their own line out of this file.
(cd "$OUT" && sha256sum breeze-token-"$VERSION"-* | sed 's/ \*/  /' > SHA256SUMS)
echo "== $OUT"
ls -l "$OUT"
