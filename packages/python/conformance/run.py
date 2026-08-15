from __future__ import annotations

import json
import os
import sys
from typing import Any, Dict, List, Optional

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from wre_runtime import WreError, connect


def check(case: Dict[str, Any], result: Any, error: Optional[WreError]) -> Optional[str]:
    if error is not None:
        expected = case.get("expect_error")
        if not expected:
            return "failed: {} {}".format(error.kind, error.message)
        if error.kind != expected:
            return "expected {}, got {}: {}".format(expected, error.kind, error.message)
        return None

    if case.get("expect_error"):
        return "expected {}, the call succeeded".format(case["expect_error"])

    expect = case.get("expect")
    if expect is not None:
        if isinstance(expect, dict):
            for key, wanted in expect.items():
                if not isinstance(result, dict) or key not in result:
                    return "{} is missing from the result".format(key)
                if result[key] != wanted:
                    return "{} is {}, expected {}".format(
                        key, json.dumps(result[key]), json.dumps(wanted)
                    )
        elif result != expect:
            return "result is {}, expected {}".format(json.dumps(result), json.dumps(expect))

    for key in case.get("expect_keys", []):
        if not isinstance(result, dict) or key not in result:
            return "{} is missing from the result".format(key)

    return None


def main() -> int:
    here = os.path.dirname(os.path.abspath(__file__))
    fallback = os.path.join(here, "..", "..", "..", "conformance", "example.json")
    suite_path = sys.argv[1] if len(sys.argv) > 1 else fallback

    with open(suite_path, "r", encoding="utf-8") as handle:
        suite = json.load(handle)

    cases: List[Dict[str, Any]] = []
    passed = 0
    failed = 0

    sidecar = connect(binary=os.environ.get("WRE_BINARY"), stderr="devnull")

    try:
        session = sidecar.open(suite["target"], suite.get("config") or {})

        for case in suite["cases"]:
            result = None
            error = None

            try:
                deadline = float(case.get("deadline_ms") or 60000) / 1000.0
                result = session.call(case["op"], case.get("params") or {}, deadline=deadline)
            except WreError as thrown:
                error = thrown

            problem = check(case, result, error)

            if problem is None:
                passed += 1
                cases.append({"name": case["name"], "ok": True})
            else:
                failed += 1
                cases.append({"name": case["name"], "ok": False, "problem": problem})

        session.close()
    finally:
        sidecar.close()

    sys.stdout.write(
        json.dumps(
            {
                "language": "python",
                "target": suite["target"],
                "passed": passed,
                "failed": failed,
                "cases": cases,
            }
        )
    )

    return 0 if failed == 0 else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        sys.stdout.write(
            json.dumps(
                {
                    "language": "python",
                    "target": "unknown",
                    "passed": 0,
                    "failed": 1,
                    "cases": [{"name": "harness", "ok": False, "problem": repr(error)}],
                }
            )
        )
        sys.exit(1)
