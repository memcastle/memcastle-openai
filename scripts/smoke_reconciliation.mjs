// Provider lifecycle guarantees through the actual component host and durable Core inventory.
import assert from "node:assert/strict"
import { copyFileSync, mkdirSync, readFileSync, rmSync, utimesSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { ROOT, isolated, pluginArchive, waitJob } from "./smoke_common.mjs"

const archive = readFileSync(pluginArchive())
await isolated("openai-reconciliation-", ({ env }) => {
  env.MEMCASTLE_CHATGPT_BEARER = "fixture-oauth"
  env.PATH = `${join(ROOT, "sources/chatgpt/fixtures/bin")}:${process.env.PATH}`
}, async ({ root, request, stop, start }) => {
  const preview = await request("/api/plugins/preview", { method: "POST", data: archive })
  await request("/api/plugins", { method: "POST", data: archive, headers: {
    "X-MemCastle-Consents": JSON.stringify(Object.fromEntries(preview.source_consents.map(({ source, digest }) => [source, digest]))),
  } })
  for (const source of ["codex", "chatgpt"]) await request(`/api/source-packages/${source}/enable`, { method: "POST" })

  const set = (name, json) => request(`/api/miners/${name}`, { method: "PUT", json: { allow_broaden: true, ...json } })
  const run = async (name, full = false, expected = "completed") => {
    const job = await waitJob(request, await request(`/api/miners/${name}/run`, { method: "POST", json: { full } }))
    assert.equal(job.status, expected, JSON.stringify(job))
    return job
  }
  // Query bookkeeping read-only, to distinguish retirement from an ingestion counter of zero.
  let socket
  let sequence = 0
  const connect = async () => {
    const endpoint = await request("/api/db", { method: "POST", json: { port: 0 } })
    socket = new WebSocket(`ws://${endpoint.addr}/rpc`)
    await new Promise((resolve, reject) => {
      socket.addEventListener("open", resolve, { once: true })
      socket.addEventListener("error", reject, { once: true })
    })
  }
  const query = async (sql) => {
    const response = await new Promise((resolve) => {
      socket.addEventListener("message", ({ data }) => resolve(JSON.parse(data)), { once: true })
      socket.send(JSON.stringify({ id: ++sequence, method: "query", params: [sql] }))
    })
    assert.ok(!response.error, JSON.stringify(response))
    for (const statement of response.result) assert.equal(statement.status, "OK", JSON.stringify(response))
    return response.result.at(-1).result
  }
  await connect()
  try {
    const sessions = join(root, "sessions")
    const date = join(sessions, "2026/11/11")
    mkdirSync(date, { recursive: true })
    const rollout = (cwd, text) => `${JSON.stringify({ type: "session_meta", payload: { id: "stable", cwd } })}\n${JSON.stringify({ type: "message", role: "user", content: text })}\n`
    const a = join(date, "rollout-a.jsonl")
    const b = join(date, "rollout-b.jsonl")
    writeFileSync(a, rollout("/work/alpha", "Alpha original"))
    writeFileSync(b, rollout("/work/beta", "Beta original"))
    const codexIds = new Map()
    for (const name of ["codex-one", "codex-two"]) {
      await set(name, { source: "codex", locator: sessions, wing: "shared", room: "history" })
      codexIds.set(name, (await run(name)).result.source_id)
      await run(name)
    }
    const sources = await query("SELECT * FROM source WHERE source = 'codex'")
    assert.equal(sources.length, 2, "identical named miners have independent source identities")
    assert.notEqual(codexIds.get("codex-one"), codexIds.get("codex-two"))
    const inventory = () => query("SELECT * FROM source_inventory")
    const documents = () => query("SELECT * FROM source_document")
    const codexDrawers = () => query("SELECT * FROM drawer WHERE source.origin.source = 'codex'")
    assert.equal((await inventory()).length, 4)
    const original = (await documents()).find((doc) => doc.source === codexIds.get("codex-one") && doc.external_id.endsWith("rollout-a.jsonl"))
    assert.ok(original)
    const originalRevision = original.revision
    await set("codex-one", { options: { project: "/work/beta" } })
    assert.equal((await run("codex-one", true)).result.source_id, codexIds.get("codex-one"), "filter changes retain named identity")
    const retired = (await inventory()).filter((entry) => entry.retired_at)
    assert.equal(retired.length, 1)
    assert.equal(retired[0].source, codexIds.get("codex-one"))
    const retiredDrawers = await codexDrawers()
    assert.ok(retiredDrawers.some((drawer) => drawer.valid_to), "retirement retains historical drawers")
    assert.equal(retiredDrawers.filter((drawer) => !drawer.valid_to && drawer.source.origin.source_id === codexIds.get("codex-two")).length, 2, "other miner's current history remains live")
    await run("codex-one")
    assert.equal((await inventory()).filter((entry) => entry.retired_at).length, 1)
    await set("codex-one", { unset_options: ["project"] })
    await run("codex-one", true)
    assert.equal((await inventory()).filter((entry) => entry.retired_at).length, 0)
    assert.ok((await documents()).filter((doc) => doc.external_id.endsWith("rollout-a.jsonl")).every((doc) => doc.revision === originalRevision), "same-revision restoration")
    assert.equal((await codexDrawers()).filter((drawer) => !drawer.valid_to).length, 4, "restoration has no duplicate current chunks")

    writeFileSync(a, "{incomplete")
    rmSync(b)
    await run("codex-one", true)
    assert.equal((await inventory()).filter((entry) => entry.retired_at).length, 0)
    assert.equal((await documents()).length, 4, "incomplete/rotated files preserve history")
    writeFileSync(a, rollout("/work/alpha", "Edited"))
    utimesSync(a, new Date("2020-01-01"), new Date("2020-01-01"))
    await run("codex-one")
    assert.notEqual((await documents()).find((doc) => doc.source === codexIds.get("codex-one") && doc.external_id.endsWith("rollout-a.jsonl")).revision, originalRevision, "inventory finds backdated edits")
    // A skipped first acquisition registers durable pending inventory.
    const pending = join(date, "rollout-pending.jsonl")
    writeFileSync(pending, "{partial")
    await run("codex-one", true)
    assert.equal((await inventory()).length, 5)
    assert.equal((await documents()).length, 4)
    socket.close()
    await stop()
    writeFileSync(pending, rollout("/work/alpha", "Recovered after restart"))
    utimesSync(pending, new Date("2020-01-01"), new Date("2020-01-01"))
    await start()
    await connect()
    await set("codex-one", { max_documents: 1 })
    for (let i = 0; i < 10; i++) await run("codex-one")
    assert.equal((await documents()).length, 5, "pending ID recovers through limited inventory traversal")

    const exportPath = join(root, "conversations.json")
    copyFileSync(join(ROOT, "sources/chatgpt/fixtures/export/conversations.json"), exportPath)
    await set("export", { source: "chatgpt", locator: exportPath, wing: "shared", room: "history" })
    await run("export")
    const exported = (await documents()).filter((doc) => doc.metadata.backend === "export")
    assert.equal(exported.length, 2)
    writeFileSync(exportPath, "[]")
    await run("export", true)
    assert.equal((await documents()).filter((doc) => doc.metadata.backend === "export").length, 2)
    rmSync(exportPath)
    await run("export", false, "failed")
    assert.equal((await inventory()).filter((entry) => entry.retired_at).length, 0)
    const zipPath = join(root, "synthetic-export.zip")
    copyFileSync(join(ROOT, "sources/chatgpt/fixtures/export/synthetic-export.zip"), zipPath)
    await set("zip-export", { source: "chatgpt", locator: zipPath })
    await run("zip-export")
    await run("zip-export", true)
    assert.equal((await documents()).filter((doc) => doc.metadata.backend === "export").length, 4)
    writeFileSync(zipPath, "PK\u0003\u0004truncated")
    await run("zip-export", false, "failed")
    assert.equal((await inventory()).filter((entry) => entry.retired_at).length, 0)

    const webIds = new Map()
    for (const name of ["web-one", "web-two"]) {
      await set(name, { source: "chatgpt", wing: "shared", room: "history", options: { mode: "web", projects: "id:g-p-alpha" } })
      webIds.set(name, (await run(name)).result.source_id)
    }
    assert.notEqual(webIds.get("web-one"), webIds.get("web-two"))
    await set("web-one", { options: { projects: "id:g-p-beta" } })
    assert.equal((await run("web-one", true)).result.source_id, webIds.get("web-one"))
    assert.equal((await inventory()).filter((entry) => entry.retired_at).length, 2, "only the selected web miner retires Alpha")
    await set("web-one", { options: { projects: "id:g-p-alpha" } })
    await run("web-one", true)
    assert.equal((await inventory()).filter((entry) => entry.retired_at).length, 1, "Alpha restores; only Beta is excluded")
    await set("web-one", { options: { projects: "id:g-p-empty" } })
    await run("web-one", true)
    assert.equal((await inventory()).filter((entry) => entry.retired_at).length, 3)
    const state = (value) => writeFileSync(join(root, "web-fixture-state.json"), JSON.stringify(value))
    const liveBefore = (await inventory()).filter((entry) => !entry.retired_at).length
    for (const status of [401, 403, 429, 500]) {
      state({ status })
      await run("web-two", false, "failed")
      assert.equal((await inventory()).filter((entry) => !entry.retired_at).length, liveBefore)
    }
    state({ move_alpha_to_beta: true })
    await run("web-two", true)
    assert.equal((await inventory()).filter((entry) => entry.retired_at).length, 5, "fresh membership detects a project move")
    state({})
    await run("web-two", true)
    assert.equal((await inventory()).filter((entry) => entry.retired_at).length, 3)
    const beforeRename = (await documents()).find((doc) => doc.source === webIds.get("web-two"))
    state({ alpha_name: "Renamed Alpha" })
    await run("web-two", true)
    const renamed = (await documents()).find((doc) => doc.source === webIds.get("web-two") && doc.external_id === beforeRename.external_id)
    assert.equal(renamed.revision, beforeRename.revision)
    assert.equal(renamed.metadata.project_name, "Renamed Alpha", "metadata-only updates persist")
    state({ text: "Edited web answer" })
    const edit = await run("web-two", true)
    assert.ok(edit.result.superseded > 0, JSON.stringify(edit))
    state({ partial_messages: true })
    await set("web-pending", { source: "chatgpt", options: { projects: "id:g-p-alpha" } })
    await run("web-pending", false, "failed")
    const pendingCount = (await inventory()).length - (await documents()).length
    assert.ok(pendingCount > 0, "failed acquisition retains pending registration")
    socket.close()
    await stop()
    state({})
    await start()
    await connect()
    await run("web-pending")
    assert.equal((await inventory()).length, (await documents()).length, "web pending acquisition recovers")
  } finally {
    socket?.close()
  }
})
console.log("Codex/ChatGPT reconciliation: isolated miners, scope retirement/restoration, retention, pending restart and limit-one traversal passed")
