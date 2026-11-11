# Contributing to memcastle-openai

Install Rust with the `wasm32-wasip2` target, Bun and a reconciliation-capable Core binary
(development containing `c311d09`, not just any binary labeled `0.5.2`).
The Codex source reads dated JSONL rollouts in `sources/codex/src/lib.rs`.
ChatGPT's export and experimental web readers live under `sources/chatgpt/src/`;
the Codex marketplace integration lives under `integrations/codex/plugin/`.

Keep `openai` as the plugin ID and preserve the `codex` and `chatgpt` source IDs and `codex` integration ID.
Changing the source name, account or locator changes cursor and document identity. Core partitions named miners
independently; mutable project filters stay in options. ChatGPT project names resolve to sorted project IDs.
Review changes to permissions,
the OAuth declaration and WIT/Core compatibility against existing installations.

## Development checks

Set `MEMCASTLE_BIN` to an extension-capable Core binary, and build both source components to their
`sources/<id>/source.wasm` paths before packaging.

```sh
cargo test --manifest-path sources/codex/Cargo.toml --locked
cargo test --manifest-path sources/chatgpt/Cargo.toml --locked
# Build both source components for wasm32-wasip2 and copy them to sources/<id>/source.wasm first.
(cd integrations/codex && bun install && bun run build && bun test)
archive="$("$MEMCASTLE_BIN" plugin package --json | jq -r .archive)"
"$MEMCASTLE_BIN" plugin test "$archive"
"$MEMCASTLE_BIN" source test sources/codex --no-build
PATH="$PWD/sources/chatgpt/fixtures/bin:$PATH" "$MEMCASTLE_BIN" source test sources/chatgpt --no-build
ARCHIVE="$archive" node scripts/smoke_adoption.mjs
ARCHIVE="$archive" node scripts/smoke_chatgpt.mjs
ARCHIVE="$archive" node scripts/smoke_reconciliation.mjs
node scripts/conformance_web.mjs
```

The archive has an SCM-derived version and a generated `manifest.toml`; `plugin.toml` remains immutable.
Commit both source `Cargo.lock` files, fixtures and code together; do not commit generated archives.
Provider CI tests the modules and validates the archive against Core `development`, checking required commit
ancestry in provider-controlled jobs and actual extension support through conformance and daemon tests.
The generic shared validation additionally checks the current development binary. Its nested setup is not
SHA-constrained by the caller; the provider-controlled gate is mandatory.
Core 0.4.1 predates plugin installation. The bundled-upgrade smoke tests manual source removal before
`--adopt-source`, without an automatic ownership transfer.

## Template updates and releases

Template answers and child revisions live in `.config/git.tpl.toml`. To update, run
`git tpl update --defaults --json`, review `git tpl diff --json`, then apply with `git tpl merge --json`.
Preserve module identities and project-specific docs when reconciling a template change.
Update covered-module answers only for modules whose native tests produce nonempty LCOV.

For a release, git-cliff computes the next tag and gh-ship opens a reviewed `release/next` PR.
Merging it publishes the validated `openai-<version>.tar.gz` archive and `.sha256sum`
under the `<version>` tag.
The public plugin catalogue lists this repository, but installation by ID requires a published release.
