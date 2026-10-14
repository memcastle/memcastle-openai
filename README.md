# memcastle-openai

Coverage CI measures native Rust tests for both history sources and uploads LCOV to Codecov with GitHub OIDC.
Enable this repository in Codecov for PR coverage status.

OpenAI Codex and ChatGPT history with Codex integration.

This independently released plugin owns the `codex` and `chatgpt` history sources and the Codex agent integration.
The two `codex` modules share a name but have independent source and integration lifecycles.
The source manifests and integration manifest preserve the IDs and options previously shipped with Core.
See [Codex history](docs/codex-source.md), [ChatGPT history](modules/chatgpt/README.md),
and [Codex integration](modules/codex/README.md) for their respective behavior.

Installing the plugin makes its modules available; enable each source and install the Codex integration separately.

```sh
python3 scripts/package.py
python3 scripts/check_modules.py
python3 -m unittest discover -s tests
MEMCASTLE_BIN=/path/to/memcastle python3 scripts/smoke_install.py
MEMCASTLE_BIN=/path/to/memcastle python3 scripts/smoke_adoption.py
MEMCASTLE_BIN=/path/to/memcastle python3 scripts/smoke_bundled_adoption.py
MEMCASTLE_BIN=/path/to/memcastle python3 scripts/smoke_chatgpt.py
MEMCASTLE_BIN=/path/to/memcastle
"$MEMCASTLE_BIN" source test modules/codex --no-build
PATH="$PWD/modules/chatgpt/fixtures/bin:$PATH" "$MEMCASTLE_BIN" source test modules/chatgpt --no-build
```

The archive in `dist/` is independently releasable from MemCastle Core.
The root `plugin.toml` declares stable typed module identities; the package script fills artifact hashes after building.
Commit each source module's generated `Cargo.lock` before publishing a release.
Each source carries the versioned public WIT contract and its own fixtures; the Codex integration uses MCP/HTTP.
No module needs a private Core checkout or a sibling provider to build or test.
The install smoke checks local catalogue discovery, digest verification and the Codex marketplace registration
in an isolated daemon and agent home.
The adoption smoke installs both historic sources and verifies miners, cursors and mined documents survive an explicit handoff.
The bundled-upgrade smoke starts with an enabled Core-style bundle, removes it between daemon starts, then confirms that
manual source removal is needed before adoption and that mined history survives the handoff.
The ChatGPT smoke exercises export and synthetic web history, project selection and repeated sweeps without network access.
Provider CI builds and packages locally, then calls Core's shared artifact-validation workflow for a `[main, versions...]`
matrix, with additional exact plugin-capable
stable versions configured in `core-versions` and the `main` prerelease always included.
The reusable validation workflow follows Core's `main` branch without a revision pin.
It runs on every pull request and every push to `main`, including merges, without a second push run on PR branches.
Every version exercises source fixtures and archive installation/activation; incompatible versions fail the workflow.
The development release body records the exact Core source SHA, and the workflow summary reports it for each run.
Core CI separately tests a freshly composed project against its current install contract.
The provider's release workflow accepts only an `openai/v<plugin.toml version>` tag on `main`, then builds
the reviewed artifact, runs manifest and compatibility-matrix tests, and refuses an existing release.
Only tag after reviewing the exact packaged hashes and the validation results; the workflow publishes the archive.

### Migration from Core

After updating Core, remove any old bundled package rows with `memcastle source remove codex` and/or
`memcastle source remove chatgpt` before installing this plugin.
This leaves mined data, cursors and `[[miners]]` definitions intact, but resets activation and deletes the old source's
credential; sign in to ChatGPT again for web mining.
Preview `memcastle plugin install openai` and supply the printed consent digests for both sources.
On an existing palace mined by the old Core-bundled `codex` or `chatgpt` source, also pass
`--adopt-source codex --adopt-source chatgpt` for whichever sources have mined data.
Remove a separately installed legacy source package with that ID before installing the plugin as well.
Run `memcastle source enable codex` and/or `memcastle source enable chatgpt`, then
`memcastle integration install codex` to register the Codex marketplace extension.
Keep existing `[[miners]]` entries, locators, account labels and project filters unchanged;
those values identify the existing source cursor and retained documents.
