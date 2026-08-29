#!/usr/bin/env bash
set -euo pipefail

readonly LYCHEE_VERSION="0.24.2"
readonly LYCHEE_SHA256="1f4e0ef7f6554a6ed33dd7ac144fb2e1bbed98598e7af973042fc5cd43951c9a"

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

readonly archive="$work_dir/lychee.tar.gz"
readonly archive_root="lychee-x86_64-unknown-linux-gnu"
curl --fail --location --proto '=https' --tlsv1.2 \
  --output "$archive" \
  "https://github.com/lycheeverse/lychee/releases/download/lychee-v${LYCHEE_VERSION}/lychee-x86_64-unknown-linux-gnu.tar.gz"
printf '%s  %s\n' "$LYCHEE_SHA256" "$archive" | sha256sum --check --strict

mkdir -p "$install_dir"
tar -xzf "$archive" -C "$work_dir" "$archive_root/lychee"
install -m 0755 "$work_dir/$archive_root/lychee" "$install_dir/lychee"
"$install_dir/lychee" --version
