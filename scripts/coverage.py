#!/usr/bin/env python3
"""Measure native Rust and Bun tests for modules with executable test suites."""

import json
import os
from pathlib import Path
import subprocess
import tomllib


root = Path(__file__).resolve().parents[1]
manifest = tomllib.loads((root / "plugin.toml").read_text())
reports = []

for module in manifest.get("modules", []):
    directory = root / "modules" / module["id"]
    if module["type"] == "source":
        if not (directory / "Cargo.toml").is_file():
            continue
        report = directory / "coverage" / "lcov.info"
        report.parent.mkdir(exist_ok=True)
        subprocess.run(
            ("cargo", "llvm-cov", "--locked", "--lcov", "--output-path", str(report)),
            cwd=directory,
            check=True,
        )
    elif module["type"] == "integration":
        package = directory / "package.json"
        if not package.is_file() or "test:cover" not in json.loads(package.read_text()).get("scripts", {}):
            continue
        subprocess.run(("bun", "install", "--frozen-lockfile"), cwd=directory, check=True)
        subprocess.run(("bun", "run", "test:cover"), cwd=directory, check=True)
        report = directory / "coverage" / "lcov.info"
    else:
        continue
    if not report.is_file() or not report.stat().st_size:
        raise SystemExit(f"coverage did not produce {report}")
    reports.append(report.relative_to(root).as_posix())

if output := os.environ.get("GITHUB_OUTPUT"):
    with open(output, "a") as stream:
        stream.write(f"files={','.join(reports)}\n")
print(f"Coverage reports: {', '.join(reports) if reports else 'no instrumented modules'}")
