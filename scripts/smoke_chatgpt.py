#!/usr/bin/env python3
"""Exercise ChatGPT export and offline web acquisition through an installed plugin."""

import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time

from smoke_install import ROOT, request
from smoke_adoption import ready


def main(token, cookie=None):
    binary = Path(os.environ["MEMCASTLE_BIN"]).resolve()
    archive = (ROOT / "dist/openai-0.1.0.tar.gz").read_bytes()
    with tempfile.TemporaryDirectory(prefix="memcastle-chatgpt-web-") as scratch:
        root = Path(scratch)
        (root / "assets").mkdir()
        env = {key: value for key, value in os.environ.items() if not key.startswith("MEMCASTLE_")}
        env.update({"HOME": scratch, "XDG_DATA_HOME": str(root / "data"),
                    "XDG_CONFIG_HOME": str(root / "config"), "XDG_STATE_HOME": str(root / "state"),
                    "MEMCASTLE_ASSETS_DIR": str(root / "assets"),
                    "MEMCASTLE_MINING_BUNDLED_DIR": str(root / "assets/sources"),
                    "MEMCASTLE_CHATGPT_BEARER": token,
                    "PATH": str(ROOT / "modules/chatgpt/fixtures/bin") + os.pathsep + os.environ["PATH"]})
        if cookie:
            env["MEMCASTLE_CHATGPT_COOKIE"] = cookie
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        base = f"http://127.0.0.1:{port}"
        process = subprocess.Popen((str(binary), "--palace", str(root / "palace"), "serve",
                                    "--bind", "127.0.0.1", "--port", str(port)),
                                   cwd=root, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            ready(process, base)
            preview = request(base, "/api/plugins/preview", method="POST", data=archive)
            consents = {item["source"]: item["digest"] for item in preview["source_consents"]}
            request(base, "/api/plugins", method="POST", data=archive,
                    headers={"X-MemCastle-Consents": json.dumps(consents)})
            request(base, "/api/source-packages/chatgpt/enable", method="POST")

            def mine(locator=None, options=None):
                body = {"type": "mine", "source": "chatgpt", "requested_by": "test"}
                if locator:
                    body["locator"] = str(locator)
                if options:
                    body["options"] = options
                job = request(base, "/api/jobs", method="POST", data=json.dumps(body).encode(),
                              headers={"Content-Type": "application/json"})
                for _ in range(300):
                    result = request(base, f"/api/jobs/{job['id']}")
                    if result["status"] in ("completed", "failed", "cancelled"):
                        return result
                    time.sleep(0.1)
                raise AssertionError("ChatGPT mining did not finish")

            export = root / "conversations.json"
            shutil.copyfile(ROOT / "modules/chatgpt/fixtures/export/conversations.json", export)
            assert mine(export)["result"]["created"] == 2
            assert mine(export)["result"]["created"] == 0
            alpha = {"mode": "web", "account": "fixture", "projects": "name:Alpha"}
            by_name = mine(options=alpha)
            assert by_name["status"] == "completed", by_name
            assert by_name["result"]["created"] == 2, by_name
            alpha["projects"] = "id:g-p-alpha"
            assert mine(options=alpha)["result"]["created"] == 0
            beta = dict(alpha, projects="id:g-p-beta")
            assert mine(options=beta)["result"]["created"] == 1
            all_projects = dict(alpha)
            del all_projects["projects"]
            assert mine(options=all_projects)["result"]["created"] >= 1
            invalid = dict(alpha, projects="name:Missing")
            failed = mine(options=invalid)
            assert failed["status"] == "failed", failed
            sources = [item for item in request(base, "/api/sources")["sources"] if item["source"] == "chatgpt"]
            assert len(sources) >= 4, sources
        finally:
            process.terminate()
            try:
                process.communicate(timeout=20)
            except subprocess.TimeoutExpired:
                process.kill()
                process.communicate()
if __name__ == "__main__":
    main("fixture-session", "fixture-cookie")
    main("fixture-oauth")
    print("ChatGPT export, scoped web history, cookie-free bearer, repeat sweeps and invalid project selection passed")
