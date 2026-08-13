#!/usr/bin/env node

import assert from "node:assert/strict"
import { createRequire } from "node:module"
import path from "node:path"

const sdkRoot = process.argv[2]

if (!sdkRoot) {
  throw new Error("usage: check-filen-sdk-socket-error.mjs <built-sdk-root>")
}

const require = createRequire(import.meta.url)
const { FS } = require(path.join(path.resolve(sdkRoot), "dist/node/fs/index.js"))

const fs = new FS({
  sdk: {
    config: { baseFolderUUID: "00000000-0000-0000-0000-000000000000" },
  },
  connectToSocket: false,
})
const socket = fs.socket
assert.equal(socket.listenerCount("error"), 1)
assert.doesNotThrow(() => socket.emit("error", new Event("error")))

const expected = new Event("error")
let actual = null
socket.addListener("error", error => {
  actual = error
})
socket.emit("error", expected)
assert.equal(actual, expected)
assert.equal(socket.listenerCount("error"), 2)

console.log("Verified optional Filen SDK socket error handling")
