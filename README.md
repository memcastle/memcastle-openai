# openai

OpenAI Codex and ChatGPT history with Codex integration.

This provider plugin owns the versioned package, while each module has its own lifecycle.
Installing the plugin makes its modules available; enable sources and install integrations separately.

```sh
python3 scripts/package.py
python3 scripts/check_modules.py
python3 -m unittest discover -s tests
MEMCASTLE_BIN=/path/to/memcastle python3 scripts/smoke_install.py
```

The archive in `dist/` is independently releasable from MemCastle Core.
The root `plugin.toml` declares stable module identities; the package script fills artifact hashes after building.
Commit each source module's generated `Cargo.lock` before publishing a release.
The source module carries the versioned public WIT contract and its own fixtures; the agent integration uses MCP/HTTP.
No module needs a private Core checkout or a sibling provider to build or test.
The smoke check starts an isolated daemon and uses a temporary agent configuration to verify the archive install path.
Provider CI builds and packages locally, then calls Core's shared artifact-validation workflow for a `[main, versions...]`
matrix, with additional exact plugin-capable
stable versions configured in `core-versions` and the `main` prerelease always included.
The reusable validation workflow follows Core's `main` branch without a revision pin.
It runs on every pull request and every push to `main`, including merges, without a second push run on PR branches.
Every version exercises source fixtures and archive installation/activation; incompatible versions fail the workflow.
The development release body records the exact Core source SHA, and the workflow summary reports it for each run.
Core CI separately tests a freshly composed project against its current install contract.
The provider's own release workflow accepts only a `openai/v<plugin.toml version>` tag on `main`, then builds
the reviewed artifact, runs manifest and compatibility-matrix tests, and refuses an existing release.
Keep IDs stable when moving this project from `plugins/openai/` to its own repository.

The separate `coverage.yml` workflow measures native Rust source tests and Bun integration `test:cover` suites,
then uploads the generated LCOV reports to Codecov using OIDC. Enable this repository in Codecov to see PR statuses.
Modules without an instrumented suite are not reported; the normal plugin checks still run independently.

