# Codex transcript source

Use upstream development Core containing `828f0f6`; stable 0.5.2 predates the WASI 0.3 baseline.
The async WIT exports use stackful `sync: true` bindings without changing reconciliation behavior.
From the plugin root, `mise run source:codex:build` stages the package-ready component.
Validate with `python3 scripts/validate_component.py source sources/codex/source.wasm` and wasm-tools 1.263.0.

This independently activatable source reads dated Codex JSONL rollouts with stable relative-path document IDs.
It exports base contract `0.5.0` plus Core-managed reconciliation `0.1.0`, including a minimal cwd-based `project` filter.
See [Codex history](../../docs/codex-source.md) for scope, configuration and adoption behavior.

Build with Rust 1.100+ using `cargo build --locked --release --target wasm32-wasip3` and run
`memcastle source test sources/codex --no-build` from the plugin root after copying the WASM to `source.wasm`.
The Codex agent integration lives separately in [`integrations/codex/`](../../integrations/codex/).
