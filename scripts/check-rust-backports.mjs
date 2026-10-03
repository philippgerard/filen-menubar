import assert from "node:assert/strict"
import { execFileSync } from "node:child_process"
import { createHash } from "node:crypto"
import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const vendor = path.join(repo, "third-party/rust")
const manifest = JSON.parse(fs.readFileSync(path.join(vendor, "SOURCE-HASHES.json"), "utf8"))
assert.equal(manifest.algorithm, "sha256")
assert.deepEqual(Object.keys(manifest.crates).sort(), ["glib", "glib-macros", "gtk3-macros"])
for (const [crate, record] of Object.entries(manifest.crates)) {
  const actualFiles = []
  const visit = (directory, prefix = "") => {
    for (const name of fs.readdirSync(directory).sort()) {
      const filename = path.join(directory, name)
      const relative = prefix + name
      const stat = fs.lstatSync(filename)
      if (stat.isDirectory()) visit(filename, relative + "/")
      else {
        assert.ok(stat.isFile(), `Unsupported vendored entry: ${crate}/${relative}`)
        actualFiles.push(relative)
        assert.equal(createHash("sha256").update(fs.readFileSync(filename)).digest("hex"), record.files[relative], `Unverified backport file: ${crate}/${relative}`)
      }
    }
  }
  visit(path.join(vendor, crate))
  assert.deepEqual(actualFiles.sort(), Object.keys(record.files).sort(), `Unverified source inventory: ${crate}`)
}
const metadata = JSON.parse(execFileSync("cargo", ["metadata", "--locked", "--format-version", "1", "--manifest-path", path.join(repo, "src-tauri/Cargo.toml")], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 }))
for (const crate of Object.keys(manifest.crates)) {
  const selected = metadata.packages.filter(record => record.name === crate).map(record => record.manifest_path)
  assert.deepEqual(selected, [path.join(vendor, crate, "Cargo.toml")], `Missing source patch: ${crate}`)
}
const packages = fs.readFileSync(path.join(repo, "src-tauri/Cargo.lock"), "utf8").split("[[package]]").slice(1)
for (const [crate, version] of [["glib", "0.18.5"], ["glib-macros", "0.18.5"], ["gtk3-macros", "0.18.2"]]) {
  const entries = packages.filter(record => record.includes(`\nname = "${crate}"\n`))
  assert.equal(entries.length, 1, `Unexpected package copies: ${crate}`)
  assert.ok(entries[0].includes(`\nversion = "${version}"\n`) && !/^source =/m.test(entries[0]), `Registry source selected instead of verified backport: ${crate}`)
}
assert.ok(!packages.some(record => /\nname = "proc-macro-error(?:-attr|2)?"\n/.test(record)), "Unmaintained macro error dependency returned")
console.log("Rust backport sources and application lockfile verified")
