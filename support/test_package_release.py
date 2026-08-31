#!/usr/bin/env python3
"""Regression tests for deterministic release packaging."""

from __future__ import annotations

import pathlib
import subprocess
import sys
import tempfile
import unittest
import zipfile


REPOSITORY = pathlib.Path(__file__).resolve().parent.parent
PACKAGER = REPOSITORY / "support" / "package-release.py"


class PackageReleaseTests(unittest.TestCase):
    def test_archive_includes_cli_assets_below_share(self) -> None:
        with tempfile.TemporaryDirectory(prefix="fujicli-package-test-") as directory:
            root = pathlib.Path(directory)
            binary = root / "fujicli"
            license_file = root / "LICENSE"
            assets = root / "assets"
            completion = assets / "bash-completion" / "completions" / "fujicli"
            manpage = assets / "man" / "man1" / "fujicli.1"
            archive = root / "package.zip"

            binary.write_bytes(b"binary")
            license_file.write_text("license\n", encoding="utf-8")
            completion.parent.mkdir(parents=True)
            completion.write_text("completion\n", encoding="utf-8")
            manpage.parent.mkdir(parents=True)
            manpage.write_text("manpage\n", encoding="utf-8")

            result = subprocess.run(
                [
                    sys.executable,
                    PACKAGER,
                    "--binary",
                    binary,
                    "--license",
                    license_file,
                    "--assets-root",
                    assets,
                    "--package",
                    "fujicli-test",
                    "--output",
                    archive,
                    "--epoch",
                    "0",
                ],
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            with zipfile.ZipFile(archive) as packaged:
                self.assertEqual(
                    packaged.namelist(),
                    [
                        "fujicli-test/LICENSE",
                        "fujicli-test/fujicli",
                        "fujicli-test/share/bash-completion/completions/fujicli",
                        "fujicli-test/share/man/man1/fujicli.1",
                    ],
                )


if __name__ == "__main__":
    unittest.main()
