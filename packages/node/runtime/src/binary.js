import { statSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { WreError, ErrorKind } from "./errors.js";

const TRIPLES = {
  "darwin:arm64": "aarch64-apple-darwin",
  "darwin:x64": "x86_64-apple-darwin",
  "linux:x64": "x86_64-unknown-linux-gnu",
  "linux:arm64": "aarch64-unknown-linux-gnu",
  "win32:x64": "x86_64-pc-windows-msvc",
};

export function currentTriple() {
  const key = `${process.platform}:${process.arch}`;
  const triple = TRIPLES[key];
  if (!triple) {
    throw new WreError(
      ErrorKind.Unsupported,
      `no known binary triple for platform "${process.platform}" arch "${process.arch}"`,
    );
  }
  return triple;
}

export function verifySha256(path, expected) {
  const data = readFileSync(path);
  const actual = createHash("sha256").update(data).digest("hex");
  if (actual !== expected) {
    throw new WreError(
      ErrorKind.Resource,
      `sha256 mismatch for "${path}": expected ${expected}, got ${actual}`,
    );
  }
}

function isFile(path) {
  try {
    return statSync(path).isFile();
  } catch {
    return false;
  }
}

export function resolveBinary(options = {}) {
  const tried = [];

  const envBinary = process.env.WRE_BINARY;
  if (envBinary) {
    tried.push(`WRE_BINARY=${envBinary}`);
    if (isFile(envBinary)) {
      return envBinary;
    }
  }

  if (options.embedded) {
    const exeName = process.platform === "win32" ? "wred.exe" : "wred";
    const candidate = join(options.embedded, exeName);
    tried.push(candidate);
    if (isFile(candidate)) {
      if (options.sha256) {
        verifySha256(candidate, options.sha256);
      }
      return candidate;
    }
  }

  const attempted = tried.length > 0 ? tried.join(", ") : "nothing (no WRE_BINARY and no embedded path given)";
  throw new WreError(ErrorKind.Resource, `could not resolve a wred binary, tried: ${attempted}`);
}
