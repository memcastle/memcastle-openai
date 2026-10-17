// Keep Codex and ChatGPT mining history across installed and Core-bundled handoffs.
import assert from "node:assert/strict"
import { copyFileSync, mkdirSync, readFileSync } from "node:fs"
import { join } from "node:path"
import { ROOT, cli, isolated, mine } from "./smoke_common.mjs"

const plugin = readFileSync(join(ROOT, "dist/openai-0.1.0.tar.gz"))

function fixture(root, name) {
  const module = join(ROOT, "sources", name)
  if (name === "codex") {
    const sessions = join(root, "sessions/2026/10/08")
    mkdirSync(sessions, { recursive: true })
    const file = "rollout-2026-10-08T10-00-00-session-a.jsonl"
    copyFileSync(join(module, "fixtures/codex-rollouts/tree/2026/10/08", file), join(sessions, file))
    return join(root, "sessions")
  }
  const exportPath = join(root, "conversations.json")
  copyFileSync(join(module, "fixtures/export/conversations.json"), exportPath)
  return exportPath
}

for (const name of ["codex", "chatgpt"]) {
  const module = join(ROOT, "sources", name)
  cli(["source", "package", module, "--no-build"], process.env)
  const legacy = readFileSync(join(module, "dist", `${name}-0.1.0.tar.gz`))
  for (const bundled of [false, true]) {
    await isolated(`memcastle-${name}-${bundled ? "bundled" : "installed"}-adopt-`, ({ root, assets, env }) => {
      env.MEMCASTLE_MINING_BUNDLED_DIR = join(assets, "sources")
      if (!bundled) return
      const old = join(assets, "sources", name)
      mkdirSync(old, { recursive: true })
      copyFileSync(join(module, "memcastle-source.toml"), join(old, "memcastle-source.toml"))
      copyFileSync(join(module, "dist/source.wasm"), join(old, "source.wasm"))
    }, async ({ root, assets, env, request, stop, start }) => {
      if (bundled) {
        assert.equal((await request(`/api/source-packages/${name}`)).origin, "bundled")
        await request(`/api/source-packages/${name}/enable`, { method: "POST" })
      } else {
        let digest
        try { await request("/api/source-packages", { method: "POST", data: legacy }) }
        catch (error) {
          assert.equal(error.body.code, "memcastle::source::consent_required")
          digest = error.body.help.split("--consent ")[1].split("`")[0]
        }
        assert.ok(digest, `${name} must require permissions consent`)
        await request(`/api/source-packages?consent=${encodeURIComponent(digest)}&enable=true`, { method: "POST", data: legacy })
      }
      const locator = fixture(root, name)
      await request(`/api/miners/${name}`, { method: "PUT", json: { source: name, locator } })
      const run = async () => {
        const job = await mine(request, name, locator)
        assert.equal(job.status, "completed", JSON.stringify(job))
        return job.result
      }
      assert.ok((await run()).created >= 1)
      const before = (await request("/api/sources")).sources.find((item) => item.source === name)
      if (bundled) {
        await assert.rejects(request("/api/plugins/preview", { method: "POST", data: plugin }), { status: 409 })
        await stop()
        const freshAssets = join(root, "new-assets")
        mkdirSync(join(freshAssets, "sources"), { recursive: true })
        env.MEMCASTLE_ASSETS_DIR = freshAssets
        env.MEMCASTLE_MINING_BUNDLED_DIR = join(freshAssets, "sources")
        await start()
        assert.equal((await request(`/api/source-packages/${name}`)).origin, "bundled")
        let blocked = false
        try { await request("/api/plugins/preview", { method: "POST", data: plugin }) }
        catch (error) {
          assert.equal(error.status, 409)
          assert.equal(error.body.code, "memcastle::plugin::blocked")
          blocked = true
        }
        assert.ok(blocked, `${name} must not be claimed while its old package row exists`)
      }
      await request(`/api/source-packages/${name}`, { method: "DELETE" })
      const preview = await request("/api/plugins/preview", { method: "POST", data: plugin })
      assert.ok(preview.adoptions.includes(name))
      const consents = Object.fromEntries(preview.source_consents.map(({ source, digest }) => [source, digest]))
      if (!bundled) {
        await assert.rejects(request("/api/plugins", { method: "POST", data: plugin,
          headers: { "X-MemCastle-Consents": JSON.stringify(consents) } }), { status: 409 })
      }
      await request("/api/plugins", { method: "POST", data: plugin, headers: {
        "X-MemCastle-Consents": JSON.stringify(consents), "X-MemCastle-Adopt-Sources": JSON.stringify([name]),
      } })
      const after = (await request("/api/sources")).sources.find((item) => item.source === name)
      assert.deepEqual([after.cursor, after.documents], [before.cursor, before.documents])
      assert.equal((await request(`/api/miners/${name}`)).source, name)
      assert.equal((await request(`/api/source-packages/${name}`)).state, "installed")
      await request(`/api/source-packages/${name}/enable`, { method: "POST" })
      assert.equal((await run()).created, 0, `${name} replayed mined history`)
    })
  }
}
console.log("Codex and ChatGPT installed/bundled miners, cursors and documents survived explicit adoption")
