import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { createHash } from "node:crypto"
import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

const script = fileURLToPath(import.meta.url)
const sha256 = bytes => createHash("sha256").update(bytes).digest("hex")

// A pinned source archive and a reviewed lock are needed: the upstream 0.23.0
// lock still contains rustix 1.1.5. Never fall back to an unlocked installation.
export function installCargoAudit(repo, run = spawnSync) {
  const pins = path.join(repo, "third-party/cargo-audit")
  const source = JSON.parse(fs.readFileSync(path.join(pins, "source.json"), "utf8"))
  assert.deepEqual(Object.keys(source).sort(), ["archiveSha256", "version"])
  assert.match(source.version, /^\d+\.\d+\.\d+$/)
  assert.match(source.archiveSha256, /^[a-f0-9]{64}$/)
  const lock = fs.readFileSync(path.join(pins, "Cargo.lock"))
  const fingerprint = sha256(JSON.stringify(source) + sha256(lock) + sha256(fs.readFileSync(script)) + process.platform + process.arch)
  const target = path.join(repo, "src-tauri/target")
  const cache = path.join(target, `cargo-audit-${source.version}-${fingerprint}`)
  const binary = path.join(cache, "bin/cargo-audit")
  const invoke = (command, args, cwd = repo) => {
    const result = run(command, args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"], maxBuffer: 16 * 1024 * 1024 })
    if (result.error || result.status !== 0) throw new Error(`${command} failed: ${result.error?.message ?? result.status}`)
    return result.stdout ?? ""
  }
  const verifyVersion = filename => assert.equal(invoke(filename, ["--version"]).trim(), `cargo-audit ${source.version}`, "Unexpected auditor version")

  // rust-cache can leave a partial tree. Reuse only an intact recorded binary,
  // bound to this source, lock, installer and host, never just its version.
  try {
    const receipt = JSON.parse(fs.readFileSync(path.join(cache, "receipt.json"), "utf8"))
    if (receipt.fingerprint === fingerprint && receipt.binarySha256 === sha256(fs.readFileSync(binary))) {
      verifyVersion(binary)
      return binary
    }
  } catch { /* Rebuild incomplete or stale tool caches. */ }

  fs.mkdirSync(target, { recursive: true })
  const stage = fs.mkdtempSync(path.join(target, ".cargo-audit-install-"))
  try {
    const archive = path.join(stage, "source.crate")
    invoke("curl", ["--proto", "=https", "--proto-redir", "=https", "--fail", "--silent", "--show-error", "--location", "--retry", "3", "--output", archive,
      `https://static.crates.io/crates/cargo-audit/cargo-audit-${source.version}.crate`])
    assert.equal(sha256(fs.readFileSync(archive)), source.archiveSha256, "Auditor source archive checksum mismatch")
    invoke("tar", ["-xzf", archive, "-C", stage])
    const sourceDir = path.join(stage, `cargo-audit-${source.version}`)
    const installed = path.join(stage, "installed")
    fs.writeFileSync(path.join(sourceDir, "Cargo.lock"), lock)
    const output = invoke("cargo", ["install", "--path", sourceDir, "--locked", "--root", installed], sourceDir)
    if (output) process.stderr.write(output)
    assert.deepEqual(fs.readFileSync(path.join(sourceDir, "Cargo.lock")), lock, "Auditor build changed the pinned lock")
    const candidate = path.join(installed, "bin/cargo-audit")
    verifyVersion(candidate)
    fs.writeFileSync(path.join(installed, "receipt.json"), JSON.stringify({ fingerprint, binarySha256: sha256(fs.readFileSync(candidate)) }) + "\n")
    fs.rmSync(cache, { recursive: true, force: true })
    fs.renameSync(installed, cache)
    return binary
  } finally {
    fs.rmSync(stage, { recursive: true, force: true })
  }
}

if (process.argv[1] && fs.realpathSync(process.argv[1]) === fs.realpathSync(script)) {
  console.log(installCargoAudit(path.resolve(path.dirname(script), "..")))
}
