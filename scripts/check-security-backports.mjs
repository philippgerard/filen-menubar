import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { createRequire } from "node:module"
import path from "node:path"
import { fileURLToPath } from "node:url"

// Exercise the installed library APIs, including callers that supply an AST.
// RSA fixtures are signed with a disposable private key; they test strict
// verification of malformed DigestInfo, not a private-key-free forgery.
export function checkSecurityBackports(sourceDir) {
  const require = createRequire(path.join(path.resolve(sourceDir), "package.json"))
  const braces = require("braces")
  for (const input of ["{".repeat(4000) + "a,b" + "}".repeat(4000), "(".repeat(4000) + ")".repeat(4000)]) {
    for (const options of [{}, { maxDepth: Infinity }, { maxDepth: 1e9 }]) {
      for (const api of [braces.parse, braces.compile, braces.expand, braces.stringify, braces]) {
        assert.throws(() => api(input, options), /exceeds max depth/)
      }
    }
  }
  for (const api of [braces.compile, braces.expand, braces.stringify]) {
    let ast = { type: "text", value: "a" }
    for (let i = 0; i < 101; i++) ast = { type: "brace", nodes: [ast] }
    assert.throws(() => api({ type: "root", nodes: [ast] }), /exceeds max depth/)
    const cycle = { type: "root", nodes: [] }
    cycle.nodes.push(cycle)
    assert.throws(() => api(cycle), /exceeds max depth/)
  }
  for (const parentCycle of [false, true]) {
    const ast = { type: "paren", nodes: [{ type: "text", value: "a" }] }
    ast.parent = parentCycle ? { type: "paren", parent: ast } : ast
    assert.throws(() => braces.expand(ast), /parent chain contains a cycle/)
  }
  assert.deepEqual(braces.expand("foo/({a,b})"), ["foo/(a)", "foo/(b)"])
  assert.deepEqual(braces.expand("file-{01..03}.{jpg,png}"), ["file-01.jpg", "file-01.png", "file-02.jpg", "file-02.png", "file-03.jpg", "file-03.png"])
  assert.equal(braces.compile("src/{foo,bar}/**"), "src/(foo|bar)/**")
  for (const pattern of ["{{a}}", "{a,{b}}", "{{x}y}", "{a,{b,{c}}", "{}{a}"]) {
    assert.equal(braces.stringify(braces.parse(pattern), { escapeInvalid: true }), pattern)
  }
  assert.doesNotThrow(() => braces.parse("{a,b}", { maxDepth: 1.5 }))
  assert.throws(() => braces.parse("{{a,b},c}", { maxDepth: 1.5 }), /exceeds max depth/)
  assert.doesNotThrow(() => braces.parse("{".repeat(100) + "a" + "}".repeat(100)))

  const forge = require("node-forge")
  const { asn1 } = forge
  const keys = forge.pki.rsa.generateKeyPair({ bits: 1024, e: 3 })
  const node = (type, value, constructed = false) => asn1.create(asn1.Class.UNIVERSAL, type, constructed, value)
  const seq = value => node(asn1.Type.SEQUENCE, value, true)
  const nullParameter = () => node(asn1.Type.NULL, "")
  const extra = () => node(asn1.Type.OCTETSTRING, "unexpected child")
  const verifyInfo = (algorithm, parameters, digestValue, outerExtras = []) => {
    const info = seq([seq([node(asn1.Type.OID, asn1.oidToDer(forge.oids[algorithm]).getBytes()), ...parameters]), node(asn1.Type.OCTETSTRING, digestValue), ...outerExtras])
    const signature = keys.privateKey.sign(asn1.toDer(info).getBytes(), "NONE")
    return keys.publicKey.verify(digestValue, signature)
  }
  for (const algorithm of ["sha1", "sha224", "sha256", "sha384", "sha512", "sha512-224", "sha512-256", "md5", "md2"]) {
    const digest = algorithm === "md2" ? "d".repeat(16) : createHash(algorithm).update("Filen security regression control").digest("latin1")
    const md = forge.md[algorithm]?.create().update("Filen security regression control")
    if (md) assert.equal(keys.publicKey.verify(digest, keys.privateKey.sign(md)), true)
    assert.equal(verifyInfo(algorithm, [nullParameter()], digest), true)
    if (!["md2", "md5"].includes(algorithm)) assert.equal(verifyInfo(algorithm, [], digest), true)
    else assert.throws(() => verifyInfo(algorithm, [], digest), /Missing algorithm identifier NULL/)
    for (const parameters of [[nullParameter(), extra()], [extra()], [extra(), nullParameter()], [node(asn1.Type.NULL, "nonempty parameters")], [node(asn1.Type.NULL, [extra()], true)]]) {
      assert.throws(() => verifyInfo(algorithm, parameters, digest), /valid RSASSA-PKCS1-v1_5/)
    }
    assert.throws(() => verifyInfo(algorithm, [nullParameter()], digest, [extra()]), /valid RSASSA-PKCS1-v1_5/)
    if (md) assert.equal(keys.publicKey.verify(String.fromCharCode(digest.charCodeAt(0) ^ 1) + digest.slice(1), keys.privateKey.sign(md)), false)
  }
  // The SDK uses OAEP with SHA-512. Preserve that encryption/decryption path.
  const oaepKeys = forge.pki.rsa.generateKeyPair({ bits: 2048 })
  const message = "Filen OAEP control"
  const encrypted = oaepKeys.publicKey.encrypt(message, "RSA-OAEP", { md: forge.md.sha512.create() })
  assert.equal(oaepKeys.privateKey.decrypt(encrypted, "RSA-OAEP", { md: forge.md.sha512.create() }), message)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  checkSecurityBackports(process.argv[2])
  console.log("braces and node-forge security regressions and legitimate controls passed")
}
