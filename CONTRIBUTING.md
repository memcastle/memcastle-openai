# Contributing to memcastle-openai

Install Rust with the `wasm32-wasip2` target, Bun and a plugin-capable Core binary.
The Codex source reads dated JSONL rollouts in `sources/codex/src/lib.rs`.
ChatGPT's export and experimental web readers live under `sources/chatgpt/src/`.
The Codex agent marketplace integration lives under `integrations/codex/plugin/`.

Keep `openai` as the plugin ID, and preserve `codex` and `chatgpt` source IDs and the `codex` integration ID.
Changing the source name, account or locator changes cursor and document identity; a ChatGPT project selection resolves to
project IDs and must retain that behavior for existing persistent miners.
Review changes to permissions, the OAuth declaration and WIT/Core compatibility against existing installs.

Run these checks with `MEMCASTLE_BIN` set to a plugin-capable Core binary:

```sh
cargo test --manifest-path sources/codex/Cargo.toml --locked
cargo test --manifest-path sources/chatgpt/Cargo.toml --locked
# Build both source components for wasm32-wasip2 and copy them to sources/<id>/source.wasm first.
(cd integrations/codex && bun install && bun run build && bun test)
mise run package
"$MEMCASTLE_BIN" plugin test dist/openai-0.1.0.tar.gz
"$MEMCASTLE_BIN" source test sources/codex --no-build
PATH="$PWD/sources/chatgpt/fixtures/bin:$PATH" "$MEMCASTLE_BIN" source test sources/chatgpt --no-build
node scripts/smoke_adoption.mjs
node scripts/smoke_chatgpt.mjs
```

The archive is `dist/openai-<manifest.toml version>.tar.gz`.
Core updates the reviewed module and artifact hashes in `manifest.toml`; `plugin.toml` is the immutable descriptor.
Commit both source `Cargo.lock` files, the reviewed manifest, fixtures and code together.
Provider CI builds only these two modules, checks their tests and validates the archive against Core's shared workflow.
That workflow tests Core `main`, reports its exact SHA and accepts additional plugin-capable stable versions;
Core 0.4.1 predates plugin installation and cannot join the matrix.
The bundled-upgrade smoke verifies the manual `source remove` step before `--adopt-source`, without a Core-specific
automatic ownership transfer.

For a release, git-cliff computes the next `manifest.toml` version and gh-ship opens a reviewed `release/next` PR.
Merging the PR publishes the validated `openai/v<version>` archive and `.sha256sum`.
The public plugin catalogue lists this repository, but live discovery cannot resolve a version until it has a release.
