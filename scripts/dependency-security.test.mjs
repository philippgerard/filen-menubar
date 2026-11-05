import assert from "node:assert/strict"
import { test } from "node:test"
import uri from "fast-uri"

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
