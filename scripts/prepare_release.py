#!/usr/bin/env python3
"""Stage the next plugin release from Conventional Commits and emit gh-ship's artifact."""

import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

root = Path(__file__).resolve().parents[1]
manifest_path = root / "plugin.toml"
text = manifest_path.read_text()
plugin = tomllib.loads(text)["plugin"]
plugin_id = plugin["id"]
tags = subprocess.check_output(["git", "tag", "--merged", "HEAD", "--list", f"{plugin_id}/v*", "--sort=-version:refname"], cwd=root, text=True).splitlines()
last_tag = tags[0] if tags else None
commits = subprocess.check_output(["git", "log", "--format=%B%x00", f"{last_tag}..HEAD" if last_tag else "HEAD"], cwd=root).decode().split("\0")
commits = [message.strip() for message in commits if message.strip() and not message.startswith("chore(release):")]
breaking = any(re.match(r"^\w+(?:\([^)]+\))?!:", message) or "BREAKING CHANGE:" in message for message in commits)
feature = any(re.match(r"^feat(?:\([^)]+\))?:", message) for message in commits)
fix = any(re.match(r"^(?:fix|perf)(?:\([^)]+\))?:", message) for message in commits)
changed = bool(commits) if last_tag is None else breaking or feature or fix
artifact = {"schemaVersion": 1, "changed": bool(changed)}
if changed:
    if last_tag:
        match = re.fullmatch(rf"{re.escape(plugin_id)}/v(\d+)\.(\d+)\.(\d+)", last_tag)
        if not match:
            raise SystemExit(f"unsupported plugin release tag: {last_tag}")
        major, minor, patch = map(int, match.groups())
        if breaking and major:
            major, minor, patch = major + 1, 0, 0
        elif breaking or feature:
            minor, patch = minor + 1, 0
        else:
            patch += 1
        version = f"{major}.{minor}.{patch}"
    else:
        version = plugin["version"]
    tag = f"{plugin_id}/v{version}"
    manifest_path.write_text(re.sub(r'(?m)^version = "[^"]+"', f'version = "{version}"', text, count=1))
    notes = "\n".join(f"- {message.splitlines()[0]}" for message in commits) + "\n"
    artifact.update(version=version, tag=tag, release={"notes": notes})
(root / "ship.release.json").write_text(json.dumps(artifact, indent=2) + "\n")
if output := os.environ.get("GITHUB_OUTPUT"):
    with open(output, "a") as stream:
        stream.write(f"changed={str(bool(changed)).lower()}\n")
        if changed:
            stream.write(f"version={version}\n")
print(json.dumps(artifact, indent=2))
