# codex

Connect an agent session to the MemCastle daemon.

This OpenCode integration is separately installed after its containing MemCastle plugin.
Extend `dist/index.js` with lifecycle hooks; interact with MemCastle only over HTTP or MCP.
Run `bun run format:check && bun run lint && bun test && bun run build` in this directory before packaging.
The entry and tests use Node's built-in APIs with Bun, so this scaffold needs no private Core checkout or dependency install.
