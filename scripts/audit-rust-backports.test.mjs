import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import { test } from "node:test"
import { fileURLToPath } from "node:url"
import { assessRustAudit, registryAuditLock } from "./audit-rust-backports.mjs"

const report = warnings => ({ settings: { ignore: [], target_arch: [], target_os: [], severity: null, informational_warnings: ["unmaintained", "unsound", "notice"] }, vulnerabilities: { found: false, count: 0, list: [] }, warnings })
const fixed = { advisory: { id: "RUSTSEC-2024-0429", package: "glib" }, package: { name: "glib", version: "0.18.5" } }

test("registry audit input preserves the graph and restores only vendored identities", () => {
  const lock = 'version = 4\n[[package]]\nname = "glib"\nversion = "0.18.5"\ndependencies = ["glib-macros"]\n[[package]]\nname = "other"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n'
  const result = registryAuditLock(lock, { glib: { archive_sha256: "a".repeat(64) } })
  assert.match(result, /name = "glib"\nversion = "0.18.5"\nsource = "registry\+/)
  assert.match(result, /dependencies = \["glib-macros"\]/)
  assert.equal(result.split('name = "other"')[1], lock.split('name = "other"')[1])
  assert.throws(() => registryAuditLock(result, { glib: { archive_sha256: "a".repeat(64) } }), /Expected vendored/)
})

test("only the verified GLib unsoundness advisory qualifies", () => {
  assert.equal(assessRustAudit(report({ unsound: [fixed] })).length, 1)
  for (const warnings of [{ unsound: [{ ...fixed, advisory: { id: "RUSTSEC-FUTURE", package: "glib" } }] }, { unmaintained: [fixed] }, { unsound: [{ ...fixed, package: { name: "glib", version: "0.18.4" } }] }]) {
    assert.throws(() => assessRustAudit(report(warnings)), /Unpatched/)
  }
  const vulnerability = report({})
  vulnerability.vulnerabilities = { found: true, count: 1, list: [fixed] }
  assert.throws(() => assessRustAudit(vulnerability), /Unpatched/)
})

test("malformed audit reports and ignored advisories fail closed", () => {
  for (const invalid of [null, [], {}, { ...report({}), settings: { ...report({}).settings, ignore: ["RUSTSEC-FUTURE"] } }, { ...report({}), settings: { ...report({}).settings, informational_warnings: [] } }, { ...report({}), settings: { ...report({}).settings, target_os: ["macos"] } }, report({ unsound: {} })]) {
    assert.throws(() => assessRustAudit(invalid))
  }
})

test("rustix xattr unsoundness is rejected for both affected lockfile versions", () => {
  for (const version of ["1.1.4", "1.1.5"]) {
    const warning = { advisory: { id: "RUSTSEC-2026-0486", package: "rustix" }, package: { name: "rustix", version } }
    assert.throws(() => assessRustAudit(report({ unsound: [warning] })), /Unpatched Rust warning: RUSTSEC-2026-0486/)
  }
})

test("only exact pending ksni maintenance notices qualify, never vulnerabilities", () => {
  for (const [name, version, id] of [["zvariant", "5.15.1", "RUSTSEC-2026-0370"], ["zbus_names", "4.3.5", "RUSTSEC-2026-0371"], ["zvariant_derive", "5.15.1", "RUSTSEC-2026-0372"]]) {
    const notice = { advisory: { id, package: name }, package: { name, version } }
    assert.match(assessRustAudit(report({ unmaintained: [notice] }))[0], /UNRESOLVED maintenance/)
    for (const warnings of [
      { unsound: [notice] }, { notice: [notice] },
      { unmaintained: [{ ...notice, package: { name, version: "99.0.0" } }] },
      { unmaintained: [{ ...notice, advisory: { id: "RUSTSEC-FUTURE", package: name } }] },
      { unmaintained: [{ ...notice, advisory: { id, package: "other" } }] },
    ]) assert.throws(() => assessRustAudit(report(warnings)), /Unpatched/)
    const vulnerability = report({})
    vulnerability.vulnerabilities = { found: true, count: 1, list: [notice] }
    assert.throws(() => assessRustAudit(vulnerability), /Unpatched/)
  }
})

test("the audit fails closed when invoked through a symlinked checkout", t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "rust-audit-gate-"))
  t.after(() => fs.rmSync(root, { recursive: true, force: true }))
  fs.symlinkSync(path.dirname(fileURLToPath(import.meta.url)), path.join(root, "scripts"))
  const result = spawnSync(process.execPath, [path.join(root, "scripts/audit-rust-backports.mjs"), path.join(root, "missing-cargo-audit")], { encoding: "utf8" })
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, /Rust audit failed/)
})
