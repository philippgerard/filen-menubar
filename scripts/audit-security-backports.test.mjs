import assert from "node:assert/strict"
import { test } from "node:test"
import { spawnSync } from "node:child_process"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { assessAudit, treeHash, validateBackportManifest, verifyPatchedPackages } from "./audit-security-backports.mjs"

const manifest = [
  { package: "braces", version: "3.0.3", advisory: "GHSA-vfj7-8cjw-p6xm", treeSha256: "0".repeat(64) },
]
const advisory = (id, severity = "high") => ({ url: `https://github.com/advisories/${id}`, severity, vulnerable_versions: "*" })

test("only exact package/advisory pairs qualify; other high and critical findings fail", () => {
  assert.equal(assessAudit({ braces: [advisory(manifest[0].advisory)] }, manifest).length, 1)
  for (const report of [{ braces: [advisory("GHSA-new-advisory")] }, { other: [advisory(manifest[0].advisory)] }, { braces: [advisory("GHSA-new-advisory", "critical")] }]) {
    assert.throws(() => assessAudit(report, manifest), /Unpatched/)
  }
  assert.equal(assessAudit({ other: [advisory("GHSA-low", "moderate")] }, manifest).length, 1)
})

test("invalid audit output fails closed", () => {
  for (const report of [null, [], { braces: {} }, { braces: [{}] }, { braces: [advisory("x", "unknown")] }]) {
    assert.throws(() => assessAudit(report, manifest), /Invalid/)
  }
})

test("retired forge backport does not exempt either high or critical findings", () => {
  for (const severity of ["high", "critical"]) {
    assert.throws(() => assessAudit({ "node-forge": [advisory("GHSA-86w9-cpqp-85rv", severity)] }, manifest), /Unpatched/)
  }
})

test("only one exact braces identity with a valid checksum can authorize a backport", () => {
  validateBackportManifest(manifest)
  const invalid = [null, {}, [], [null], [manifest[0], manifest[0]]]
  for (const change of [
    { package: "node-forge", version: "1.4.0", advisory: "GHSA-86w9-cpqp-85rv" },
    { package: "other" }, { version: "3.0.4" }, { advisory: "GHSA-new-advisory" },
    { treeSha256: undefined }, { treeSha256: 1 }, { treeSha256: "0".repeat(63) }, { treeSha256: "g".repeat(64) },
  ]) invalid.push([{ ...manifest[0], ...change }])
  for (const candidate of invalid) {
    assert.throws(() => validateBackportManifest(candidate), /Invalid security backport manifest/)
    assert.throws(() => assessAudit({}, candidate), /Invalid security backport manifest/)
    assert.throws(() => verifyPatchedPackages("unused", candidate), /Invalid security backport manifest/)
  }
})

test("all installed copies must have exact source bytes and version", t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "backport-gate-"))
  t.after(() => fs.rmSync(root, { recursive: true, force: true }))
  const fixture = manifest.map(patch => {
    const dir = path.join(root, "node_modules", patch.package)
    fs.mkdirSync(dir, { recursive: true })
    fs.writeFileSync(path.join(dir, "package.json"), JSON.stringify({ name: patch.package, version: patch.version }))
    fs.writeFileSync(path.join(dir, "index.js"), "verified fixture\n")
    return { ...patch, treeSha256: treeHash(dir) }
  })
  verifyPatchedPackages(root, fixture)
  const nested = path.join(root, "node_modules", "consumer", "node_modules", "braces")
  fs.mkdirSync(nested, { recursive: true })
  fs.writeFileSync(path.join(root, "node_modules", "consumer", "package.json"), '{"name":"consumer"}')
  fs.cpSync(path.join(root, "node_modules", "braces"), nested, { recursive: true })
  verifyPatchedPackages(root, fixture)
  fs.appendFileSync(path.join(nested, "index.js"), "changed")
  assert.throws(() => verifyPatchedPackages(root, fixture), /Unverified/)
  fs.rmSync(nested, { recursive: true })
  const metadata = path.join(root, "node_modules", "braces", "package.json")
  fs.writeFileSync(metadata, JSON.stringify({ name: "braces", version: "3.0.4" }))
  const changedVersion = [{ ...fixture[0], treeSha256: treeHash(path.dirname(metadata)) }]
  assert.throws(() => verifyPatchedPackages(root, changedVersion), /Unverified/)
  fs.writeFileSync(metadata, JSON.stringify({ name: "braces", version: "3.0.3" }))
  fs.symlinkSync("index.js", path.join(root, "node_modules", "braces", "unexpected-link"))
  assert.throws(() => verifyPatchedPackages(root, fixture), /Unsupported/)
})

test("gates fail closed when invoked through a symlinked checkout", t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "backport-gate-"))
  t.after(() => fs.rmSync(root, { recursive: true, force: true }))
  fs.symlinkSync(path.dirname(fileURLToPath(import.meta.url)), path.join(root, "scripts"))
  fs.mkdirSync(path.join(root, "source", "node_modules"), { recursive: true })
  fs.writeFileSync(path.join(root, "manifest.json"), JSON.stringify(manifest))
  const run = (script, ...args) => spawnSync(process.execPath, [path.join(root, "scripts", script), ...args], { encoding: "utf8" })
  let result = run("audit-security-backports.mjs", path.join(root, "source"), path.join(root, "manifest.json"), path.join(root, "missing-bun"))
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, /Missing patched dependency: braces/)
  result = run("check-security-backports.mjs", path.join(root, "source"))
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, /Cannot find module 'braces'/)
})
