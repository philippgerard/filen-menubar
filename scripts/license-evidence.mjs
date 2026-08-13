import { readFile, readdir } from "node:fs/promises"
import path from "node:path"

export const licenseFilePattern = /^(?:licen[cs]e|copying|notice)(?:$|[._ -])/i
const readmePattern = /^readme(?:$|[._ -])/i

function heading(line) {
  const match = /^ {0,3}(#{1,6})[ \t]+(.+?)[ \t]*#*[ \t]*$/.exec(line)
  return match ? { level: match[1].length, title: match[2] } : null
}

function setextHeading(lines, index) {
  if (!lines[index]?.trim() || index + 1 >= lines.length) return null
  const match = /^ {0,3}(=+|-+)[ \t]*$/.exec(lines[index + 1])
  return match ? { level: match[1][0] === "=" ? 1 : 2, title: lines[index] } : null
}

function isLicenseHeading(title) {
  return /\blicen[cs]es?\b/i.test(title.replace(/<[^>]*>/g, " ").replace(/[`*_~[\]()]/g, " "))
}

export function extractReadmeLicense(readme) {
  const lines = readme.replaceAll("\r\n", "\n").replaceAll("\r", "\n").split("\n")
  let start
  for (let index = 0; index < lines.length; index += 1) {
    const found = heading(lines[index]) ?? setextHeading(lines, index)
    if (found && isLicenseHeading(found.title)) {
      start = {
        index,
        body: index + (heading(lines[index]) ? 1 : 2),
        level: found.level,
      }
      break
    }
  }
  if (!start) return null

  let end = lines.length
  for (let index = start.body; index < lines.length; index += 1) {
    const found = heading(lines[index]) ?? setextHeading(lines, index)
    if (found && found.level <= start.level) {
      end = index
      break
    }
  }
  return lines.slice(start.index, end).join("\n").trim()
}

export function requireSubstantiveLicenseText(text, source) {
  const normalized = text.trim()
  if (normalized.length < 200 ||
      !/(copyright|permission|redistribution|public domain|warranty|licensed? under)/i.test(normalized)) {
    throw new Error(`license text is missing or incomplete: ${source}`)
  }
  return normalized
}

export async function discoverLicenseEvidence(directory) {
  const files = (await readdir(directory, { withFileTypes: true }))
    .filter(entry => entry.isFile() || entry.isSymbolicLink())
    .map(entry => entry.name)
    .sort()
  const licenseFiles = files.filter(file => licenseFilePattern.test(file))
  if (licenseFiles.length > 0) {
    const evidence = await Promise.all(licenseFiles.map(async file => ({
      source: file,
      text: (await readFile(path.join(directory, file), "utf8")).trim(),
    })))
    if (evidence.some(item => {
      try {
        requireSubstantiveLicenseText(item.text, path.join(directory, item.source))
        return true
      } catch {
        return false
      }
    })) return evidence
  }

  for (const file of files.filter(file => readmePattern.test(file))) {
    const section = extractReadmeLicense(await readFile(path.join(directory, file), "utf8"))
    if (section) {
      try {
        return [{
          source: `${file} (license section)`,
          text: requireSubstantiveLicenseText(section, `${path.join(directory, file)} license section`),
        }]
      } catch {
        // A short pointer such as "MIT; see AUTHORS" is not the license text.
        // Callers may supply an exact-hash pinned upstream supplement instead.
      }
    }
  }
  return []
}
