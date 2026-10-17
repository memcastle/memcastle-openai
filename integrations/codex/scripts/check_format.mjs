// Shipped assets and tests should not vary with editor-specific whitespace settings.
import { readFileSync, readdirSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const files = ["dist/index.js", ...readdirSync(join(root, "tests")).filter((name) => name.endsWith(".mjs")).map((name) => `tests/${name}`)]
for (const file of files) {
  const data = readFileSync(join(root, file), "utf8")
  if (data.includes("\r") || !data.endsWith("\n") || data.split("\n").some((line) => /[\t ]+$/.test(line))) {
    throw new Error(`${file}: use LF, remove trailing whitespace and add a final newline`)
  }
}
