#!/bin/sh
# run.sh -- run breeze-token once, and leave nothing behind.
#
#   curl -fsSL https://aspic.salataputarica.hr.eu.org/breeze-token/run.sh | sh
#   curl -fsSL https://aspic.salataputarica.hr.eu.org/breeze-token/run.sh | sh -s -- --write config.json
#
# What it does, and nothing more:
#   * downloads this machine's breeze-token into a new temporary directory;
#   * checks it against SHA256SUMS, and runs nothing that does not match;
#   * runs it once, on this terminal, with the arguments given after `--`;
#   * deletes the directory again, whatever happened.
#
# Nothing is installed and nothing outside that directory is touched, except
# what breeze-token itself is told to write (--write FILE).
#
# Copyright 2026 Monika. AGPL-3.0-or-later; see LICENSE.

# Everything is in functions, and `main` runs on the last line: piped into sh,
# the whole script has been read before anything runs.

BASE="${BREEZE_TOKEN_BASE:-https://aspic.salataputarica.hr.eu.org/breeze-token}"

die() {
  printf 'breeze-token: %s\n' "$1" >&2
  exit 1
}

# This machine, as the binaries are labelled.
label() {
  os=$(uname -s 2>/dev/null)
  arch=$(uname -m 2>/dev/null)
  case "$os" in
    Linux)
      if [ "$(uname -o 2>/dev/null)" = Android ]; then
        die "there is no build for Android (Termux) yet: run it on a computer on the same network"
      fi
      case "$arch" in
        x86_64 | amd64) echo linux-amd64 ;;
        aarch64 | arm64) echo linux-arm64 ;;
        armv7* | armv8l) echo linux-armv7 ;;
        armv6*) echo linux-armv6 ;;
        riscv64) echo linux-riscv64 ;;
        *) die "there is no build for Linux on $arch" ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        arm64) echo macos-arm64 ;;
        x86_64) echo macos-x86_64 ;;
        *) die "there is no build for macOS on $arch" ;;
      esac
      ;;
    MINGW* | MSYS* | CYGWIN*)
      die "on Windows, in PowerShell: irm $BASE/run.ps1 | iex"
      ;;
    *) die "there is no build for ${os:-this system}" ;;
  esac
}

download() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$2" "$1"
  else
    die "downloading needs curl or wget"
  fi
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d ' ' -f 1
  elif command -v sha256 >/dev/null 2>&1; then
    sha256 -q "$1"
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$1" | sed 's/.*= *//'
  else
    die "nothing here computes a SHA-256 (sha256sum, shasum, sha256 or openssl)"
  fi
}

main() {
  me=$(label) || exit 1
  dir=$(mktemp -d 2>/dev/null || mktemp -d -t breeze-token) || die "cannot make a temporary directory"
  trap 'rm -rf "$dir"' EXIT
  trap 'rm -rf "$dir"; exit 130' INT TERM HUP

  download "$BASE/SHA256SUMS" "$dir/SHA256SUMS" || die "cannot download $BASE/SHA256SUMS"
  # "HASH  breeze-token-VERSION-LABEL": this machine's line. One version is
  # served at a time.
  line=$(awk -v me="$me" '$2 ~ ("^breeze-token-[0-9][0-9.]*-" me "$")' "$dir/SHA256SUMS" | tail -n 1)
  [ -n "$line" ] || die "$BASE has no breeze-token for $me"
  want=${line%% *}
  name=${line##* }
  download "$BASE/$name" "$dir/breeze-token" || die "cannot download $BASE/$name"
  [ "$(sha256 "$dir/breeze-token")" = "$want" ] || die "$name does not match its SHA-256 in SHA256SUMS; nothing was run"
  chmod 700 "$dir/breeze-token"
  version=${name#breeze-token-}
  printf 'breeze-token %s for %s, checked against its SHA-256. It runs once and is deleted afterwards.\n' "${version%-"$me"}" "$me"

  # Standard input is this script, piped: the terminal is where the
  # questions are answered, if there is one.
  if (: </dev/tty) 2>/dev/null; then
    "$dir/breeze-token" "$@" </dev/tty
  else
    "$dir/breeze-token" "$@"
  fi
}

main "$@"
