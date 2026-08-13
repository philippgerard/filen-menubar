#!/usr/bin/env node

import { readFile } from "node:fs/promises"
import path from "node:path"
import process from "node:process"
import { JsonValidator } from "@cyclonedx/cyclonedx-library/Validation"
import { Version } from "@cyclonedx/cyclonedx-library/Spec"
import packageUrl from "packageurl-js"

const [
  bomArg = "src-tauri/generated/licenses/filen-cli/runtime.cdx.json",
  noticesArg,
] = process.argv.slice(2)
const bomFile = path.resolve(bomArg)
const noticesFile = path.resolve(noticesArg ?? path.join(path.dirname(bomFile), "THIRD_PARTY_NOTICES.txt"))
const data = await readFile(bomFile, "utf8")
const bom = JSON.parse(data)
const noticeBlocks = (await readFile(noticesFile, "utf8"))
  .split("\n\n================================================================================\n\n")
const validationError = await new JsonValidator(Version.v1dot6).validate(data)
if (validationError !== null) {
  console.error(JSON.stringify(validationError, null, 2))
  throw new Error(`CycloneDX 1.6 schema validation failed: ${bomFile}`)
}

if (!/^urn:uuid:[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(bom.serialNumber)) {
  throw new Error(`SBOM serialNumber is not an RFC 4122 version-4 UUID: ${bom.serialNumber}`)
}

const { PackageURL } = packageUrl
const ecosystems = new Map()
const runtimeComponents = []
for (const component of bom.components ?? []) {
  if (!component.purl) throw new Error(`component has no purl: ${component.name}@${component.version}`)
  const canonical = PackageURL.fromString(component.purl).toString()
  if (canonical !== component.purl) throw new Error(`non-canonical purl: ${component.purl} (expected ${canonical})`)
  const ecosystem = component.properties?.find(item => item.name === "filen-menubar:ecosystem")?.value
  ecosystems.set(ecosystem, (ecosystems.get(ecosystem) ?? 0) + 1)
  if (ecosystem === "npm" || ecosystem === "cargo" || ecosystem === "node-runtime") {
    runtimeComponents.push({ component, ecosystem })
  }
}

for (const { component, ecosystem } of runtimeComponents) {
  const identity = `${component.name}@${component.version}`
  const block = noticeBlocks.find(item => ecosystem === "node-runtime"
    ? item.startsWith(`Node.js v${component.version} (`)
    : item.startsWith(`${identity}\n`))
  if (!block) throw new Error(`notices do not cover runtime component: ${identity}`)
  const evidence = /\n--- [^\n]+ ---\n([\s\S]+)$/.exec(block)?.[1]?.trim()
  if (!evidence || evidence.length < 200 ||
      !/(copyright|permission|redistribution|public domain|warranty|licensed? under)/i.test(evidence)) {
    throw new Error(`notices do not contain substantive license text: ${identity}`)
  }
}

if ((ecosystems.get("npm") ?? 0) < 100) throw new Error("SBOM does not cover the npm runtime graph")
if ((ecosystems.get("cargo") ?? 0) < 150) throw new Error("SBOM does not cover the native Cargo graph")
if (ecosystems.get("node-runtime") !== 1) throw new Error("SBOM must contain exactly one Node runtime component")
if (!(bom.components ?? []).some(item => item.purl.startsWith("pkg:npm/%40filen/sdk@"))) {
  throw new Error("scoped npm purls do not use the canonical namespace/name form")
}

console.log(`validated CycloneDX 1.6 SBOM with ${bom.components.length} runtime components`)
