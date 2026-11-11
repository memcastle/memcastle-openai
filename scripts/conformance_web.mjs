// Separate web cases from offline fixtures so ordinary export conformance never requires credentials.
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { ROOT, cli } from "./smoke_common.mjs"

const project = mkdtempSync(join(tmpdir(), "openai-web-conformance-"))
try {
  const source = join(ROOT, "sources/chatgpt")
  const manifest = readFileSync(join(source, "memcastle-source.toml"), "utf8")
    .replace('fixtures = "fixtures"', `fixtures = ${JSON.stringify(join(source, "fixtures-web"))}`)
  writeFileSync(join(project, "memcastle-source.toml"), manifest)
  mkdirSync(join(project, "dist"))
  copyFileSync(join(source, "source.wasm"), join(project, "dist/source.wasm"))
  copyFileSync(join(source, "source.wasm"), join(project, "source.wasm"))
  const env = { ...process.env }
  delete env.MEMCASTLE_CHATGPT_COOKIE
  console.log(cli(["source", "test", project, "--no-build"], {
    ...env, MEMCASTLE_CHATGPT_BEARER: "fixture-oauth",
    PATH: `${join(source, "fixtures/bin")}:${process.env.PATH}`,
  }))
} finally {
  rmSync(project, { recursive: true, force: true })
}
