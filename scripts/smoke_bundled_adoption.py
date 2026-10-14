#!/usr/bin/env python3
"""Upgrade a palace from Core-bundled sources to independently installed modules."""

import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
from urllib.error import HTTPError

from smoke_install import ROOT, request
from smoke_adoption import mine, ready


def start(binary, root, assets, port):
    env = {key: value for key, value in os.environ.items() if not key.startswith("MEMCASTLE_")}
    env.update({"HOME": str(root), "XDG_DATA_HOME": str(root / "data"),
                "XDG_CONFIG_HOME": str(root / "config"), "XDG_STATE_HOME": str(root / "state"),
                "MEMCASTLE_ASSETS_DIR": str(assets),
                "MEMCASTLE_MINING_BUNDLED_DIR": str(assets / "sources")})
    process = subprocess.Popen((str(binary), "--palace", str(root / "palace"), "serve",
                                "--bind", "127.0.0.1", "--port", str(port)),
                               cwd=root, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    ready(process, f"http://127.0.0.1:{port}")
    return process


def stop(process):
    process.terminate()
    try:
        process.communicate(timeout=20)
    except subprocess.TimeoutExpired:
        process.kill()
        process.communicate()


def handoff(binary, archive, name):
    with tempfile.TemporaryDirectory(prefix=f"memcastle-bundled-{name}-") as scratch:
        root = Path(scratch)
        old_assets = root / "old-assets"
        old_bundle = old_assets / "sources" / name
        old_bundle.mkdir(parents=True)
        module = ROOT / "modules" / name
        shutil.copyfile(module / "memcastle-source.toml", old_bundle / "memcastle-source.toml")
        shutil.copyfile(module / "dist/source.wasm", old_bundle / "source.wasm")
        new_assets = root / "new-assets"
        new_assets.mkdir()
        (new_assets / "sources").mkdir()
        if name == "codex":
            sessions = root / "sessions" / "2026" / "10" / "08"
            sessions.mkdir(parents=True)
            fixture = module / "fixtures/codex-rollouts/tree/2026/10/08/rollout-2026-10-08T10-00-00-session-a.jsonl"
            shutil.copyfile(fixture, sessions / fixture.name)
            locator = root / "sessions"
        else:
            locator = root / "conversations.json"
            shutil.copyfile(module / "fixtures/export/conversations.json", locator)
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        base = f"http://127.0.0.1:{port}"
        process = start(binary, root, old_assets, port)
        try:
            bundled = request(base, f"/api/source-packages/{name}")
            assert bundled["origin"] == "bundled", bundled
            request(base, f"/api/source-packages/{name}/enable", method="POST")
            request(base, f"/api/miners/{name}", method="PUT", data=json.dumps({
                "source": name, "locator": str(locator),
            }).encode(), headers={"Content-Type": "application/json"})
            assert mine(base, name, locator)["created"] >= 1
            try:
                request(base, "/api/plugins/preview", method="POST", data=archive)
            except HTTPError as error:
                assert error.code == 409, json.load(error)
            else:
                raise AssertionError("plugin claimed a still-shipped Core source")
            before = next(item for item in request(base, "/api/sources")["sources"] if item["source"] == name)
        finally:
            stop(process)
        # A Core upgrade removes the old bundle while leaving its enabled-state row and mined history in the palace.
        process = start(binary, root, new_assets, port)
        try:
            orphan = request(base, f"/api/source-packages/{name}")
            assert orphan["origin"] == "bundled", orphan
            try:
                preview = request(base, "/api/plugins/preview", method="POST", data=archive)
            except HTTPError as error:
                raise AssertionError(f"orphaned {name} bundle refused: {json.load(error)}") from error
            assert preview["adoptions"] == [name], preview
            consents = {item["source"]: item["digest"] for item in preview["source_consents"]}
            request(base, "/api/plugins", method="POST", data=archive, headers={
                "X-MemCastle-Consents": json.dumps(consents),
                "X-MemCastle-Adopt-Sources": json.dumps([name]),
            })
            record = request(base, f"/api/source-packages/{name}")
            assert record["origin"] == "plugin" and record["state"] == "enabled", record
            after = next(item for item in request(base, "/api/sources")["sources"] if item["source"] == name)
            assert (before["cursor"], before["documents"]) == (after["cursor"], after["documents"]), (before, after)
            assert request(base, f"/api/miners/{name}")["source"] == name
            assert mine(base, name, locator)["created"] == 0
        finally:
            stop(process)


if __name__ == "__main__":
    executable = Path(os.environ["MEMCASTLE_BIN"]).resolve()
    plugin = (ROOT / "dist/openai-0.1.0.tar.gz").read_bytes()
    for source in ("codex", "chatgpt"):
        handoff(executable, plugin, source)
    print("Core-bundled Codex and ChatGPT state, miners and documents survived the upgrade")
