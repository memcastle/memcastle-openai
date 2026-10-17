// Run provider-owned migration checks against a disposable Core daemon.
import assert from "node:assert/strict"
import { spawn, spawnSync } from "node:child_process"
import { mkdtempSync, mkdirSync, rmSync } from "node:fs"
import { createServer } from "node:net"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"

export const ROOT = dirname(dirname(fileURLToPath(import.meta.url)))
export const BIN = process.env.MEMCASTLE_BIN && resolve(process.env.MEMCASTLE_BIN)
assert.ok(BIN, "set MEMCASTLE_BIN to a plugin-capable Core binary")

export function cli(args, env, cwd = ROOT) {
  const result = spawnSync(BIN, args, { cwd, env, encoding: "utf8", timeout: 60000 })
  assert.equal(result.status, 0, `${BIN} ${args.join(" ")}: ${result.stderr || result.error}`)
  return result.stdout
}

export async function mine(request, source, locator, options) {
  const job = await request("/api/jobs", { method: "POST", json: {
    type: "mine", source, requested_by: "test", ...(locator && { locator }), ...(options && { options }),
  } })
  for (let attempt = 0; attempt < 300; attempt++) {
    const result = await request(`/api/jobs/${job.id}`)
    if (["completed", "failed", "cancelled"].includes(result.status)) return result
    await new Promise((done) => setTimeout(done, 100))
  }
  throw new Error(`${source} mining did not finish within 30 seconds`)
}

async function freePort() {
  const server = createServer()
  await new Promise((done) => server.listen(0, "127.0.0.1", done))
  const port = server.address().port
  await new Promise((done) => server.close(done))
  return port
}

export async function isolated(prefix, configure, check) {
  const root = mkdtempSync(join(tmpdir(), prefix))
  const assets = join(root, "assets")
  mkdirSync(assets)
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("MEMCASTLE_")))
  Object.assign(env, {
    HOME: root, XDG_DATA_HOME: join(root, "data"), XDG_CONFIG_HOME: join(root, "config"),
    XDG_STATE_HOME: join(root, "state"), MEMCASTLE_ASSETS_DIR: assets,
  })
  const port = await freePort()
  const base = `http://127.0.0.1:${port}`
  let child
  let errors = ""
  const request = async (route, { method = "GET", json, data, headers = {} } = {}) => {
    const response = await fetch(base + route, {
      method, headers: { ...(json && { "Content-Type": "application/json" }), ...headers },
      body: json ? JSON.stringify(json) : data,
      signal: AbortSignal.timeout(20000),
    })
    const text = await response.text()
    const body = text ? JSON.parse(text) : null
    if (!response.ok) throw Object.assign(new Error(`HTTP ${response.status}: ${text}`), { status: response.status, body })
    return body
  }
  const stop = async () => {
    if (!child) return
    const running = child
    child = undefined
    if (running.exitCode === null) {
      running.kill("SIGTERM")
      let timer
      await Promise.race([
        new Promise((done) => running.once("exit", done)),
        new Promise((done) => { timer = setTimeout(() => { running.kill("SIGKILL"); done() }, 20000) }),
      ])
      clearTimeout(timer)
    }
  }
  const start = async () => {
    child = spawn(BIN, ["--palace", join(root, "palace"), "serve", "--bind", "127.0.0.1", "--port", String(port)], {
      cwd: root, env, stdio: ["ignore", "ignore", "pipe"],
    })
    errors = ""
    child.stderr.on("data", (chunk) => { errors += chunk })
    for (let attempt = 0; attempt < 120; attempt++) {
      assert.equal(child.exitCode, null, `disposable daemon exited: ${errors}`)
      try { await request("/api/health"); return } catch { await new Promise((done) => setTimeout(done, 250)) }
    }
    throw new Error(`disposable daemon did not become healthy: ${errors}`)
  }
  try {
    await configure?.({ root, assets, env })
    await start()
    await check({ root, assets, env, base, request, stop, start })
  } finally {
    await stop()
    rmSync(root, { recursive: true, force: true })
  }
}
