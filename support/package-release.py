#!/usr/bin/env python3
"""Create a deterministic cross-platform fujicli release archive."""

from __future__ import annotations

import argparse
import datetime as dt
import pathlib
import zipfile


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=pathlib.Path, required=True)
    parser.add_argument("--license", type=pathlib.Path, required=True)
    parser.add_argument("--package", required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--epoch", type=int, required=True)
    return parser.parse_args()


def archive_timestamp(epoch: int) -> tuple[int, int, int, int, int, int]:
    timestamp = dt.datetime.fromtimestamp(epoch, tz=dt.UTC)
    timestamp = max(timestamp, dt.datetime(1980, 1, 1, tzinfo=dt.UTC))
    return (
        timestamp.year,
        timestamp.month,
        timestamp.day,
        timestamp.hour,
        timestamp.minute,
        timestamp.second,
    )


def add_file(
    archive: zipfile.ZipFile,
    source: pathlib.Path,
    destination: str,
    mode: int,
    timestamp: tuple[int, int, int, int, int, int],
) -> None:
    entry = zipfile.ZipInfo(destination, timestamp)
    entry.compress_type = zipfile.ZIP_DEFLATED
    entry.create_system = 3
    entry.external_attr = (0o100000 | mode) << 16
    archive.writestr(entry, source.read_bytes())


def main() -> None:
    options = arguments()
    options.output.parent.mkdir(parents=True, exist_ok=True)
    timestamp = archive_timestamp(options.epoch)
    with zipfile.ZipFile(options.output, "w") as archive:
        add_file(
            archive,
            options.license,
            f"{options.package}/LICENSE",
            0o644,
            timestamp,
        )
        add_file(
            archive,
            options.binary,
            f"{options.package}/{options.binary.name}",
            0o755,
            timestamp,
        )


if __name__ == "__main__":
    main()
