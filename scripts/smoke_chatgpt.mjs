// Exercise ChatGPT export and offline web acquisition without a real account.
import assert from "node:assert/strict"
import { copyFileSync, readFileSync } from "node:fs"
import { join } from "node:path"
import { ROOT, isolated, mine, pluginArchive } from "./smoke_common.mjs"

// A standalone package exercises this source without building the plugin's unrelated Codex/integration modules.
const sourceArchive = process.env.SOURCE_ARCHIVE
const archive = readFileSync(sourceArchive ? join(ROOT, sourceArchive) : pluginArchive())
for (const [token, cookie] of [["fixture-session", "fixture-cookie"], ["fixture-oauth", null]]) {
  await isolated("memcastle-chatgpt-web-", ({ env }) => {
    env.MEMCASTLE_CHATGPT_BEARER = token
    if (cookie) env.MEMCASTLE_CHATGPT_COOKIE = cookie
    env.PATH = `${join(ROOT, "sources/chatgpt/fixtures/bin")}:${process.env.PATH}`
  }, async ({ root, request }) => {
    if (sourceArchive) {
      let digest
      try { await request("/api/source-packages", { method: "POST", data: archive }) }
      catch (error) { digest = error.body?.help?.match(/--consent\s+([a-f0-9]{64})/)?.[1] }
      assert.ok(digest, "the daemon must request consent for a standalone source package")
      await request(`/api/source-packages?consent=${digest}&enable=true`, { method: "POST", data: archive })
    } else {
      const preview = await request("/api/plugins/preview", { method: "POST", data: archive })
      const consents = Object.fromEntries(preview.source_consents.map(({ source, digest }) => [source, digest]))
      await request("/api/plugins", { method: "POST", data: archive,
        headers: { "X-MemCastle-Consents": JSON.stringify(consents) } })
      await request("/api/source-packages/chatgpt/enable", { method: "POST" })
    }
    const run = (locator, options) => mine(request, "chatgpt", locator, options)
    const exportPath = join(root, "conversations.json")
    copyFileSync(join(ROOT, "sources/chatgpt/fixtures/export/conversations.json"), exportPath)
    assert.equal((await run(exportPath)).result.created, 2)
    assert.equal((await run(exportPath)).result.created, 0)
    assert.equal((await run(exportPath, { mode: "export" })).result.created, 0)
    assert.equal((await run(exportPath, { mode: "web" })).status, "failed")
    await request("/api/source-configs/chatgpt-work", { method: "PUT", json: { source: "chatgpt" } })
    await assert.rejects(
      request("/api/source-configs/chatgpt-work/auth", { method: "POST" }),
      (error) => error.body?.error?.includes("one default web account"),
    )
    await request("/api/miners/work-export", { method: "PUT", json: {
      source: "chatgpt", source_config: "chatgpt-work", locator: exportPath, wing: "work", room: "imports",
    } })
    const preset = await request("/api/miners/work-export/run", { method: "POST" })
    assert.equal(preset.kind.preset.configuration, "chatgpt-work")
    let completed
    for (let attempt = 0; attempt < 300; attempt++) {
      completed = await request(`/api/jobs/${preset.id}`)
      if (completed.status === "completed" || completed.status === "failed") break
      await new Promise((done) => setTimeout(done, 100))
    }
    assert.equal(completed.status, "completed", JSON.stringify(completed))
    const alpha = { mode: "web", account: "fixture", projects: "name:Alpha" }
    const byName = await run(null, alpha)
    assert.equal(byName.status, "completed", JSON.stringify(byName))
    assert.equal(byName.result.created, 2)
    assert.equal((await run(null, { ...alpha, projects: "id:g-p-alpha" })).result.created, 0)
    assert.equal((await run(null, { ...alpha, projects: "id:g-p-beta" })).result.created, 1)
    assert.ok((await run(null, { mode: "web", account: "fixture" })).result.created >= 1)
    assert.ok((await run(null, {})).result.created >= 1, "the registered account needs no mode or label")
    assert.equal((await run(null, { ...alpha, projects: "name:Missing" })).status, "failed")
    const sources = (await request("/api/sources")).sources.filter((item) => item.source === "chatgpt")
    assert.ok(sources.length >= 4)
  })
}
console.log("ChatGPT export, scoped web history, cookie-free bearer and repeat sweeps passed")
