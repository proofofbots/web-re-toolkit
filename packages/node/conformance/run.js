import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { connect, WreError } from "../runtime/index.js";

const here = dirname(fileURLToPath(import.meta.url));

function equal(left, right) {
  if (left === right) {
    return true;
  }

  if (Array.isArray(left) && Array.isArray(right)) {
    return left.length === right.length && left.every((item, index) => equal(item, right[index]));
  }

  if (left && right && typeof left === "object" && typeof right === "object") {
    const leftKeys = Object.keys(left).sort();
    const rightKeys = Object.keys(right).sort();
    return equal(leftKeys, rightKeys) && leftKeys.every((key) => equal(left[key], right[key]));
  }

  return false;
}

function check(testCase, result, error) {
  if (error) {
    if (!testCase.expect_error) {
      return `failed: ${error.kind ?? "unknown"} ${error.message}`;
    }
    if (error.kind !== testCase.expect_error) {
      return `expected ${testCase.expect_error}, got ${error.kind}: ${error.message}`;
    }
    return null;
  }

  if (testCase.expect_error) {
    return `expected ${testCase.expect_error}, the call succeeded`;
  }

  if (testCase.expect !== undefined && testCase.expect !== null) {
    if (!Array.isArray(testCase.expect) && typeof testCase.expect === "object") {
      for (const [key, wanted] of Object.entries(testCase.expect)) {
        if (!(result && key in result)) {
          return `${key} is missing from the result`;
        }
        if (!equal(result[key], wanted)) {
          return `${key} is ${JSON.stringify(result[key])}, expected ${JSON.stringify(wanted)}`;
        }
      }
    } else if (!equal(result, testCase.expect)) {
      return `result is ${JSON.stringify(result)}, expected ${JSON.stringify(testCase.expect)}`;
    }
  }

  for (const key of testCase.expect_keys ?? []) {
    if (!(result && key in result)) {
      return `${key} is missing from the result`;
    }
  }

  return null;
}

async function main() {
  const suitePath = process.argv[2] ?? join(here, "..", "..", "..", "conformance", "example.json");
  const suite = JSON.parse(readFileSync(suitePath, "utf8"));

  const sidecar = await connect({
    binary: process.env.WRE_BINARY,
    stderr: "ignore",
  });

  const session = await sidecar.open(suite.target, suite.config ?? {});

  const cases = [];
  let passed = 0;
  let failed = 0;

  for (const testCase of suite.cases) {
    let result;
    let error;

    try {
      result = await session.call(testCase.op, testCase.params ?? {}, {
        deadlineMs: testCase.deadline_ms ?? 60000,
      });
    } catch (thrown) {
      error = thrown instanceof WreError ? thrown : new WreError("internal", String(thrown));
    }

    const problem = check(testCase, result, error);

    if (problem) {
      failed += 1;
      cases.push({ name: testCase.name, ok: false, problem });
    } else {
      passed += 1;
      cases.push({ name: testCase.name, ok: true });
    }
  }

  await session.close();
  await sidecar.close();

  process.stdout.write(
    JSON.stringify({ language: "node", target: suite.target, passed, failed, cases }),
  );

  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  process.stdout.write(
    JSON.stringify({
      language: "node",
      target: "unknown",
      passed: 0,
      failed: 1,
      cases: [{ name: "harness", ok: false, problem: String(error && error.stack ? error.stack : error) }],
    }),
  );
  process.exit(1);
});
