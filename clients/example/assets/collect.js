"use strict";

function fnv1a(text) {
  var hash = 0x811c9dc5;
  for (var index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index) & 0xff;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash >>> 0;
}

function encodeJson(value) {
  if (value === undefined) {
    throw new Error("Recursive input");
  }
  return JSON.stringify(value, function (key, entry) {
    if (typeof entry === "bigint") {
      return entry.toString();
    }
    return entry;
  });
}

function sealBody(text, key) {
  var bytes = [];
  var table = typeof key === "string" ? deriveKey(key) : key;
  var size = table.length;
  for (var index = 0; index < text.length; index += 1) {
    var unit = text.charCodeAt(index) & 0xff;
    bytes.push((unit ^ table[index % size]) & 0xff);
  }
  return bytes;
}

function deriveKey(seed) {
  var state = fnv1a(String(seed));
  var table = [];
  for (var index = 0; index < 16; index += 1) {
    state = (Math.imul(state, 0x01000193) ^ (index + 1)) >>> 0;
    table.push(state & 0xff);
  }
  return table;
}

function buildPayload(facts) {
  var payload = {
    v: 3,
    u: facts.url || "",
    t: facts.title || "",
    ts: facts.now || 0,
    n: facts.nonce || "",
    s: [facts.width || 0, facts.height || 0, facts.depth || 24],
    l: facts.language || "en-US",
    z: facts.timezone || "UTC",
    w: facts.webdriver === true ? 1 : 0
  };

  if (facts.extra && typeof facts.extra === "object") {
    payload.x = facts.extra;
  }

  payload.c = fnv1a(payload.u + "|" + payload.t + "|" + payload.ts + "|" + payload.n);
  return payload;
}

globalThis.__internals = {
  build: "example-3.1.0",
  codec: {
    hash: fnv1a,
    seal: sealBody,
    encode: encodeJson,
    payload: buildPayload
  }
};
