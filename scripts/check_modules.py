#!/usr/bin/env python3
"""Format, lint and test only this provider's declared module projects."""

from pathlib import Path
import subprocess
import tomllib


root = Path(__file__).resolve().parents[1]
manifest = tomllib.loads((root / "plugin.toml").read_text())
for module in manifest.get("modules", []):
    directory = root / "modules" / module["id"]
    if module["type"] == "source":
        # The release packager produces and then requires a committed lockfile.
        if not (directory / "Cargo.lock").is_file():
            raise SystemExit(f"generate and commit {directory / 'Cargo.lock'} before checking modules")
        for args in (
            ("cargo", "fmt", "--all", "--", "--check"),
            ("cargo", "clippy", "--locked", "--target", "wasm32-wasip2", "--", "-D", "warnings"),
        ):
            subprocess.run(args, cwd=directory, check=True)
    elif module["type"] == "integration" and (directory / "package.json").is_file():
        for task in ("format:check", "lint", "test", "build"):
            subprocess.run(("npm", "run", task), cwd=directory, check=True)
