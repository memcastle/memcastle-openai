# memcastle-openai

[![CI](https://github.com/memcastle/memcastle-openai/actions/workflows/ci.yaml/badge.svg?branch=main)](https://github.com/memcastle/memcastle-openai/actions/workflows/ci.yaml) [![Codecov](https://codecov.io/gh/memcastle/memcastle-openai/graph/badge.svg)](https://codecov.io/gh/memcastle/memcastle-openai)

OpenAI Codex and ChatGPT history with Codex integration.

This independently released plugin owns the `codex` and `chatgpt` history sources and the Codex agent integration.
The two `codex` modules share a name but have independent source and integration lifecycles.
The source manifests and integration manifest preserve the IDs and options previously shipped with Core.
See [Codex history](docs/codex-source.md), [ChatGPT history](sources/chatgpt/README.md),
and [Codex integration](integrations/codex/README.md) for their respective behavior.

Installing the plugin makes its modules available; enable each source and install the Codex integration separately.
Their separate `sources/codex/` and `integrations/codex/` roots keep their implementation and activation independent.

```sh
memcastle plugin inspect --json
# Build both sources for wasm32-wasip2 and run bun run build in integrations/codex first.
memcastle plugin package
memcastle plugin test dist/openai-VERSION.tar.gz
node scripts/smoke_adoption.mjs
node scripts/smoke_chatgpt.mjs
memcastle source test sources/codex --no-build
PATH="$PWD/sources/chatgpt/fixtures/bin:$PATH" memcastle source test sources/chatgpt --no-build
```

The archive in `dist/` is independently releasable from MemCastle Core.
The root `plugin.toml` is an immutable descriptor of typed modules; Core generates `manifest.toml` only inside the
SCM-versioned archive, with release versions and artifact checksums.
Commit each source module's generated `Cargo.lock` before publishing a release.
Each source carries the versioned public WIT contract and its own fixtures; the Codex integration uses MCP/HTTP.
No module needs a private Core checkout or a sibling provider to build or test.
The install smoke checks local catalogue discovery, digest verification and the Codex marketplace registration
in an isolated daemon and agent home.
The adoption smoke installs both historic sources and verifies miners, cursors and mined documents survive an explicit handoff.
The bundled-upgrade smoke starts with an enabled Core-style bundle, removes it between daemon starts, then confirms that
manual source removal is needed before adoption and that mined history survives the handoff.
The ChatGPT smoke exercises export and synthetic web history, project selection and repeated sweeps without network access.
Provider CI builds and packages locally, then calls Core's shared artifact-validation workflow for the static
`core_versions` selection (currently `main`); add plugin-capable stable release trees when available.
The reusable validation workflow follows Core's `main` branch without a revision pin.
It runs on every pull request and every push to `main`, including merges, without a second push run on PR branches.
Every version exercises source fixtures and archive installation/activation; incompatible versions fail the workflow.
The development release body records the exact Core source SHA, and the workflow summary reports it for each run.
Core CI separately tests a freshly composed project against its current install contract.
The release workflow validates the reviewed `openai/vVERSION` tag on `main`, then builds and
validates the artifact before attaching it to gh-ship's draft release. gh-ship and git-cliff open a reviewed
`release/next` PR for releasable commits. Configure `APP_CLIENT_ID` and `APP_PRIVATE_KEY` in the `release`
environment for an installed GitHub App. Native Cargo/Bun coverage and test results upload to Codecov.
Codecov uses a fixed 15% project floor, an 80% patch floor, and 80% floors for both source components.
Source CI also requires strictly greater than 80% native production-line coverage per adapter (excluding inline test modules).
The `integration-codex` component remains informational.

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
