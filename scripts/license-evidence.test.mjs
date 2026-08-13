import assert from "node:assert/strict"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import os from "node:os"
import path from "node:path"
import test from "node:test"
import {
  discoverLicenseEvidence,
  extractReadmeLicense,
  licenseFilePattern,
} from "./license-evidence.mjs"

const mit = `MIT License

Copyright (c) 2026 Example

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies.

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.`

test("recognizes hyphenated license filenames", () => {
  assert.equal(licenseFilePattern.test("LICENSE-MIT.txt"), true)
  assert.equal(licenseFilePattern.test("LICENCE-APACHE"), true)
  assert.equal(licenseFilePattern.test("README.md"), false)
})

test("extracts bounded ATX and Setext README license sections", () => {
  for (const readme of [
    `# Package\n\n## License\n\n${mit}\n\n## Contributing\n\nExcluded`,
    `Package\n=======\n\nLicense (MIT)\n-------------\n${mit}\n\nAPI\n---\nExcluded`,
  ]) {
    const section = extractReadmeLicense(readme)
    assert.match(section, /Permission is hereby granted/)
    assert.doesNotMatch(section, /Excluded/)
  }
})

test("prefers LICENSE-* and fails closed without substantive text", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "filen-license-test-"))
  try {
    await writeFile(path.join(directory, "LICENSE-MIT.txt"), mit)
    await writeFile(path.join(directory, "README.md"), `# Package\n\n## License\n\n${mit}`)
    assert.deepEqual((await discoverLicenseEvidence(directory)).map(item => item.source), ["LICENSE-MIT.txt"])
    await rm(path.join(directory, "LICENSE-MIT.txt"))
    await writeFile(path.join(directory, "README.md"), "# Package\n\n## License\n\nMIT; see AUTHORS.")
    assert.deepEqual(await discoverLicenseEvidence(directory), [])
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test("keeps short routing notes when another license file has full text", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "filen-license-test-"))
  try {
    await writeFile(path.join(directory, "COPYING"), "See LICENSE-MIT.")
    await writeFile(path.join(directory, "LICENSE-MIT"), mit)
    assert.deepEqual(
      (await discoverLicenseEvidence(directory)).map(item => item.source),
      ["COPYING", "LICENSE-MIT"],
    )
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})
