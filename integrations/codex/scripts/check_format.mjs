import { readFileSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const root = dirname(dirname(fileURLToPath(import.meta.url)))
for (const file of ["plugin/plugin.json", "plugin/mcp.json", "plugin/.agents/plugins/marketplace.json"]) {
  const data = readFileSync(join(root, file), "utf8")
  if (data.includes("\r") || !data.endsWith("\n") || data.split("\n").some((line) => /[\t ]+$/.test(line))) {
    throw new Error(`${file}: use LF, remove trailing whitespace and add a final newline`)
  }
}
