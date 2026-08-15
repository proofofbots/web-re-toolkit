from __future__ import annotations

import hashlib
import os
import platform
import sys
from typing import Dict, List, Optional, Tuple

from .errors import ResourceError, Unsupported

_TRIPLES: Dict[Tuple[str, str], str] = {
    ("darwin", "arm64"): "aarch64-apple-darwin",
    ("darwin", "x86_64"): "x86_64-apple-darwin",
    ("linux", "x86_64"): "x86_64-unknown-linux-gnu",
    ("linux", "arm64"): "aarch64-unknown-linux-gnu",
    ("win32", "x86_64"): "x86_64-pc-windows-msvc",
}


def current_triple() -> str:
    plat = sys.platform
    machine = platform.machine()
    normalized = machine
    if machine in ("arm64", "aarch64"):
        normalized = "arm64"
    elif machine in ("x86_64", "AMD64"):
        normalized = "x86_64"
    triple = _TRIPLES.get((plat, normalized))
    if triple is None:
        raise Unsupported(f"no wred binary for platform {plat!r} machine {machine!r}")
    return triple


def verify_sha256(path: str, expected: str) -> None:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    actual = digest.hexdigest()
    if actual.lower() != expected.lower():
        raise ResourceError(
            f"sha256 mismatch for {path}: expected {expected}, got {actual}",
            retryable=True,
        )


def resolve_binary(package_dir: Optional[str] = None, sha256: Optional[str] = None) -> str:
    tried: List[str] = []
    env_path = os.environ.get("WRE_BINARY")
    if env_path:
        tried.append(env_path)
        if os.path.isabs(env_path) and os.path.isfile(env_path):
            return env_path
    if package_dir is not None:
        triple = current_triple()
        binary_name = "wred.exe" if sys.platform == "win32" else "wred"
        candidate = os.path.join(package_dir, "bin", triple, binary_name)
        tried.append(candidate)
        if os.path.isfile(candidate):
            if sha256 is not None:
                verify_sha256(candidate, sha256)
            return candidate
    listed = ", ".join(tried) if tried else "(none)"
    raise ResourceError(f"could not resolve a wred binary, tried: {listed}", retryable=True)


def cache_root() -> str:
    override = os.environ.get("WRE_CACHE_DIR")
    if override:
        return override
    xdg = os.environ.get("XDG_CACHE_HOME")
    if xdg:
        return os.path.join(xdg, "wre")
    if sys.platform == "win32":
        local_app_data = os.environ.get("LOCALAPPDATA")
        if local_app_data:
            return os.path.join(local_app_data, "wre")
        return os.path.join(os.path.expanduser("~"), "AppData", "Local", "wre")
    return os.path.join(os.path.expanduser("~"), ".cache", "wre")
