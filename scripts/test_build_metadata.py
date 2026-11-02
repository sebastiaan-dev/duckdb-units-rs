#!/usr/bin/env python3
"""E2E regression: rebuilding must refresh cached revision/override metadata.

Run after `make configure`, using configure/venv/bin/python3. Each load runs in
its own process so a rebuild never overwrites a library mapped by the test runner.
"""
import os
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]


class BuildMetadataTest(unittest.TestCase):
    def command(self, *args):
        env = os.environ.copy()
        env.pop("EXTENSION_VERSION", None)
        result = subprocess.run(
            args, cwd=ROOT, env=env, text=True, capture_output=True, check=False
        )
        self.assertEqual(
            result.returncode, 0, f"{args!r}\n{result.stdout}\n{result.stderr}"
        )
        return result.stdout.strip()

    def loaded_version(self):
        return self.command(
            sys.executable,
            "-c",
            "import duckdb, sys; "
            "db=duckdb.connect(config={'allow_unsigned_extensions': True}); "
            "path=sys.argv[1].replace(\"'\", \"''\"); "
            "db.execute(\"LOAD '\"+path+\"'\"); "
            "print(db.execute(\"SELECT extension_version FROM duckdb_extensions() "
            "WHERE extension_name='units'\").fetchone()[0])",
            str(ROOT / "build/release/units.duckdb_extension"),
        )

    def test_refreshes_revision_and_explicit_override(self):
        version_file = ROOT / "configure/extension_version.txt"
        previous = version_file.read_bytes()
        revision = self.command("git", "tag", "--points-at", "HEAD")
        if not revision:
            revision = self.command("git", "log", "-1", "--format=%h")
        try:
            version_file.write_text("stale-regression-sentinel")
            self.command("make", "release")
            self.assertEqual(self.loaded_version(), revision)

            self.command("make", "EXTENSION_VERSION=metadata-regression", "release")
            self.assertEqual(self.loaded_version(), "metadata-regression")

            self.command("make", "release")
            self.assertEqual(self.loaded_version(), revision)
        finally:
            version_file.write_bytes(previous)
            self.command("make", "release")


if __name__ == "__main__":
    unittest.main()
