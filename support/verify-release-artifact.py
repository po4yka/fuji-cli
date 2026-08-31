#!/usr/bin/env python3
"""Verify the production command boundary and packaged CLI assets."""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import tempfile
import zipfile


REVERSE_INVOCATIONS = (
    ("device", "reverse", "info"),
    ("device", "r", "info"),
    ("d", "reverse", "info"),
    ("d", "r", "info"),
)


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--binary", type=pathlib.Path)
    source.add_argument("--archive", type=pathlib.Path)
    parser.add_argument("--binary-name", default="fujicli")
    parser.add_argument("--assets-root", type=pathlib.Path)
    return parser.parse_args()


def verify_binary(binary: pathlib.Path) -> None:
    version = subprocess.run(
        [binary, "--version"], check=False, capture_output=True
    )
    if version.returncode != 0 or version.stderr:
        raise SystemExit(f"production binary version smoke failed: {version!r}")

    help_result = subprocess.run(
        [binary, "device", "--help"], check=False, capture_output=True
    )
    if help_result.returncode != 0 or b"reverse" in help_result.stdout.lower():
        raise SystemExit("production device help exposes reverse tooling")

    for invocation in REVERSE_INVOCATIONS:
        result = subprocess.run([binary, *invocation], check=False, capture_output=True)
        if result.returncode != 2:
            raise SystemExit(
                f"production binary accepted {invocation!r}: status={result.returncode}"
            )
        if result.stdout:
            raise SystemExit(
                f"rejected reverse invocation wrote stdout for {invocation!r}"
            )
        if b"unrecognized subcommand" not in result.stderr:
            raise SystemExit(
                f"reverse invocation did not fail at parser boundary: {invocation!r}"
            )


def verify_archive(
    archive_path: pathlib.Path,
    binary_name: str,
    assets_root: pathlib.Path,
) -> None:
    with zipfile.ZipFile(archive_path) as archive:
        names = archive.namelist()
        roots = {pathlib.PurePosixPath(name).parts[0] for name in names}
        if len(roots) != 1:
            raise SystemExit(f"release archive must have one package root: {names!r}")
        root = roots.pop()
        expected = {f"{root}/LICENSE", f"{root}/{binary_name}"}
        expected.update(
            f"{root}/share/{asset.relative_to(assets_root).as_posix()}"
            for asset in assets_root.rglob("*")
            if asset.is_file()
        )
        if set(names) != expected:
            raise SystemExit(
                f"release archive entries differ from production allowlist: {names!r}"
            )
        for asset in assets_root.rglob("*"):
            if asset.is_file():
                archived = f"{root}/share/{asset.relative_to(assets_root).as_posix()}"
                if archive.read(archived) != asset.read_bytes():
                    raise SystemExit(
                        f"release archive asset differs from source: {archived}"
                    )
        with tempfile.TemporaryDirectory(prefix="fujicli-release-verify-") as directory:
            archive.extractall(directory)
            binary = pathlib.Path(directory, root, binary_name)
            binary.chmod(binary.stat().st_mode | 0o111)
            verify_binary(binary)


def main() -> None:
    options = arguments()
    if options.binary is not None:
        verify_binary(options.binary)
    else:
        if options.assets_root is None:
            raise SystemExit("--assets-root is required with --archive")
        verify_archive(options.archive, options.binary_name, options.assets_root)


if __name__ == "__main__":
    main()
