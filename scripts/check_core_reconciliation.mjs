// Version 0.5.2 alone establishes neither the WASI 0.3 ABI nor reconciliation support.
import assert from "node:assert/strict"
import { execFileSync } from "node:child_process"

const required = "828f0f6e6d937fe29b379e1a63b0985c40ab8aa7"
const actual = process.env.CORE_SHA
assert.match(actual ?? "", /^[a-f0-9]{40}$/, "Core setup must report its development source SHA")
if (actual !== required) {
  const status = execFileSync("gh", ["api", `repos/memcastle/memcastle/compare/${required}...${actual}`, "--jq", ".status"], { encoding: "utf8" }).trim()
  assert.equal(status, "ahead", `Core ${actual} must contain WASI 0.3 and reconciliation baseline ${required}`)
}
console.log(`Core reconciliation provenance: ${actual}; source conformance and daemon lifecycle checks establish runtime support`)
