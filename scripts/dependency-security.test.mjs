import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { test } from "node:test"
import { fileURLToPath } from "node:url"
import uri from "fast-uri"

test("HTTPS sessions cannot share a cache key across different CA policies", () => {
  // Exercise the shipped interpreter, independently of the development host.
  const runtime = fileURLToPath(new URL("../src-tauri/generated/filen-cli-node", import.meta.url))
  const result = spawnSync(runtime, ["--input-type=module", "-e", `
    import assert from "node:assert/strict"
    import https from "node:https"
    const agent = new https.Agent()
    const options = { host: "example.com", port: 443 }
    // CVE-2026-123877: ca: [] must not resume a default-trust-store session.
    const defaultPolicy = agent.getName(options)
    const emptyPolicy = agent.getName({ ...options, ca: [] })
    assert.notEqual(emptyPolicy, defaultPolicy)
    assert.equal(agent.getName({ ...options }), defaultPolicy)
    assert.equal(agent.getName({ ...options, ca: [] }), emptyPolicy)
    assert.notEqual(agent.getName({ ...options, ca: ["custom CA"] }), emptyPolicy)
  `], { encoding: "utf8", timeout: 5000 })
  assert.ifError(result.error)
  assert.equal(result.status, 0, result.stderr)
})

test("encoded URI schemes never become executable schemes", () => {
  for (const input of ["%6Aavascript:alert(1)", "%u006Aavascript:alert(1)"]) {
    assert.notEqual(uri.parse(input).scheme, "javascript")
    assert.ok(!uri.normalize(input).startsWith("javascript:"))
  }
  assert.equal(uri.parse("https://example.com/file").scheme, "https")
})

test("ambiguous hosts are rejected instead of silently canonicalized", () => {
  for (const input of ["http://a@b@evil.example/", "http://exa\tmple.com/", "http://exa\nmple.com/", "http://exa\rmple.com/"]) {
    assert.ok(uri.parse(input).error)
  }
  assert.equal(uri.parse("https://example.com/file").host, "example.com")
})

test("relative reference normalization preserves its resolution target", () => {
  const base = "https://example.com/dir/subdir/"
  for (const ref of ["../x", "a/../b", "../../x"]) {
    assert.equal(uri.resolve(base, uri.normalize(ref)), uri.resolve(base, ref))
  }
  assert.equal(uri.resolve(base, "../file"), "https://example.com/dir/file")
})

test("suffix paths without an authority cannot inject a scheme", () => {
  for (const input of ["javascript:alert(1)", "data:text/plain,test", "vbscript:test"]) {
    // A suffix reference drops the scheme, so its first path segment must stay unparseable as one.
    for (const component of [{ path: input }, { scheme: "https", path: input }]) {
      const result = uri.serialize(component, { reference: "suffix" })
      assert.equal(uri.parse(result).scheme, undefined)
    }
  }
})
