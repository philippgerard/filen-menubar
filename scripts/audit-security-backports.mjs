import { createHash } from "node:crypto"
import fs from "node:fs"
import path from "node:path"
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import { checkSecurityBackports } from "./check-security-backports.mjs"

// Hash paths as well as bytes. Refuse symlinks and unexpected package content.
export function treeHash(directory) {
  const hash = createHash("sha256")
  const visit = (dir, prefix = "") => {
    for (const name of fs.readdirSync(dir).sort()) {
      const filename = path.join(dir, name)
      const relative = prefix + name
      const stat = fs.lstatSync(filename)
      if (stat.isDirectory()) visit(filename, relative + "/")
      else if (stat.isFile()) hash.update(relative + "\0").update(fs.readFileSync(filename)).update("\0")
      else throw new Error(`Unsupported entry in patched dependency: ${relative}`)
    }
  }
  visit(directory)
  return hash.digest("hex")
}

export function verifyPatchedPackages(sourceDir, manifest) {
  if (!Array.isArray(manifest) || manifest.length !== 2) throw new Error("Invalid security backport manifest")
  const found = new Set()
  const visitModules = directory => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      if (entry.name.startsWith(".")) continue
      const filename = path.join(directory, entry.name)
      if (!entry.isDirectory()) {
        if (entry.isSymbolicLink()) throw new Error(`Unexpected dependency symlink: ${entry.name}`)
        continue
      }
      if (entry.name.startsWith("@")) { visitModules(filename); continue }
      const metadataFile = path.join(filename, "package.json")
      if (!fs.existsSync(metadataFile)) continue
      const metadata = JSON.parse(fs.readFileSync(metadataFile, "utf8"))
      const patch = manifest.find(item => item.package === metadata.name)
      if (patch) {
        if (metadata.version !== patch.version || treeHash(filename) !== patch.treeSha256) {
          throw new Error(`Unverified security backport: ${metadata.name}@${metadata.version}`)
        }
        found.add(patch.package)
      }
      const nested = path.join(filename, "node_modules")
      if (fs.existsSync(nested)) visitModules(nested)
    }
  }
  visitModules(path.join(sourceDir, "node_modules"))
  for (const patch of manifest) if (!found.has(patch.package)) throw new Error(`Missing patched dependency: ${patch.package}`)
}

export function assessAudit(report, manifest) {
  if (!report || typeof report !== "object" || Array.isArray(report)) throw new Error("Invalid Bun audit response")
  const results = []
  for (const [name, advisories] of Object.entries(report)) {
    if (!Array.isArray(advisories)) throw new Error("Invalid Bun audit advisories")
    for (const advisory of advisories) {
      if (!["low", "moderate", "high", "critical"].includes(advisory.severity) || typeof advisory.url !== "string" || typeof advisory.vulnerable_versions !== "string") {
        throw new Error("Invalid Bun audit advisory")
      }
      const patch = manifest.find(item => item.package === name && advisory.url === `https://github.com/advisories/${item.advisory}`)
      if (["high", "critical"].includes(advisory.severity) && !patch) {
        throw new Error(`Unpatched ${advisory.severity} advisory: ${name} ${advisory.url}`)
      }
      results.push(`${name}: ${advisory.url} (${patch ? "verified source backport" : advisory.severity})`)
    }
  }
  return results
}

export function auditSecurityBackports(sourceDir, manifestFile, bunBin) {
  const manifest = JSON.parse(fs.readFileSync(manifestFile, "utf8"))
  verifyPatchedPackages(sourceDir, manifest)
  checkSecurityBackports(sourceDir)
  const result = spawnSync(bunBin, ["audit", "--prod", "--json"], { cwd: sourceDir, encoding: "utf8", timeout: 120000, maxBuffer: 4 * 1024 * 1024 })
  if (result.error || ![0, 1].includes(result.status)) throw new Error(`Bun audit failed: ${result.error?.message ?? result.stderr}`)
  const report = JSON.parse(result.stdout)
  // A failing process with an empty report cannot be considered a clean audit.
  if (result.status === 1 && Object.keys(report).length === 0) throw new Error("Bun audit failed without advisories")
  for (const line of assessAudit(report, manifest)) console.log(line)
  console.log("Production audit passed with checksum-verified security backports")
}

if (process.argv[1] && fs.realpathSync(process.argv[1]) === fs.realpathSync(fileURLToPath(import.meta.url))) {
  auditSecurityBackports(path.resolve(process.argv[2]), path.resolve(process.argv[3]), process.argv[4] ?? "bun")
}
