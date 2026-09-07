#!/usr/bin/env bash
set -euo pipefail

case "${1:-}" in
  stable) printf '%s\n' stable; exit 0 ;;
  msrv) ;;
  *) echo 'Select a Rust toolchain policy: stable or msrv' >&2; exit 1 ;;
esac

# Bootstrap without Cargo or a TOML package: accept only this repository's
# literal package.rust-version form and fail closed if its shape changes.
declared="$(awk '
  { sub(/\r$/, "") }
  /^[[:space:]]*\[/ {
    package = ($0 ~ /^[[:space:]]*\[package\][[:space:]]*(#.*)?$/)
  }
  package && /^[[:space:]]*rust-version[[:space:]]*=/ {
    sub(/^[^=]*=[[:space:]]*/, "")
    print
  }
' "${2:-Cargo.toml}")"
literal="^[\"']([0-9]+\.[0-9]+(\.[0-9]+)?)[\"'][[:space:]]*(#.*)?$"
if [[ "$declared" == *$'\n'* || ! "$declared" =~ $literal ]]; then
  echo 'Expected one literal numeric [package] rust-version in Cargo.toml' >&2
  exit 1
fi
version="${BASH_REMATCH[1]}"
quoted="${declared:0:1}${version}${declared:0:1}"
if [[ "$declared" != "$quoted"* ]]; then
  echo 'Mismatched quotes in Cargo.toml rust-version' >&2
  exit 1
fi
if [[ "$version" =~ ^[0-9]+\.[0-9]+$ ]]; then
  version="$version.0"
fi
printf '%s\n' "$version"
