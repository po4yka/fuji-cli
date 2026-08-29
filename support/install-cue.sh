#!/usr/bin/env bash
set -euo pipefail

readonly CUE_VERSION="0.16.1"
readonly CUE_SHA256="5d644c1305a2b86504c8dcd2ec829cf5b4999efc2cf51ee375624e0455f774ae"

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <install-directory>" >&2
  exit 2
fi

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
  echo "this installer supports only Linux x86_64" >&2
  exit 1
fi

readonly install_dir="$1"
work_dir="$(mktemp -d)"
readonly work_dir
trap 'rm -rf "$work_dir"' EXIT

readonly archive="$work_dir/cue.tar.gz"
curl --fail --location --proto '=https' --tlsv1.2 \
  --output "$archive" \
  "https://github.com/cue-lang/cue/releases/download/v${CUE_VERSION}/cue_v${CUE_VERSION}_linux_amd64.tar.gz"
printf '%s  %s\n' "$CUE_SHA256" "$archive" | sha256sum --check --strict

mkdir -p "$install_dir"
tar -xzf "$archive" -C "$work_dir" cue
install -m 0755 "$work_dir/cue" "$install_dir/cue"
"$install_dir/cue" version
