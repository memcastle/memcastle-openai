# Codex transcript source

This independently activatable source reads dated Codex JSONL rollouts and preserves the `codex` source ID and cursor.
See [Codex history](../../docs/codex-source.md) for scope, configuration and adoption behavior.

Build with `cargo build --locked --release --target wasm32-wasip2` and run
`memcastle source test sources/codex --no-build` from the plugin root after copying the WASM to `source.wasm`.
The Codex agent integration lives separately in [`integrations/codex/`](../../integrations/codex/).
