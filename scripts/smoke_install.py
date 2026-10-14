#!/usr/bin/env python3
"""Exercise a built plugin archive against a Core with the plugin-install contract.

MEMCASTLE_BIN selects a released or locally built Core. Everything else lives in a
temporary XDG home; neither the user's palace nor their agent configuration is used.
"""

import gzip
import io
import json
import os
from pathlib import Path
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
        env = {key: value for key, value in os.environ.items() if not key.startswith("MEMCASTLE_")}
        env.update({"HOME": scratch, "XDG_DATA_HOME": str(root / "data"),
                    "XDG_CONFIG_HOME": str(root / "config"), "XDG_STATE_HOME": str(root / "state")})
        # The local installer asks the agent its version; the stand-in never loads user plugins.
        agent = root / "bin"
        agent.mkdir()
        (agent / "opencode").write_text("#!/bin/sh\nprintf 'opencode 1.99.0\\n'\n")
        (agent / "opencode").chmod(0o755)
        env["PATH"] = str(agent) + os.pathsep + os.environ["PATH"]
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
            expected = {item["id"] for item in manifest.get("modules", [])}
            assert {item["id"] for item in preview["modules"]} == expected
            assert not request(base, "/api/plugins")
            consents = {item["source"]: item["digest"] for item in preview["source_consents"]}
            installed = request(base, "/api/plugins", method="POST", data=archive,
                                headers={"X-MemCastle-Consents": json.dumps(consents)})
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
