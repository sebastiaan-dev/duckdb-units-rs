#!/usr/bin/env python3
"""Check generated link commands without requiring a local Emscripten SDK.

Actual WASM builds remain covered by the distribution workflow. The original
failure was reproduced there: emcc could not export the obsolete v1 entrypoint.
"""
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]


class WasmBuildConfigTest(unittest.TestCase):
    def test_all_wasm_profiles_export_c_api_v2(self):
        for platform in ("wasm_mvp", "wasm_eh", "wasm_threads"):
            for profile in ("debug", "release"):
                with self.subTest(platform=platform, profile=profile):
                    result = subprocess.run(
                        ["make", "-n", f"DUCKDB_PLATFORM={platform}", profile],
                        cwd=ROOT, text=True, capture_output=True, check=False,
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    link_commands = [line for line in result.stdout.splitlines()
                                     if line.startswith("emcc ")]
                    self.assertEqual(len(link_commands), 1, result.stdout)
                    self.assertIn('EXPORTED_FUNCTIONS="_units_init_c_api_v2"', link_commands[0])
                    self.assertIn("--package units_wasm", result.stdout)
                    self.assertIn("--abi-type C_STRUCT_UNSTABLE", result.stdout)


if __name__ == "__main__":
    unittest.main()
