import assert from "node:assert/strict"
import path from "node:path"
import os from "node:os"
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, symlinkSync, rmSync } from "node:fs"
import { spawnSync } from "node:child_process"
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

  const cargoCache = unconditional("Cargo cache", item => item.uses?.startsWith("swatinem/rust-cache@"))
  const cleanup = unconditional("backend cache recovery", item => item.name === "Clear restored backend build inputs")
  const checks = unconditional("repository checks", item => item.run?.trim() === "npm run check")
  assert.ok(job.steps.indexOf(cargoCache) < job.steps.indexOf(cleanup), `${name}: backend recovery must follow cache restoration`)
  assert.ok(job.steps.indexOf(cleanup) < job.steps.indexOf(checks), `${name}: backend recovery must precede repository checks`)
  assert.equal(cleanup.shell, "bash", `${name}: recovery must use the tested shell`)

  // Execute the workflow's command against a partially restored cache. Cargo
  // artifacts and the checked-out application must survive the recovery.
  const fixture = mkdtempSync(path.join(os.tmpdir(), "filen-workflow-cache-"))
  try {
    const checkout = path.join(fixture, "checkout")
    const backend = path.join(checkout, "src-tauri/target/filen-cli-build")
    const runtime = path.join(checkout, "src-tauri/target/node-runtime-cache")
    const survivors = [
      "src-tauri/target/debug/deps/libcached.rlib",
      "src-tauri/target/debug/build/cached/output",
      "src-tauri/target/debug/.fingerprint/cached/invoked.timestamp",
      "src-tauri/target/aarch64-apple-darwin/release/deps/libcached.rlib",
      "src-tauri/src/lib.rs",
      "src-tauri/target/unrelated/keep",
    ]
    mkdirSync(path.join(backend, "fingerprint/filen-sync/.git"), { recursive: true })
    writeFileSync(path.join(backend, "fingerprint/filen-sync/.git/HEAD"), "pinned commit\n")
    mkdirSync(runtime, { recursive: true })
    writeFileSync(path.join(runtime, "partial-runtime"), "incomplete")
    for (const relative of survivors) {
      const file = path.join(checkout, relative)
      mkdirSync(path.dirname(file), { recursive: true })
      writeFileSync(file, "keep")
    }
    const outside = path.join(fixture, "outside")
    writeFileSync(outside, "keep")
    symlinkSync(outside, path.join(backend, "external-link"))
    for (let attempt = 0; attempt < 2; attempt++) {
      const result = spawnSync("bash", ["-euo", "pipefail", "-c", cleanup.run], { cwd: checkout, encoding: "utf8" })
      assert.equal(result.status, 0, `${name}: cache recovery failed: ${result.stderr}`)
      assert.equal(existsSync(backend), false, `${name}: incomplete backend checkout survived`)
      assert.equal(existsSync(runtime), false, `${name}: incomplete Node runtime survived`)
      for (const relative of survivors) assert.ok(existsSync(path.join(checkout, relative)), `${name}: recovery removed ${relative}`)
      assert.ok(existsSync(outside), `${name}: recovery removed unrelated files`)
    }
  } finally {
    rmSync(fixture, { recursive: true, force: true })
  }
}
console.log("Workflow build job toolchains verified")
