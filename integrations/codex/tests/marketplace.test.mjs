import { expect, test } from "bun:test"
import { readFileSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const root = dirname(dirname(fileURLToPath(import.meta.url)))

test("the Codex marketplace advertises the local MemCastle plugin", () => {
  const marketplace = JSON.parse(readFileSync(join(root, "plugin/.agents/plugins/marketplace.json"), "utf8"))
  expect(marketplace.name).toBe("memcastle-local")
  expect(marketplace.plugins.map(({ name }) => name)).toEqual(["memcastle"])
  expect(marketplace.plugins[0].source).toBe("./")
})
