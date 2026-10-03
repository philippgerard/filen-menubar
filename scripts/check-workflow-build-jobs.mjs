import assert from "node:assert/strict"
import path from "node:path"
import { fileURLToPath } from "node:url"

// Run with Bun so the workflows are checked through a real YAML parser.
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
for (const name of ["build.yml", "checks.yml"]) {
  const workflow = Bun.YAML.parse(await Bun.file(path.join(repo, ".github/workflows", name)).text())
  const job = workflow.jobs.build
  const step = (description, predicate) => {
    const matches = job.steps.filter(predicate)
    assert.equal(matches.length, 1, `${name}: expected one ${description} step`)
    return matches[0]
  }
  const unconditional = (description, predicate) => {
    const match = step(description, predicate)
    assert.equal(match.if, undefined, `${name}: ${description} must always run`)
    assert.equal(match["continue-on-error"], undefined, `${name}: ${description} must fail the job`)
    return match
  }

  assert.equal(job["runs-on"], "${{ matrix.platform }}", `${name}: build job must run on its matrix platform`)
  assert.deepEqual(
    job.strategy.matrix.include
      .filter(entry => String(entry.platform).startsWith("macos"))
      .map(({ platform, target }) => ({ platform, target })),
    [{ platform: "macos-latest", target: "aarch64-apple-darwin" }],
    `${name}: macOS builds must track the supported latest arm64 image`,
  )
  const hostCheck = step("native host check", item => item.run?.trim() === 'test "$(uname -m)" = arm64')
  assert.equal(hostCheck.if, "matrix.platform == 'macos-latest'", `${name}: native host check must cover the macOS build`)
  assert.equal(hostCheck["continue-on-error"], undefined, `${name}: native host check must fail the job`)

  assert.equal(unconditional("Node.js setup", item => item.uses?.startsWith("actions/setup-node@")).with?.["node-version"], "24.21.0", `${name}: Node.js must use the pinned runtime`)
  assert.equal(unconditional("Bun setup", item => item.uses?.startsWith("oven-sh/setup-bun@")).with?.["bun-version"], "1.4.2", `${name}: Bun must use the pinned build tool`)
  unconditional("Bun revision check", item => item.run?.trim() === 'test "$(bun --revision)" = "1.4.2+744846f84"')
}
console.log("Workflow build job toolchains verified")
