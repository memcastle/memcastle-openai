import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"
import test from "node:test"

import plugin from "../dist/index.js"

test("the integration exposes independent agent lifecycle hooks", async () => {
  assert.equal(plugin.id, "codex")
  assert.deepEqual(await plugin.server(), {})
  assert.deepEqual(await plugin.setup(), {})
})

test("the integration entry is the asset installed for the agent", async () => {
  const manifest = await readFile(new URL("../memcastle-integration.toml", import.meta.url), "utf8")
  assert.match(manifest, /entry = "index\.js"/)
  assert.match(manifest, /from = "dist"/)
})
