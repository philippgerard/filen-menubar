import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { createHash } from "node:crypto"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import { test } from "node:test"
import { installCargoAudit } from "./install-cargo-audit.mjs"

function fixture(t) {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), "auditor-install-"))
  t.after(() => fs.rmSync(repo, { recursive: true, force: true }))
  const pins = path.join(repo, "third-party/cargo-audit")
  const upstream = path.join(repo, "upstream/cargo-audit-0.23.0")
  fs.mkdirSync(pins, { recursive: true })
  fs.mkdirSync(upstream, { recursive: true })
  fs.writeFileSync(path.join(upstream, "Cargo.lock"), "upstream lock\n")
  fs.writeFileSync(path.join(pins, "Cargo.lock"), "reviewed lock\n")
  const archive = path.join(repo, "source.crate")
  assert.equal(spawnSync("tar", ["-czf", archive, "-C", path.dirname(upstream), "cargo-audit-0.23.0"]).status, 0)
  const source = { version: "0.23.0", archiveSha256: createHash("sha256").update(fs.readFileSync(archive)).digest("hex") }
  fs.writeFileSync(path.join(pins, "source.json"), JSON.stringify(source))
  const state = { downloads: 0, builds: 0, failBuild: false, wrongVersion: false }
  const run = (command, args, options) => {
    if (command === "curl") {
      state.downloads++
      fs.copyFileSync(archive, args[args.indexOf("--output") + 1])
      return { status: 0, stdout: "" }
    }
    if (command === "cargo") {
      state.builds++
      // Model the installer's dependency-resolution contract: an unlocked
      // build or the archive's original lock cannot produce the reviewed tool.
      assert.ok(args.includes("--locked"))
      assert.equal(fs.readFileSync(path.join(options.cwd, "Cargo.lock"), "utf8"), fs.readFileSync(path.join(pins, "Cargo.lock"), "utf8"))
      const bin = path.join(args[args.indexOf("--root") + 1], "bin")
      fs.mkdirSync(bin, { recursive: true })
      fs.writeFileSync(path.join(bin, "cargo-audit"), `#!${process.execPath}\nconsole.log("cargo-audit ${state.wrongVersion ? "0.22.2" : "0.23.0"}")\n`, { mode: 0o755 })
      return { status: state.failBuild ? 1 : 0, stdout: "" }
    }
    return spawnSync(command, args, options)
  }
  return { repo, pins, archive, source, state, run }
}

function assertNoPublishedTool(repo) {
  const target = path.join(repo, "src-tauri/target")
  assert.deepEqual(fs.existsSync(target) ? fs.readdirSync(target) : [], [])
}

test("installs the reviewed graph and reuses only an intact cached tool", t => {
  const f = fixture(t)
  const first = installCargoAudit(f.repo, f.run)
  assert.equal(spawnSync(first, ["--version"], { encoding: "utf8" }).stdout.trim(), "cargo-audit 0.23.0")
  assert.equal(installCargoAudit(f.repo, f.run), first)
  assert.equal(f.state.downloads, 1)
  assert.equal(f.state.builds, 1)
  const built = fs.readFileSync(first, "utf8")
  fs.appendFileSync(first, "// changed cached bytes\n")
  assert.equal(spawnSync(first, ["--version"], { encoding: "utf8" }).stdout.trim(), "cargo-audit 0.23.0")
  assert.equal(installCargoAudit(f.repo, f.run), first)
  assert.equal(f.state.builds, 2)
  assert.equal(fs.readFileSync(first, "utf8"), built)
})

test("a reviewed lock change gets a separate installation even at the same version", t => {
  const f = fixture(t)
  const first = installCargoAudit(f.repo, f.run)
  fs.writeFileSync(path.join(f.pins, "Cargo.lock"), "new reviewed lock\n")
  const second = installCargoAudit(f.repo, f.run)
  assert.notEqual(second, first)
  assert.equal(f.state.builds, 2)
  assert.ok(fs.existsSync(first))
})

test("an archive checksum mismatch cannot execute a build or publish a tool", t => {
  const f = fixture(t)
  fs.appendFileSync(f.archive, "corruption")
  assert.throws(() => installCargoAudit(f.repo, f.run), /archive checksum mismatch/)
  assert.equal(f.state.builds, 0)
  assertNoPublishedTool(f.repo)
})

test("a partial failed build cannot become a reusable installation", t => {
  const f = fixture(t)
  f.state.failBuild = true
  assert.throws(() => installCargoAudit(f.repo, f.run), /cargo failed/)
  assertNoPublishedTool(f.repo)
  f.state.failBuild = false
  assert.ok(fs.existsSync(installCargoAudit(f.repo, f.run)))
  assert.equal(f.state.builds, 2)
})

test("a wrong executable version fails before publication", t => {
  const f = fixture(t)
  f.state.wrongVersion = true
  assert.throws(() => installCargoAudit(f.repo, f.run), /Unexpected auditor version/)
  assertNoPublishedTool(f.repo)
})

test("malformed source pins fail before downloading", t => {
  const f = fixture(t)
  fs.writeFileSync(path.join(f.pins, "source.json"), JSON.stringify({ ...f.source, version: "../../other" }))
  assert.throws(() => installCargoAudit(f.repo, f.run))
  assert.equal(f.state.downloads, 0)
  assertNoPublishedTool(f.repo)
})
