# memcastle-openai

[![CI][ci-badge]][ci-workflow] [![Codecov][codecov-badge]][codecov-project]

[ci-badge]: https://github.com/memcastle/memcastle-openai/actions/workflows/ci.yaml/badge.svg?branch=main
[ci-workflow]: https://github.com/memcastle/memcastle-openai/actions/workflows/ci.yaml
[codecov-badge]: https://codecov.io/gh/memcastle/memcastle-openai/graph/badge.svg
[codecov-project]: https://codecov.io/gh/memcastle/memcastle-openai

OpenAI Codex and ChatGPT history with Codex agent integration.

This independently released plugin owns the `codex` and `chatgpt` history sources and the `codex` agent
integration. The two `codex` modules share a name but have independent lifecycles and separate
`sources/codex/` and `integrations/codex/` paths. Their manifests preserve the IDs and options previously
shipped with Core. See [Codex history](docs/codex-source.md), [ChatGPT history](sources/chatgpt/README.md),
and [Codex integration](integrations/codex/README.md) for their respective behavior.

## Install

When a release is available in the [MemCastle plugin catalogue](https://memcastle.github.io/plugins.json),
install it by ID with a plugin-capable Core binary:

```sh
memcastle plugin install openai
memcastle source enable codex
memcastle source enable chatgpt
memcastle integration install codex
```

Core retrieves and verifies the release archive. The sources are enabled explicitly; agent registration
is a separate step. If there is no published release yet, build the archive below and install
`dist/openai-VERSION.tar.gz` instead.

### Migration from Core

After updating Core, remove any old bundled package rows with `memcastle source remove codex` and/or
`memcastle source remove chatgpt` before installing the plugin. This leaves mined data, cursors and `[[miners]]`
definitions intact, but resets activation and deletes the old source credential; sign in to ChatGPT again for
web mining. Remove a separately installed legacy source package with the same ID as well.

Preview `memcastle plugin install openai` and supply the printed consent digests for both sources.
On a palace previously mined by bundled `codex` or `chatgpt`, also pass `--adopt-source codex` and/or
`--adopt-source chatgpt` for whichever sources have mined data. Then enable the sources and register the Codex
integration. Keep existing `[[miners]]` entries, locators, account labels and project filters unchanged;
they identify existing cursors and retained documents.

## Build and test locally

Install Rust with `wasm32-wasip2`, Bun and a plugin-capable Core binary. Build both source components to their
`sources/<id>/source.wasm` paths and the Codex integration into `integrations/codex/dist/` before packaging.
Commit both generated source `Cargo.lock` files. See the CI workflow for reproducible build and copy steps.

```sh
memcastle plugin inspect --json
memcastle plugin package
memcastle plugin test dist/openai-VERSION.tar.gz
node scripts/smoke_adoption.mjs
node scripts/smoke_chatgpt.mjs
memcastle source test sources/codex --no-build
PATH="$PWD/sources/chatgpt/fixtures/bin:$PATH" memcastle source test sources/chatgpt --no-build
```

Use the archive name generated in `dist/` instead of `VERSION`. Core writes the SCM-versioned `manifest.toml`
inside the archive with artifact checksums and a matching external `.sha256sum`; `plugin.toml` stays immutable.
No module needs a sibling Core checkout to build or test. The install smoke checks catalogue discovery and Codex
marketplace registration; adoption checks preserve existing miners, cursors and documents. The bundled-upgrade
smoke checks manual source removal before adoption, and the ChatGPT smoke tests export and synthetic web history
without network access.

## CI and releases

Provider CI builds and packages locally, then calls Core's shared validation for the selected `development`
release. The Core setup action verifies its binary against `SHA256SUMS` and reports its source SHA.
Validation runs on pull requests and pushes to `main`, checking source fixtures, installation and activation.
The `openai/vVERSION` release tag is validated before attaching the archive to gh-ship's draft release.

git-cliff and gh-ship open a reviewed `release/next` PR for releasable commits. Configure `APP_CLIENT_ID` and
`APP_PRIVATE_KEY` in the `release` environment for an installed GitHub App. Native Cargo/Bun coverage and test
results upload to Codecov: fixed 15% project, 80% patch and 80% reporting-source component floors. Source CI
also requires strictly greater than 80% native production-line coverage per adapter (excluding inline tests).
The `integration-codex` component remains informational.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for module checks and [AGENTS.md](AGENTS.md) for coding-agent guidance.
