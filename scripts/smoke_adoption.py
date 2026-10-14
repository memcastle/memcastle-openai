#!/usr/bin/env python3
"""Verify both historic source identities survive explicit plugin ownership transfer."""

import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
from urllib.error import HTTPError, URLError

from smoke_install import ROOT, request


def ready(process, base):
    for _ in range(120):
        if process.poll() is not None:
            raise RuntimeError(f"daemon exited: {process.stderr.read().decode()}")
        try:
            request(base, "/api/health")
            return
        except (URLError, TimeoutError):
            time.sleep(0.25)
    raise AssertionError("disposable daemon did not become healthy")


def mine(base, source, locator):
    job = request(base, "/api/jobs", method="POST", data=json.dumps({
        "type": "mine", "source": source, "locator": str(locator), "requested_by": "test",
    }).encode(), headers={"Content-Type": "application/json"})
    for _ in range(300):
        result = request(base, f"/api/jobs/{job['id']}")
        if result["status"] in ("completed", "failed", "cancelled"):
            assert result["status"] == "completed", result
            return result["result"]
        time.sleep(0.1)
    raise AssertionError(f"{source} mining did not finish")


def handoff(binary, archive, name):
    module = ROOT / "modules" / name
    subprocess.run((str(binary), "source", "package", str(module), "--no-build"),
                   check=True, capture_output=True)
    legacy = (module / "dist" / f"{name}-0.1.0.tar.gz").read_bytes()
    with tempfile.TemporaryDirectory(prefix=f"memcastle-{name}-adopt-") as scratch:
        root = Path(scratch)
        (root / "assets").mkdir()
        env = {key: value for key, value in os.environ.items() if not key.startswith("MEMCASTLE_")}
        env.update({"HOME": scratch, "XDG_DATA_HOME": str(root / "data"),
                    "XDG_CONFIG_HOME": str(root / "config"), "XDG_STATE_HOME": str(root / "state"),
                    "MEMCASTLE_ASSETS_DIR": str(root / "assets"),
                    "MEMCASTLE_MINING_BUNDLED_DIR": str(root / "assets/sources")})
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        base = f"http://127.0.0.1:{port}"
        process = subprocess.Popen((str(binary), "--palace", str(root / "palace"), "serve",
                                    "--bind", "127.0.0.1", "--port", str(port)),
                                   cwd=root, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            ready(process, base)
            try:
                request(base, "/api/source-packages", method="POST", data=legacy)
            except HTTPError as error:
                refusal = json.load(error)
                assert refusal["code"] == "memcastle::source::consent_required", refusal
                digest = refusal["help"].split("--consent ", 1)[1].split("`", 1)[0]
            else:
                raise AssertionError(f"{name} installed without permission consent")
            request(base, "/api/source-packages?consent=" + digest + "&enable=true",
                    method="POST", data=legacy)
            if name == "codex":
                sessions = root / "sessions" / "2026" / "10" / "08"
                sessions.mkdir(parents=True)
                fixture = module / "fixtures/codex-rollouts/tree/2026/10/08/rollout-2026-10-08T10-00-00-session-a.jsonl"
                shutil.copyfile(fixture, sessions / fixture.name)
                locator = root / "sessions"
            else:
                locator = root / "conversations.json"
                shutil.copyfile(module / "fixtures/export/conversations.json", locator)
            request(base, f"/api/miners/{name}", method="PUT", data=json.dumps({
                "source": name, "locator": str(locator),
            }).encode(), headers={"Content-Type": "application/json"})
            assert mine(base, name, locator)["created"] >= 1
            before = next(item for item in request(base, "/api/sources")["sources"] if item["source"] == name)
            request(base, f"/api/source-packages/{name}", method="DELETE")
            preview = request(base, "/api/plugins/preview", method="POST", data=archive)
            assert name in preview["adoptions"], preview
            consents = {item["source"]: item["digest"] for item in preview["source_consents"]}
            try:
                request(base, "/api/plugins", method="POST", data=archive,
                        headers={"X-MemCastle-Consents": json.dumps(consents)})
            except HTTPError as error:
                assert error.code == 409, error.read()
            else:
                raise AssertionError("mined source adopted without explicit permission")
            request(base, "/api/plugins", method="POST", data=archive, headers={
                "X-MemCastle-Consents": json.dumps(consents),
                "X-MemCastle-Adopt-Sources": json.dumps([name]),
            })
            after = next(item for item in request(base, "/api/sources")["sources"] if item["source"] == name)
            assert (before["cursor"], before["documents"]) == (after["cursor"], after["documents"])
            assert request(base, f"/api/miners/{name}")["source"] == name
            request(base, f"/api/source-packages/{name}/enable", method="POST")
            resumed = mine(base, name, locator)
            assert resumed["created"] == 0, f"{name} replayed mined history: {resumed}"
        finally:
            process.terminate()
            try:
                process.communicate(timeout=20)
            except subprocess.TimeoutExpired:
                process.kill()
                process.communicate()


if __name__ == "__main__":
    executable = Path(os.environ["MEMCASTLE_BIN"]).resolve()
    plugin = (ROOT / "dist/openai-0.1.0.tar.gz").read_bytes()
    for source in ("codex", "chatgpt"):
        handoff(executable, plugin, source)
    print("Codex and ChatGPT miners, cursors and documents survived explicit adoption")
