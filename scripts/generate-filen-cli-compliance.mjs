#!/usr/bin/env node

import { createHash } from "node:crypto"
import { cp, mkdir, readFile, readdir, writeFile } from "node:fs/promises"
import path from "node:path"
import process from "node:process"
import packageUrl from "packageurl-js"
import { discoverLicenseEvidence } from "./license-evidence.mjs"

const [
  sourceArg,
  metaArg,
  outputArg,
  helperVersion,
  nodeBinaryArg,
  nodeVersion,
  nodePlatform,
  nodeArchiveSha256,
  nodeSourceArchiveArg,
  nodeSourceSha256,
  cargoComponentsArg,
  cargoNoticesArg,
] = process.argv.slice(2)

if ([sourceArg, metaArg, outputArg, helperVersion, nodeBinaryArg, nodeVersion, nodePlatform,
  nodeArchiveSha256, nodeSourceArchiveArg, nodeSourceSha256, cargoComponentsArg, cargoNoticesArg]
  .some(value => !value)) {
  console.error("usage: generate-filen-cli-compliance.mjs <patched-source> <esbuild-metafile> <output-dir> <helper-version> <node-binary> <node-version> <node-platform> <node-archive-sha256> <node-source-archive> <node-source-sha256> <cargo-components.json> <cargo-notices>")
  process.exit(2)
}

const { PackageURL } = packageUrl
const source = path.resolve(sourceArg)
const meta = JSON.parse(await readFile(path.resolve(metaArg), "utf8"))
const output = path.resolve(outputArg)
const runtimeSource = path.join(output, "corresponding-source", "runtime-packages")
const nodeSourceDirectory = path.join(output, "corresponding-source", "node")
const notices = []
const npmPackages = new Map()

const sha256 = value => createHash("sha256").update(value).digest("hex")
const uuidFromHash = hash => {
  const variant = ((Number.parseInt(hash[16], 16) & 0x3) | 0x8).toString(16)
  return `${hash.slice(0, 8)}-${hash.slice(8, 12)}-4${hash.slice(13, 16)}-${variant}${hash.slice(17, 20)}-${hash.slice(20, 32)}`
}
const npmPurl = (name, version) => {
  const match = /^@([^/]+)\/(.+)$/.exec(name)
  return new PackageURL("npm", match ? `@${match[1]}` : null, match?.[2] ?? name, version, null, null).toString()
}
const fileSha256 = async file => sha256(await readFile(file))

async function findPackageRoot(input) {
  let directory = path.dirname(path.resolve(source, input))
  while (directory.startsWith(source + path.sep)) {
    const manifest = path.join(directory, "package.json")
    try {
      const parsed = JSON.parse(await readFile(manifest, "utf8"))
      if (parsed.name && parsed.version) return { directory, manifest: parsed }
    } catch (error) {
      if (error.code !== "ENOENT") throw error
    }
    const parent = path.dirname(directory)
    if (parent === directory) break
    directory = parent
  }
  return null
}

for (const input of Object.keys(meta.inputs)) {
  if (!input.startsWith("node_modules/")) continue
  const found = await findPackageRoot(input)
  if (found) npmPackages.set(found.directory, found.manifest)
}

// esbuild deliberately leaves the app-owned native keyring loader external.
const keyringDirectory = path.join(source, "node_modules", "@jupiterpi", "node-keyring")
npmPackages.set(keyringDirectory, JSON.parse(await readFile(path.join(keyringDirectory, "package.json"), "utf8")))

const sortedNpm = [...npmPackages.entries()].sort((a, b) =>
  `${a[1].name}@${a[1].version}`.localeCompare(`${b[1].name}@${b[1].version}`, "en")
)
const uniqueNpm = []
const npmByPurl = new Map()
for (const entry of sortedNpm) {
  const [, manifest] = entry
  const purl = npmPurl(manifest.name, manifest.version)
  const existing = npmByPurl.get(purl)
  if (existing) {
    const existingManifest = JSON.stringify(existing[1])
    const duplicateManifest = JSON.stringify(manifest)
    if (existingManifest !== duplicateManifest) {
      throw new Error(`duplicate npm identity has non-identical package metadata: ${purl}`)
    }
    continue
  }
  npmByPurl.set(purl, entry)
  uniqueNpm.push(entry)
}
if (uniqueNpm.length < 100) throw new Error(`implausibly small runtime graph: ${uniqueNpm.length} npm packages`)

const npmComponents = []
for (const [directory, manifest] of uniqueNpm) {
  const licenseEvidence = await discoverLicenseEvidence(directory)
  if (licenseEvidence.length === 0) {
    throw new Error(`runtime package has no substantive license text: ${manifest.name}@${manifest.version}`)
  }

  const destination = path.join(runtimeSource, manifest.name, manifest.version)
  await cp(directory, destination, {
    recursive: true,
    filter: candidate => {
      const relative = path.relative(directory, candidate)
      return relative === "" || relative.split(path.sep)[0] !== "node_modules"
    },
  })
  notices.push([
    `${manifest.name}@${manifest.version}`,
    `Declared license: ${manifest.license ?? "see included license text"}`,
    ...licenseEvidence.map(item => `--- ${item.source} ---\n${item.text}`),
  ].join("\n"))
  const purl = npmPurl(manifest.name, manifest.version)
  npmComponents.push({
    type: "library",
    "bom-ref": purl,
    name: manifest.name,
    version: manifest.version,
    purl,
    licenses: manifest.license ? [{ license: { name: String(manifest.license) } }] : undefined,
    properties: [{ name: "filen-menubar:ecosystem", value: "npm" }],
  })
}

