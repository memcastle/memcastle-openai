// Version 0.5.2 alone does not establish the optional reconciliation interface.
import assert from "node:assert/strict"
import { execFileSync } from "node:child_process"

const required = "c311d09d839afea2e652e8f31e088ebc9792a0f5"
const actual = process.env.CORE_SHA
assert.match(actual ?? "", /^[a-f0-9]{40}$/, "Core setup must report its development source SHA")
if (actual !== required) {
  const status = execFileSync("gh", ["api", `repos/memcastle/memcastle/compare/${required}...${actual}`, "--jq", ".status"], { encoding: "utf8" }).trim()
  assert.equal(status, "ahead", `Core ${actual} must contain reconciliation commit ${required}`)
}
console.log(`Core reconciliation provenance: ${actual}; source conformance and daemon lifecycle checks establish runtime support`)
