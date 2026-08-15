from __future__ import annotations

import json
import struct
from typing import Any, BinaryIO, Dict, List, Optional, Tuple

from .errors import ProtocolError

_HEADER = struct.Struct(">II")
_MAX_JSON_LEN = 64 * 1024 * 1024
_MAX_BIN_LEN = 512 * 1024 * 1024


def encode_frame(envelope: Dict[str, Any], bin_part: bytes = b"") -> bytes:
    json_bytes = json.dumps(envelope, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    return _HEADER.pack(len(json_bytes), len(bin_part)) + json_bytes + bin_part


class FrameDecoder:
    def __init__(self) -> None:
        self._buffer = bytearray()

    def feed(self, chunk: bytes) -> List[Tuple[Dict[str, Any], bytes]]:
        self._buffer.extend(chunk)
        frames: List[Tuple[Dict[str, Any], bytes]] = []
        while True:
            if len(self._buffer) < _HEADER.size:
                break
            json_len, bin_len = _HEADER.unpack_from(self._buffer, 0)
            _check_caps(json_len, bin_len)
            total = _HEADER.size + json_len + bin_len
            if len(self._buffer) < total:
                break
            json_bytes = bytes(self._buffer[_HEADER.size : _HEADER.size + json_len])
            bin_bytes = bytes(self._buffer[_HEADER.size + json_len : total])
            del self._buffer[:total]
            envelope = json.loads(json_bytes.decode("utf-8"))
            frames.append((envelope, bin_bytes))
        return frames


def read_frame(stream: BinaryIO) -> Optional[Tuple[Dict[str, Any], bytes]]:
    header = _read_exact(stream, _HEADER.size, allow_eof_at_start=True)
    if header is None:
        return None
    json_len, bin_len = _HEADER.unpack(header)
    _check_caps(json_len, bin_len)
    json_bytes = _read_exact(stream, json_len, allow_eof_at_start=False)
    bin_bytes = _read_exact(stream, bin_len, allow_eof_at_start=False)
    envelope = json.loads((json_bytes or b"").decode("utf-8"))
    return envelope, bin_bytes or b""


def _check_caps(json_len: int, bin_len: int) -> None:
    if json_len > _MAX_JSON_LEN:
        raise ProtocolError(f"json part {json_len} bytes exceeds the {_MAX_JSON_LEN} byte cap")
    if bin_len > _MAX_BIN_LEN:
        raise ProtocolError(f"binary part {bin_len} bytes exceeds the {_MAX_BIN_LEN} byte cap")


def _read_exact(stream: BinaryIO, n: int, allow_eof_at_start: bool) -> Optional[bytes]:
    if n == 0:
        return b""
    chunks: List[bytes] = []
    remaining = n
    first = True
    while remaining > 0:
        chunk = stream.read(remaining)
        if not chunk:
            if first and allow_eof_at_start:
                return None
            raise ProtocolError("truncated frame: stream ended mid frame")
        chunks.append(chunk)
        remaining -= len(chunk)
        first = False
    return b"".join(chunks)
