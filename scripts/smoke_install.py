#!/usr/bin/env python3
"""Exercise a built plugin archive against a Core with the plugin-install contract.

MEMCASTLE_BIN selects a released or locally built Core. Everything else lives in a
temporary XDG home; neither the user's palace nor their agent configuration is used.
"""

import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tarfile
import tempfile
import time
import tomllib
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[1]


def request(base: str, route: str, *, method: str = "GET", data: bytes | None = None,
            headers: dict[str, str] | None = None):
    with urlopen(Request(base + route, data=data, method=method, headers=headers or {}), timeout=20) as response:
        return json.load(response)


def incompatible(archive: bytes) -> bytes:
    """Change only the parent Core bound to exercise the host's version gate."""
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as bundle:
        entries = [(info.name, bundle.extractfile(info).read()) for info in bundle if info.isfile()]
    output = io.BytesIO()
    with gzip.GzipFile(fileobj=output, mode="wb", filename="", mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as bundle:
            for name, data in entries:
                if name == "plugin.toml":
                    data = data.replace(b'memcastle = ">=0.4, <0.6"', b'memcastle = ">=99"', 1)
                info = tarfile.TarInfo(name)
                info.size = len(data)
                bundle.addfile(info, io.BytesIO(data))
    return output.getvalue()


def main() -> None:
    binary = Path(os.environ["MEMCASTLE_BIN"]).resolve()
    manifest = tomllib.loads((ROOT / "plugin.toml").read_text())
    plugin = manifest["plugin"]["id"]
    archive = (ROOT / "dist" / f"{plugin}-{manifest['plugin']['version']}.tar.gz").read_bytes()
    with tempfile.TemporaryDirectory(prefix="memcastle-plugin-smoke-") as scratch:
        root = Path(scratch)
        (root / "assets").mkdir()
        registry = root / "plugins.json"
        (root / f"{plugin}-{manifest['plugin']['version']}.tar.gz").write_bytes(archive)
        registry.write_text(json.dumps({"format": 1, "name": "OpenAI smoke registry", "plugins": [{
            "id": plugin, "description": manifest["plugin"]["description"],
            "repository": "memcastle/memcastle-openai",
            "modules": [{"id": item["id"], "type": item["type"]} for item in manifest["modules"]],
            "versions": [{"version": manifest["plugin"]["version"],
                          "url": f"{plugin}-{manifest['plugin']['version']}.tar.gz",
                          "sha256": hashlib.sha256(archive).hexdigest()}],
        }]}))
        config = root / "config.toml"
        config.write_text(f'[plugins]\nregistries = ["{registry}"]\n')
        env = {key: value for key, value in os.environ.items() if not key.startswith("MEMCASTLE_")}
        env.update({"HOME": scratch, "XDG_DATA_HOME": str(root / "data"),
                    "XDG_CONFIG_HOME": str(root / "config"), "XDG_STATE_HOME": str(root / "state"),
                    "MEMCASTLE_CONFIG": str(config), "MEMCASTLE_ASSETS_DIR": str(root / "assets"),
                    "MEMCASTLE_MINING_BUNDLED_DIR": str(root / "assets/sources")})
        # The local installer asks the agent its version; the stand-in never loads user plugins.
        agent = root / "bin"
        agent.mkdir()
        (agent / "codex").write_text('''#!/bin/sh
case "$1:$2" in
  --version:*) echo 'codex-cli 0.200.0' ;;
  plugin:marketplace) : ;;
  plugin:add) echo "$3" >> "$FAKE_CODEX_STATE" ;;
  plugin:remove) sed "/^$3$/d" "$FAKE_CODEX_STATE" > "$FAKE_CODEX_STATE.tmp"; mv "$FAKE_CODEX_STATE.tmp" "$FAKE_CODEX_STATE" ;;
  plugin:list) printf 'PLUGIN STATUS VERSION SOURCE\\n'; while IFS= read -r item; do printf '%s installed 0.1.0 /tmp\\n' "$item"; done < "$FAKE_CODEX_STATE" ;;
  *) exit 2 ;;
esac
''')
        (agent / "codex").chmod(0o755)
        state = root / "codex-plugins"
        state.write_text("")
        env["FAKE_CODEX_STATE"] = str(state)
        env["PATH"] = str(agent) + os.pathsep + os.environ["PATH"]
        # Simulate the previous Core-shipped integration without a checkout of old Core.
        old_assets = root / "old-assets"
        old_codex = old_assets / "integrations/codex"
        old_codex.mkdir(parents=True)
        manifest_text = (ROOT / "modules/codex/memcastle-integration.toml").read_text()
        (old_codex / "memcastle-integration.toml").write_text(
            manifest_text.replace('memcastle = ">=0.4, <0.6"', 'memcastle = ">=0.2"').replace("local = true\n", ""))
        shutil.copytree(ROOT / "modules/codex/plugin", old_codex / "dist")
        shutil.copytree(ROOT / "modules/codex/skills", old_assets / "skills")
        previous = subprocess.run((str(binary), "--json", "integration", "install", "codex",
                                   "--assets-dir", str(old_assets)), env=env, capture_output=True, text=True)
        assert previous.returncode == 0, previous.stderr
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        base = f"http://127.0.0.1:{port}"
        process = subprocess.Popen([str(binary), "--palace", str(root / "palace"), "serve",
                                    "--bind", "127.0.0.1", "--port", str(port)],
                                   cwd=root, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            for _ in range(120):
                if process.poll() is not None:
                    raise RuntimeError(f"daemon exited during startup: {process.stderr.read().decode()}")
                try:
                    request(base, "/api/health")
                    break
                except (URLError, TimeoutError):
                    time.sleep(0.25)
            else:
                raise RuntimeError("temporary daemon did not become healthy within 30 seconds")
            preview = request(base, "/api/plugins/preview", method="POST", data=archive)
            assert preview["id"] == plugin
            expected = {(item["type"], item["id"]) for item in manifest["modules"]}
            assert {(item["type"], item["id"]) for item in preview["modules"]} == expected
            assert not request(base, "/api/plugins")
            consents = {item["source"]: item["digest"] for item in preview["source_consents"]}
            discovered = request(base, "/api/plugin-registry/search?q=openai")
            assert discovered["plugins"][0]["id"] == plugin and not discovered["warnings"], discovered
            indexed = request(base, f"/api/plugin-registry/plugins/{plugin}")
            assert indexed["archive_digest"] == hashlib.sha256(archive).hexdigest(), indexed
            assert {(item["type"], item["id"]) for item in indexed["plugin"]["modules"]} == expected
            release = root / f"{plugin}-{manifest['plugin']['version']}.tar.gz"
            release.write_bytes(b"tampered after preview")
            try:
                request(base, "/api/plugin-registry/install", method="POST", data=json.dumps({
                    "id": plugin, "version": manifest["plugin"]["version"],
                    "archive_digest": indexed["archive_digest"], "consents": consents,
                }).encode(), headers={"Content-Type": "application/json"})
            except HTTPError as error:
                assert error.code == 502 and json.load(error)["code"] == "memcastle::plugin::integrity"
            else:
                raise AssertionError("the changed catalogue asset was installed")
            release.write_bytes(archive)
            installed = request(base, "/api/plugin-registry/install", method="POST", data=json.dumps({
                "id": plugin, "version": manifest["plugin"]["version"],
                "archive_digest": indexed["archive_digest"], "consents": consents,
            }).encode(), headers={"Content-Type": "application/json"})
            assert installed["id"] == plugin
            assert request(base, f"/api/plugins/{plugin}")["id"] == plugin
            for item in manifest.get("modules", []):
                if item["type"] == "source":
                    source = request(base, f"/api/source-packages/{item['id']}")
                    assert source["state"] == "installed", source
                    command = subprocess.run([str(binary), "--palace", str(root / "palace"),
                                              "source", "enable", item["id"]], env=env,
                                             capture_output=True, text=True)
                    assert command.returncode == 0, command.stderr
                elif item["type"] == "integration":
                    command = subprocess.run([str(binary), "--json", "integration", "install", item["id"]],
                                             env=env, capture_output=True, text=True)
                    assert command.returncode == 0, command.stderr
                    assert json.loads(command.stdout)["id"] == item["id"]
                    assert state.read_text().splitlines() == ["memcastle@memcastle-local"]
                    receipt = json.loads((root / "data/memcastle/agents/codex/.memcastle-install.json").read_text())
                    # Identical files can retain the original receipt until a later integration update;
                    # the installed copy and Codex marketplace remain intact either way.
                    assert receipt["assets_root"] == str(old_assets) or "/plugins/openai/" in receipt["assets_root"], receipt
                    assert (root / "data/memcastle/agents/codex/mcp.json").is_file()
            if b'memcastle = ">=0.4, <0.6"' in archive:
                try:
                    request(base, "/api/plugins/preview", method="POST", data=incompatible(archive))
                except HTTPError as error:
                    assert error.code == 400, error.read()
                else:
                    raise AssertionError("an incompatible parent manifest was accepted")
        finally:
            process.terminate()
            try:
                process.communicate(timeout=20)
            except subprocess.TimeoutExpired:
                process.kill()
                process.communicate()
    print("generated plugin preview, install, module lifecycle and version gate passed")


if __name__ == "__main__":
    main()
