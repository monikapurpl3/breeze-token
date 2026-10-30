#!/bin/sh
# What aspic serves under /breeze-token/: the runners, this version's
# binaries (from packaging/build.sh), and SHA256SUMS over all of them.
#
#   sh stage.sh OUT
#
# Refuses a dirty tree, so what is served is what is committed.
set -eu

[ "$#" -eq 1 ] || { echo "usage: sh stage.sh OUT" >&2; exit 2; }
REPO=$(cd "$(dirname "$0")" && pwd)
OUT=$1
cd "$REPO"

if [ -n "$(git status --porcelain)" ]; then
  echo "!! the tree is dirty; commit first" >&2
  exit 1
fi
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
# Windows PowerShell 5.1 reads a script without a BOM as ANSI.
if LC_ALL=C grep -q '[^[:print:][:space:]]' run.ps1; then
  echo "!! run.ps1 is not plain ASCII" >&2
  exit 1
fi
for label in linux-amd64 linux-arm64 linux-armv7 linux-armv6 linux-riscv64 macos-arm64 macos-x86_64 windows-x86_64.exe; do
  [ -f "packaging/out/breeze-token-$VERSION-$label" ] || { echo "!! no packaging/out/breeze-token-$VERSION-$label: run packaging/build.sh" >&2; exit 1; }
done
# The binaries must be this commit's: each says its version.
case "$(packaging/out/breeze-token-"$VERSION"-windows-x86_64.exe --version 2>/dev/null || true)" in
  "breeze-token $VERSION") ;;
  *) [ "$(uname -o 2>/dev/null)" != Msys ] || { echo "!! the Windows binary is not $VERSION" >&2; exit 1; } ;;
esac

mkdir -p "$OUT"
rm -f "$OUT"/breeze-token-* "$OUT"/SHA256SUMS
cp run.sh run.ps1 "$OUT"/
cp packaging/out/breeze-token-"$VERSION"-* "$OUT"/
(cd "$OUT" && sha256sum run.sh run.ps1 breeze-token-"$VERSION"-* | sed 's/ \*/  /' > SHA256SUMS)
echo "staged breeze-token $VERSION ($(git rev-parse --short HEAD)) in $OUT"
ls -l "$OUT"
