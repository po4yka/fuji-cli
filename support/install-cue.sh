#!/usr/bin/env bash
set -euo pipefail

readonly CUE_VERSION="0.16.1"

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <install-directory>" >&2
  exit 2
fi

case "$(uname -s):$(uname -m)" in
  Linux:x86_64)
    platform="linux_amd64"
    checksum="5d644c1305a2b86504c8dcd2ec829cf5b4999efc2cf51ee375624e0455f774ae"
    archive_kind="tar.gz"
    executable="cue"
    ;;
  Linux:aarch64 | Linux:arm64)
    platform="linux_arm64"
    checksum="3cc715a9e969f87b93c4fa34cfaef5388b93e96efa20b248e8ad6826abd25a83"
    archive_kind="tar.gz"
    executable="cue"
    ;;
  Darwin:x86_64)
    platform="darwin_amd64"
    checksum="97b0d78e4c5ee49ff72145fd6ef4f4bab0bb332d55f29660de3fec2af5ec96a9"
    archive_kind="tar.gz"
    executable="cue"
    ;;
  Darwin:arm64)
    platform="darwin_arm64"
    checksum="a72b0cddb377c52d1b003bed9a335d893b70cd75a182cd5e3fee8bae30ddb6d6"
    archive_kind="tar.gz"
    executable="cue"
    ;;
  MINGW*:x86_64 | MSYS*:x86_64 | CYGWIN*:x86_64)
    platform="windows_amd64"
    checksum="2f24123f458229fcf283db534bd86692ad1074da806defee0f0cc62976c0397c"
    archive_kind="zip"
    executable="cue.exe"
    ;;
  MINGW*:arm64 | MSYS*:arm64 | CYGWIN*:arm64)
    platform="windows_arm64"
    checksum="e0c15ce53f73e8609b0e8ce6507298f3474b334ac5eb0c826c9497a811fd0cce"
    archive_kind="zip"
    executable="cue.exe"
    ;;
  *)
    echo "unsupported CUE host: $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac
readonly platform checksum archive_kind executable

readonly install_dir="$1"
work_dir="$(mktemp -d)"
readonly work_dir
trap 'rm -rf "$work_dir"' EXIT

readonly archive="$work_dir/cue.$archive_kind"
curl --fail --location --proto '=https' --tlsv1.2 \
  --output "$archive" \
  "https://github.com/cue-lang/cue/releases/download/v${CUE_VERSION}/cue_v${CUE_VERSION}_${platform}.${archive_kind}"
if command -v sha256sum >/dev/null 2>&1; then
  actual_checksum="$(sha256sum "$archive" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  actual_checksum="$(shasum -a 256 "$archive" | awk '{print $1}')"
else
  echo "no SHA-256 checksum utility found" >&2
  exit 1
fi
readonly actual_checksum
if [[ "$actual_checksum" != "$checksum" ]]; then
  echo "CUE archive checksum mismatch: expected $checksum, got $actual_checksum" >&2
  exit 1
fi

mkdir -p "$install_dir"
if [[ "$archive_kind" == "zip" ]]; then
  unzip -q "$archive" -d "$work_dir"
else
  tar -xzf "$archive" -C "$work_dir" "$executable"
fi
install -m 0755 "$work_dir/$executable" "$install_dir/$executable"
"$install_dir/$executable" version
