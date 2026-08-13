#!/usr/bin/env node

import { createHash } from "node:crypto"
import { cp, mkdir, readFile, readdir, writeFile } from "node:fs/promises"
import path from "node:path"
import process from "node:process"
import { discoverLicenseEvidence, requireSubstantiveLicenseText } from "./license-evidence.mjs"

const [metadataArg, vendorArg, lockArg, outputArg, supplementsArg] = process.argv.slice(2)
if (!metadataArg || !vendorArg || !lockArg || !outputArg || !supplementsArg) {
  console.error("usage: generate-keyring-compliance.mjs <cargo-metadata.json> <vendor-dir> <Cargo.lock> <output-dir> <supplements.json>")
  process.exit(2)
}

const metadata = JSON.parse(await readFile(path.resolve(metadataArg), "utf8"))
const vendor = path.resolve(vendorArg)
const output = path.resolve(outputArg)
const sourceOutput = path.join(output, "corresponding-source", "cargo-vendor")
const supplementsFile = path.resolve(supplementsArg)
const supplementsDirectory = path.dirname(supplementsFile)
const supplements = JSON.parse(await readFile(supplementsFile, "utf8"))
const usedSupplements = new Set()
const sha256 = value => createHash("sha256").update(value).digest("hex")
const vendorPackages = new Map()
for (const directoryName of await readdir(vendor)) {
  const directory = path.join(vendor, directoryName)
  let manifest
  try {
    manifest = await readFile(path.join(directory, "Cargo.toml"), "utf8")
  } catch (error) {
    if (error.code === "ENOENT") continue
    throw error
  }
  const packageBlock = manifest.match(/\[package\]([\s\S]*?)(?:\n\[|$)/)?.[1] ?? ""
  const name = packageBlock.match(/^name\s*=\s*"([^"]+)"/m)?.[1]
  const version = packageBlock.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
  if (!name || !version) throw new Error(`cannot identify vendored crate: ${directory}`)
  const key = `${name}@${version}`
  if (vendorPackages.has(key)) throw new Error(`duplicate vendored crate: ${key}`)
  vendorPackages.set(key, directory)
}

const packages = metadata.packages
  .filter(item => item.source?.startsWith("registry+") || item.name === "node_keyring")
  .sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`, "en"))

if (packages.length < 150) throw new Error(`implausibly small Cargo graph: ${packages.length} packages`)
const seen = new Set()
const notices = []
const components = []
for (const item of packages) {
  const key = `${item.name}@${item.version}`
  if (seen.has(key)) continue
  seen.add(key)
  const directory = item.name === "node_keyring"
    ? path.dirname(item.manifest_path)
    : vendorPackages.get(key)
  if (!directory) throw new Error(`locked crate is absent from cargo vendor output: ${key}`)
  let evidence = await discoverLicenseEvidence(directory)
  if (evidence.length === 0) {
    const supplement = supplements[key]
    if (!supplement) throw new Error(`crate has no substantive license text or pinned supplement: ${key}`)
    const supplementFile = path.resolve(supplementsDirectory, supplement.file)
    const text = await readFile(supplementFile, "utf8")
    if (sha256(text) !== supplement.sha256) throw new Error(`supplement SHA-256 mismatch: ${key}`)
    evidence = [{
      source: `${supplement.file} (${supplement.source})`,
      text: requireSubstantiveLicenseText(text, supplementFile),
    }]
    await cp(supplementFile, path.join(directory, path.basename(supplement.file)))
    usedSupplements.add(key)
  }
  notices.push([
    key,
    `Declared license: ${item.license ?? "see included license text"}`,
    ...evidence.map(item => `--- ${item.source} ---\n${item.text}`),
  ].join("\n"))
  components.push({
    type: "library",
    name: item.name,
    version: item.version,
    purl: `pkg:cargo/${encodeURIComponent(item.name)}@${item.version}`,
    licenses: item.license ? [{ license: { name: item.license } }] : undefined,
  })
}

const lockText = await readFile(path.resolve(lockArg), "utf8")
const locked = new Set()
for (const block of lockText.split("[[package]]").slice(1)) {
  const name = block.match(/^name\s*=\s*"([^"]+)"/m)?.[1]
  const version = block.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
  if (name && version) locked.add(`${name}@${version}`)
}
const missing = [...locked].filter(key => !seen.has(key))
if (missing.length > 0) throw new Error(`Cargo.lock packages missing from compliance inventory: ${missing.join(", ")}`)
const unexpected = [...seen].filter(key => !locked.has(key))
if (unexpected.length > 0) throw new Error(`compliance inventory packages missing from Cargo.lock: ${unexpected.join(", ")}`)
const unusedSupplements = Object.keys(supplements).filter(key => !usedSupplements.has(key))
if (unusedSupplements.length > 0) throw new Error(`unused Cargo license supplements: ${unusedSupplements.join(", ")}`)

await mkdir(output, { recursive: true })
await cp(vendor, sourceOutput, { recursive: true })
await writeFile(path.join(output, "cargo-components.json"), JSON.stringify(components, null, 2) + "\n")
await writeFile(path.join(output, "CARGO_THIRD_PARTY_NOTICES.txt"), notices.join("\n\n================================================================================\n\n") + "\n")
await writeFile(path.join(output, "cargo-packages.txt"), [...seen].join("\n") + "\n")
console.log(`generated native compliance inventory for ${seen.size} crates`)
