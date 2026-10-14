# Contributing to memcastle-openai

Install Rust with the `wasm32-wasip2` target and Python 3.11 or later.
The Codex source reads dated JSONL rollouts in `modules/codex/src/lib.rs`.
ChatGPT's export and experimental web readers live under `modules/chatgpt/src/`.
The Codex agent marketplace integration lives under `modules/codex/plugin/`.

Keep `openai` as the plugin ID, and preserve `codex` and `chatgpt` source IDs and the `codex` integration ID.
Changing the source name, account or locator changes cursor and document identity; a ChatGPT project selection resolves to
project IDs and must retain that behavior for existing persistent miners.
Review changes to permissions, the OAuth declaration and WIT/Core compatibility against existing installs.

Run these checks with `MEMCASTLE_BIN` set to a plugin-capable Core binary:

```sh
mise run package
mise run test
"$MEMCASTLE_BIN" source test modules/codex --no-build
PATH="$PWD/modules/chatgpt/fixtures/bin:$PATH" "$MEMCASTLE_BIN" source test modules/chatgpt --no-build
python3 scripts/smoke_install.py
python3 scripts/smoke_adoption.py
python3 scripts/smoke_bundled_adoption.py
python3 scripts/smoke_chatgpt.py
```

The archive is `dist/openai-<plugin.version>.tar.gz`.
`scripts/package.py` updates reviewed manifest/component hashes in `plugin.toml`.
Commit both source `Cargo.lock` files, the reviewed hashes, fixtures and code together.
Provider CI builds only these two modules, checks their tests and validates the archive against Core's shared workflow.
That workflow tests Core `main`, reports its exact SHA and accepts additional plugin-capable stable versions;
Core 0.4.1 predates plugin installation and cannot join the matrix.
The bundled-upgrade smoke verifies the manual `source remove` step before `--adopt-source`, without a Core-specific
automatic ownership transfer.

For a release, advance `plugin.version` and each changed module version, then repackage and review the hashes and
compatibility matrix on `main`.
The `openai/v<version>` tag triggers the release workflow, which requires a matching reviewed manifest,
rebuilds the archive and refuses an existing release.
The public plugin catalogue lists this repository, but live discovery cannot resolve a version until it has a release.
