"""Boundary and report-validation checks for the source coverage gate."""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CHECKER = ROOT / "scripts" / "check_source_coverage.py"
SOURCE = ROOT / "sources" / "codex" / "src" / "lib.rs"


class SourceCoverageGateTests(unittest.TestCase):
    def test_strict_threshold_and_missing_coverage(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "lcov.info"
            for hits, succeeds in [([1, 1, 1, 1, 0], False), ([1, 1, 1, 1, 1], True), ([], False)]:
                report.write_text(
                    f"SF:{SOURCE}\n"
                    + "".join(f"DA:{line},{hit}\n" for line, hit in enumerate(hits, 32))
                    + "end_of_record\n"
                )
                result = subprocess.run(
                    [sys.executable, str(CHECKER), "codex", str(report)],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual(result.returncode == 0, succeeds, result.stdout + result.stderr)

    def test_excludes_test_module_and_other_sources(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "lcov.info"
            test_line = next(
                n for n, line in enumerate(SOURCE.read_text().splitlines(), 1)
                if line.strip() == "mod tests {"
            ) + 1
            report.write_text(
                f"SF:{SOURCE}\nDA:32,1\nDA:{test_line},0\nend_of_record\n"
                f"SF:{ROOT / 'sources/chatgpt/src/lib.rs'}\nDA:25,0\nend_of_record\n"
            )
            result = subprocess.run(
                [sys.executable, str(CHECKER), "codex", str(report)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("1/1 production lines", result.stdout)


if __name__ == "__main__":
    unittest.main()
