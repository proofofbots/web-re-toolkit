from __future__ import annotations

from typing import Any, Dict, Optional, Tuple, Type


class ErrorKind:
    BAD_INPUT: str = "bad_input"
    UNSUPPORTED: str = "unsupported"
    TARGET_DRIFT: str = "target_drift"
    BLOCKED: str = "blocked"
    TIMEOUT: str = "timeout"
    CANCELLED: str = "cancelled"
    RESOURCE: str = "resource"
    PROTOCOL: str = "protocol"
    INTERNAL: str = "internal"
    ALL: Tuple[str, ...] = (
        BAD_INPUT,
        UNSUPPORTED,
        TARGET_DRIFT,
        BLOCKED,
        TIMEOUT,
        CANCELLED,
        RESOURCE,
        PROTOCOL,
        INTERNAL,
    )


class WreError(Exception):
    kind: str = ErrorKind.INTERNAL

    def __init__(
        self,
        message: str,
        retryable: bool = False,
        target: Optional[str] = None,
        op: Optional[str] = None,
        detail: Any = None,
    ) -> None:
        super().__init__(message)
        self.message: str = message
        self.retryable: bool = retryable
        self.target: Optional[str] = target
        self.op: Optional[str] = op
        self.detail: Any = detail

    def __str__(self) -> str:
        location = ""
        if self.target and self.op:
            location = f" in {self.target}.{self.op}"
        elif self.target:
            location = f" in {self.target}"
        elif self.op:
            location = f" in {self.op}"
        return f"{self.kind}{location}: {self.message}"


class BadInput(WreError):
    kind: str = ErrorKind.BAD_INPUT


class Unsupported(WreError):
    kind: str = ErrorKind.UNSUPPORTED


class TargetDrift(WreError):
    kind: str = ErrorKind.TARGET_DRIFT


class Blocked(WreError):
    kind: str = ErrorKind.BLOCKED


class Timeout(WreError):
    kind: str = ErrorKind.TIMEOUT


class Cancelled(WreError):
    kind: str = ErrorKind.CANCELLED


class ResourceError(WreError):
    kind: str = ErrorKind.RESOURCE


class ProtocolError(WreError):
    kind: str = ErrorKind.PROTOCOL


class InternalError(WreError):
    kind: str = ErrorKind.INTERNAL


_KIND_TO_CLASS: Dict[str, Type[WreError]] = {
    ErrorKind.BAD_INPUT: BadInput,
    ErrorKind.UNSUPPORTED: Unsupported,
    ErrorKind.TARGET_DRIFT: TargetDrift,
    ErrorKind.BLOCKED: Blocked,
    ErrorKind.TIMEOUT: Timeout,
    ErrorKind.CANCELLED: Cancelled,
    ErrorKind.RESOURCE: ResourceError,
    ErrorKind.PROTOCOL: ProtocolError,
    ErrorKind.INTERNAL: InternalError,
}


def error_from_wire(payload: Optional[Dict[str, Any]]) -> WreError:
    data: Dict[str, Any] = payload if isinstance(payload, dict) else {}
    kind = data.get("kind")
    message = data.get("message")
    if not isinstance(message, str):
        message = "unknown error"
    retryable = data.get("retryable")
    if not isinstance(retryable, bool):
        retryable = False
    target = data.get("target")
    if not isinstance(target, str):
        target = None
    op = data.get("op")
    if not isinstance(op, str):
        op = None
    detail = data.get("detail")
    error_cls = _KIND_TO_CLASS.get(kind, InternalError) if isinstance(kind, str) else InternalError
    return error_cls(message, retryable=retryable, target=target, op=op, detail=detail)
