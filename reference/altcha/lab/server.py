import base64
import hashlib
import hmac
import json
import os
import random
import struct
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

SECRET = os.environ.get("ALTCHA_SECRET", "lab.secret").encode()
COST = int(os.environ.get("ALTCHA_COST", "1"))
COUNTER_MAX = int(os.environ.get("ALTCHA_COUNTER_MAX", "250000"))
PORT = int(os.environ.get("ALTCHA_PORT", "8787"))
ROOT = os.path.dirname(os.path.abspath(__file__))


def derive_key(salt, password, cost, key_length):
    data = salt + password
    derived = b""
    for _ in range(max(1, cost)):
        derived = hashlib.sha256(data).digest()[:key_length]
        data = derived
    return derived


def canonical(parameters):
    return json.dumps(parameters, sort_keys=True, separators=(",", ":"))


def create_challenge():
    nonce = os.urandom(16)
    salt = os.urandom(16)
    counter = random.randrange(COUNTER_MAX // 2, COUNTER_MAX)
    password = nonce + struct.pack(">I", counter)
    derived = derive_key(salt, password, COST, 32)

    parameters = {
        "algorithm": "SHA-256",
        "cost": COST,
        "expiresAt": int(time.time()) + 600,
        "keyLength": 32,
        "keyPrefix": derived[:16].hex(),
        "nonce": nonce.hex(),
        "salt": salt.hex(),
    }
    signature = hmac.new(SECRET, canonical(parameters).encode(), hashlib.sha256).hexdigest()
    return {"parameters": parameters, "signature": signature}, counter


def verify(payload):
    decoded = json.loads(base64.b64decode(payload))
    parameters = decoded["challenge"]["parameters"]
    solution = decoded["solution"]

    expected = hmac.new(SECRET, canonical(parameters).encode(), hashlib.sha256).hexdigest()
    if not hmac.compare_digest(expected, decoded["challenge"]["signature"]):
        return {"verified": False, "reason": "signature"}
    if parameters["expiresAt"] < time.time():
        return {"verified": False, "reason": "expired"}

    password = bytes.fromhex(parameters["nonce"]) + struct.pack(">I", solution["counter"])
    derived = derive_key(
        bytes.fromhex(parameters["salt"]),
        password,
        parameters["cost"],
        parameters["keyLength"],
    )
    if derived.hex() != solution["derivedKey"]:
        return {"verified": False, "reason": "solution"}

    return {"verified": True, "counter": solution["counter"], "took": solution.get("time")}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        print("%s %s" % (self.command, self.path))

    def send_json(self, status, body):
        raw = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(raw)))
        self.send_header("x-altcha-config", json.dumps({"maxnumber": COUNTER_MAX}))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        if self.path.startswith("/altcha/challenge"):
            challenge, counter = create_challenge()
            print("issued challenge, counter %d" % counter)
            self.send_json(200, challenge)
            return

        name = "index.html" if self.path in ("/", "") else self.path.lstrip("/")
        path = os.path.join(ROOT, os.path.basename(name))
        if not os.path.isfile(path):
            self.send_json(404, {"error": "not found"})
            return

        with open(path, "rb") as handle:
            raw = handle.read()
        self.send_response(200)
        self.send_header("content-type", "text/html; charset=utf-8")
        self.send_header("content-length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        body = json.loads(self.rfile.read(length) or b"{}")
        try:
            result = verify(body.get("payload", ""))
        except Exception as error:
            result = {"verified": False, "reason": str(error)}
        print("verify %s" % result)
        self.send_json(200, result)


if __name__ == "__main__":
    print("altcha lab on http://localhost:%d, cost %d, counter below %d" % (PORT, COST, COUNTER_MAX))
    HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
