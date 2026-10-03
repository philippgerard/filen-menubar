import assert from "node:assert/strict"
import { test } from "node:test"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import { assessAudit, treeHash, verifyPatchedPackages } from "./audit-security-backports.mjs"

const manifest = [
  { package: "braces", version: "3.0.3", advisory: "GHSA-vfj7-8cjw-p6xm" },
  { package: "node-forge", version: "1.4.0", advisory: "GHSA-86w9-cpqp-85rv" },
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
  fs.symlinkSync("index.js", path.join(root, "node_modules", "braces", "unexpected-link"))
  assert.throws(() => verifyPatchedPackages(root, fixture), /Unsupported/)
})
