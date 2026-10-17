// Exercise ChatGPT export and offline web acquisition without a real account.
import assert from "node:assert/strict"
import { copyFileSync, readFileSync } from "node:fs"
import { join } from "node:path"
import { ROOT, isolated, mine } from "./smoke_common.mjs"

const archive = readFileSync(join(ROOT, "dist/openai-0.1.0.tar.gz"))
for (const [token, cookie] of [["fixture-session", "fixture-cookie"], ["fixture-oauth", null]]) {
  await isolated("memcastle-chatgpt-web-", ({ env }) => {
    env.MEMCASTLE_CHATGPT_BEARER = token
    if (cookie) env.MEMCASTLE_CHATGPT_COOKIE = cookie
    env.PATH = `${join(ROOT, "sources/chatgpt/fixtures/bin")}:${process.env.PATH}`
  }, async ({ root, request }) => {
    const preview = await request("/api/plugins/preview", { method: "POST", data: archive })
    const consents = Object.fromEntries(preview.source_consents.map(({ source, digest }) => [source, digest]))
    await request("/api/plugins", { method: "POST", data: archive,
      headers: { "X-MemCastle-Consents": JSON.stringify(consents) } })
    await request("/api/source-packages/chatgpt/enable", { method: "POST" })
    const run = (locator, options) => mine(request, "chatgpt", locator, options)
    const exportPath = join(root, "conversations.json")
    copyFileSync(join(ROOT, "sources/chatgpt/fixtures/export/conversations.json"), exportPath)
    assert.equal((await run(exportPath)).result.created, 2)
    assert.equal((await run(exportPath)).result.created, 0)
    const alpha = { mode: "web", account: "fixture", projects: "name:Alpha" }
    const byName = await run(null, alpha)
    assert.equal(byName.status, "completed", JSON.stringify(byName))
    assert.equal(byName.result.created, 2)
    assert.equal((await run(null, { ...alpha, projects: "id:g-p-alpha" })).result.created, 0)
    assert.equal((await run(null, { ...alpha, projects: "id:g-p-beta" })).result.created, 1)
    assert.ok((await run(null, { mode: "web", account: "fixture" })).result.created >= 1)
    assert.equal((await run(null, { ...alpha, projects: "name:Missing" })).status, "failed")
    const sources = (await request("/api/sources")).sources.filter((item) => item.source === "chatgpt")
    assert.ok(sources.length >= 4)
  })
}
console.log("ChatGPT export, scoped web history, cookie-free bearer and repeat sweeps passed")
