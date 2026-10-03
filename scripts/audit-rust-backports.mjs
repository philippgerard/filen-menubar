import assert from "node:assert/strict"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"

// cargo-audit skips path dependencies. Restore their original registry identity
// in a disposable audit input, while retaining the application's actual graph.
export function registryAuditLock(lock, records) {
  return lock.split("[[package]]").map((record, index) => {
    if (index === 0) return record
    const name = /^name = "([^"]+)"$/m.exec(record)?.[1]
    const backport = Object.hasOwn(records, name) ? records[name] : undefined
    if (!backport) return record
    assert.ok(!/^source =/m.test(record), `Expected vendored source: ${name}`)
    assert.match(backport.archive_sha256, /^[a-f0-9]{64}$/)
    return record.replace(/^(version = "[^"]+")$/m, `$1\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "${backport.archive_sha256}"`)
  }).join("[[package]]")
}

export function assessRustAudit(report) {
  assert.ok(report && typeof report === "object" && !Array.isArray(report), "Invalid Rust audit report")
  assert.ok(Array.isArray(report.settings?.ignore) && report.settings.ignore.length === 0, "Rust audit must not ignore advisories")
  assert.deepEqual(report.settings.target_arch, [], "Rust audit must cover all target architectures")
  assert.deepEqual(report.settings.target_os, [], "Rust audit must cover all target platforms")
  assert.equal(report.settings.severity, null, "Rust audit must cover all vulnerability severities")
  assert.deepEqual([...report.settings.informational_warnings].sort(), ["notice", "unmaintained", "unsound"], "Rust audit must include all advisory warnings")
  assert.ok(Array.isArray(report.vulnerabilities?.list), "Invalid Rust vulnerability list")
  assert.equal(report.vulnerabilities.count, report.vulnerabilities.list.length, "Invalid Rust vulnerability count")
  assert.equal(report.vulnerabilities.found, report.vulnerabilities.list.length !== 0, "Invalid Rust vulnerability status")
  assert.ok(report.warnings && typeof report.warnings === "object" && !Array.isArray(report.warnings), "Invalid Rust warning list")
  assert.equal(report.vulnerabilities.list.length, 0, "Unpatched Rust vulnerability")
  const verified = []
  for (const [kind, warnings] of Object.entries(report.warnings)) {
    assert.ok(Array.isArray(warnings), "Invalid Rust warning entries")
    for (const warning of warnings) {
      const fixed = kind === "unsound" && warning.advisory?.id === "RUSTSEC-2024-0429" && warning.advisory?.package === "glib" && warning.package?.name === "glib" && warning.package?.version === "0.18.5"
      assert.ok(fixed, `Unpatched Rust warning: ${warning.advisory?.id ?? kind}`)
      verified.push("glib@0.18.5: RUSTSEC-2024-0429 (verified source backport)")
    }
  }
  return verified
}

function auditRustBackports(repo, auditBin) {
  const sourceCheck = spawnSync(process.execPath, [path.join(repo, "scripts/check-rust-backports.mjs")], { encoding: "utf8" })
  if (sourceCheck.status !== 0 || sourceCheck.error) throw new Error(`Rust source verification failed: ${sourceCheck.stderr}`)
  const records = JSON.parse(fs.readFileSync(path.join(repo, "third-party/rust/SOURCE-HASHES.json"), "utf8")).crates
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "filen-rust-audit-"))
  try {
    const filename = path.join(directory, "Cargo.lock")
    fs.writeFileSync(filename, registryAuditLock(fs.readFileSync(path.join(repo, "src-tauri/Cargo.lock"), "utf8"), records))
    const result = spawnSync(auditBin, ["audit", "--file", filename, "--json", "--deny", "warnings"], { encoding: "utf8", timeout: 120000, maxBuffer: 4 * 1024 * 1024, env: { ...process.env, GIT_CONFIG_GLOBAL: "/dev/null" } })
    if (result.error || ![0, 1].includes(result.status)) throw new Error(`Rust audit failed: ${result.error?.message ?? result.stderr}`)
    const lines = assessRustAudit(JSON.parse(result.stdout))
    if (result.status === 1 && lines.length === 0) throw new Error("Rust audit failed without a verified backport finding")
    for (const line of lines) console.log(line)
    console.log("Rust audit passed, including registry advisories for all vendored crates")
  } finally {
    fs.rmSync(directory, { recursive: true, force: true })
  }
}

if (process.argv[1] && fs.realpathSync(process.argv[1]) === fs.realpathSync(fileURLToPath(import.meta.url))) {
  auditRustBackports(path.resolve(path.dirname(fileURLToPath(import.meta.url)), ".."), path.resolve(process.argv[2]))
}