const cargoComponents = JSON.parse(await readFile(path.resolve(cargoComponentsArg), "utf8"))
if (!Array.isArray(cargoComponents) || cargoComponents.length < 150) {
  throw new Error(`implausibly small native runtime graph: ${cargoComponents.length} crates`)
}
for (const component of cargoComponents) {
  PackageURL.fromString(component.purl)
  component["bom-ref"] = component.purl
  component.properties = [{ name: "filen-menubar:ecosystem", value: "cargo" }]
}

const nodeBinary = path.resolve(nodeBinaryArg)
const nodeSourceArchive = path.resolve(nodeSourceArchiveArg)
const nodeBinarySha256 = await fileSha256(nodeBinary)
const nodeLicenseFile = path.join(path.dirname(path.dirname(nodeBinary)), "LICENSE")
const nodeLicenseText = await readFile(nodeLicenseFile, "utf8")
if (nodeLicenseText.trim().length < 10000) throw new Error("official Node license/notices are unexpectedly short")
if (await fileSha256(nodeSourceArchive) !== nodeSourceSha256) throw new Error("Node source archive SHA-256 mismatch")
await mkdir(nodeSourceDirectory, { recursive: true })
await cp(nodeSourceArchive, path.join(nodeSourceDirectory, path.basename(nodeSourceArchive)))

const nodePurl = new PackageURL("generic", null, "node", nodeVersion, null, null).toString()
const nodeAssetUrl = `https://nodejs.org/dist/v${nodeVersion}/node-v${nodeVersion}-${nodePlatform}.tar.gz`
const nodeSourceUrl = `https://nodejs.org/dist/v${nodeVersion}/node-v${nodeVersion}.tar.gz`
const nodeComponent = {
  type: "platform",
  "bom-ref": nodePurl,
  name: "Node.js",
  version: nodeVersion,
  purl: nodePurl,
  licenses: [{ license: { id: "MIT" } }],
  externalReferences: [
    { type: "distribution", url: nodeAssetUrl, hashes: [{ alg: "SHA-256", content: nodeArchiveSha256 }] },
    { type: "distribution", url: nodeSourceUrl, hashes: [{ alg: "SHA-256", content: nodeSourceSha256 }] },
  ],
  properties: [
    { name: "filen-menubar:ecosystem", value: "node-runtime" },
    { name: "filen-menubar:platform", value: nodePlatform },
  ],
}
notices.unshift([
  `Node.js v${nodeVersion} (${nodePlatform})`,
  "Declared license: MIT (including the official distribution's bundled third-party notices)",
  `Unsigned executable SHA-256 (before platform signing): ${nodeBinarySha256}`,
  `Distribution: ${nodeAssetUrl}`,
  `Distribution archive SHA-256: ${nodeArchiveSha256}`,
  `Corresponding source: ${nodeSourceUrl}`,
  `Source archive SHA-256: ${nodeSourceSha256}`,
  "Full license and third-party notices: NODE-LICENSE.txt",
  `--- NODE-LICENSE.txt (official ${nodePlatform} distribution) ---\n${nodeLicenseText.trim()}`,
].join("\n"))
notices.push((await readFile(path.resolve(cargoNoticesArg), "utf8")).trim())

const components = [nodeComponent, ...npmComponents, ...cargoComponents]
const refs = new Set()
for (const component of components) {
  PackageURL.fromString(component.purl)
  if (refs.has(component["bom-ref"])) throw new Error(`duplicate SBOM reference: ${component["bom-ref"]}`)
  refs.add(component["bom-ref"])
}

const serialSeed = JSON.stringify({ nodePlatform, nodeBinarySha256, components })
const bom = {
  bomFormat: "CycloneDX",
  specVersion: "1.6",
  serialNumber: `urn:uuid:${uuidFromHash(sha256(serialSeed))}`,
  version: 1,
  metadata: {
    component: {
      type: "application",
      "bom-ref": `filen-menubar-cli@${helperVersion}`,
      name: "filen-menubar-cli",
      version: helperVersion,
    },
  },
  components,
}

await mkdir(output, { recursive: true })
await writeFile(path.join(output, "THIRD_PARTY_NOTICES.txt"), notices.join("\n\n================================================================================\n\n") + "\n")
await writeFile(path.join(output, "runtime.cdx.json"), JSON.stringify(bom, null, 2) + "\n")
await writeFile(path.join(output, "runtime-packages.txt"), uniqueNpm.map(([, item]) => `${item.name}@${item.version}`).join("\n") + "\n")
await writeFile(path.join(output, "runtime-components.txt"), components.map(item => item["bom-ref"]).sort().join("\n") + "\n")

if (npmComponents.some(item => /@filen\/(network-drive|s3|webdav)@/.test(item.purl))) {
  throw new Error("unused mount/server packages leaked into the runtime graph")
}
console.log(`generated compliance inventory for ${npmComponents.length} npm packages, ${cargoComponents.length} crates, and Node.js v${nodeVersion}`)
