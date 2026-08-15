from __future__ import annotations

import os
import queue
import subprocess
import threading
from typing import Any, Callable, Dict, List, Mapping, Optional, Sequence, Tuple, Union

from .binary import resolve_binary
from .errors import ProtocolError, Timeout, WreError, error_from_wire
from .frames import encode_frame, read_frame

EventHandler = Callable[[int, str, Any], None]


class _PendingCall:
    def __init__(self, on_event: Optional[EventHandler]) -> None:
        self.queue: "queue.Queue[Tuple[str, Any]]" = queue.Queue(maxsize=1)
        self.on_event = on_event


class Sidecar:
    def __init__(
        self,
        proc: "subprocess.Popen[bytes]",
        on_event: Optional[EventHandler] = None,
    ) -> None:
        self._proc = proc
        self._on_event = on_event
        self._lock = threading.Lock()
        self._write_lock = threading.Lock()
        self._next_id = 1
        self._pending: Dict[int, _PendingCall] = {}
        self._closed = False
        self._hello: Dict[str, Any] = {}
        self._reader_thread = threading.Thread(
            target=self._reader_loop, name="wre-sidecar-reader", daemon=True
        )
        self._reader_thread.start()

    @property
    def hello(self) -> Dict[str, Any]:
        return self._hello

    @property
    def pid(self) -> int:
        return self._proc.pid

    def describe(self) -> Any:
        result, _ = self._request("describe", {}, None, None, None, b"", None)
        return result

    def targets(self) -> Any:
        result, _ = self._request("targets", {}, None, None, None, b"", None)
        return result

    def metrics(self) -> Any:
        result, _ = self._request("metrics", {}, None, None, None, b"", None)
        return result

    def open(self, target: str, config: Optional[Dict[str, Any]] = None) -> "Session":
        params = {"target": target, "config": config if config is not None else {}}
        result, _ = self._request("open", params, None, None, None, b"", None)
        return Session(self, result["session"], result["target"], result.get("ops", []))

    def call(
        self,
        op: str,
        params: Optional[Dict[str, Any]] = None,
        session: Optional[Union[str, "Session"]] = None,
        deadline: Optional[float] = None,
        on_event: Optional[EventHandler] = None,
        bin_part: bytes = b"",
    ) -> Any:
        session_id, target = _split_session(session)
        result, _ = self._request(op, params, session_id, deadline, on_event, bin_part, target)
        return result

    def call_with_binary(
        self,
        op: str,
        params: Optional[Dict[str, Any]] = None,
        session: Optional[Union[str, "Session"]] = None,
        deadline: Optional[float] = None,
        on_event: Optional[EventHandler] = None,
        bin_part: bytes = b"",
    ) -> Tuple[Any, bytes]:
        session_id, target = _split_session(session)
        return self._request(op, params, session_id, deadline, on_event, bin_part, target)

    def close(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._closed = True
        try:
            if self._proc.poll() is None:
                self._proc.kill()
        except Exception:
            pass
        self._fail_all(ProtocolError("sidecar is closed", retryable=False))
        try:
            if self._proc.stdin is not None:
                self._proc.stdin.close()
        except Exception:
            pass
        self._reader_thread.join(timeout=2.0)
        try:
            self._proc.wait(timeout=2.0)
        except Exception:
            pass

    def shutdown(self) -> None:
        try:
            self._request("shutdown", {}, None, 5.0, None, b"", None)
        except WreError:
            pass
        self.close()

    def __enter__(self) -> "Sidecar":
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        self.close()

    def _request(
        self,
        op: str,
        params: Optional[Dict[str, Any]],
        session: Optional[str],
        deadline: Optional[float],
        on_event: Optional[EventHandler],
        bin_part: bytes,
        target: Optional[str],
    ) -> Tuple[Any, bytes]:
        with self._lock:
            if self._closed:
                raise ProtocolError("sidecar is closed", retryable=False, target=target, op=op)
            rid = self._next_id
            self._next_id += 1
            call = _PendingCall(on_event)
            self._pending[rid] = call

        envelope: Dict[str, Any] = {
            "t": "req",
            "v": 1,
            "id": rid,
            "op": op,
            "params": params if params is not None else {},
        }
        if session is not None:
            envelope["session"] = session
        if deadline is not None:
            envelope["deadline_ms"] = int(deadline * 1000)

        try:
            self._write_frame(envelope, bin_part)
        except Exception as exc:
            with self._lock:
                self._pending.pop(rid, None)
            raise ProtocolError(
                f"failed to write frame: {exc}", retryable=False, target=target, op=op
            ) from exc

        wait_timeout = (deadline + 5.0) if deadline is not None else None
        try:
            kind, payload = call.queue.get(timeout=wait_timeout)
        except queue.Empty:
            with self._lock:
                self._pending.pop(rid, None)
            self._send_cancel(rid)
            raise Timeout(
                f"{op} did not respond within {wait_timeout:.1f}s",
                retryable=True,
                target=target,
                op=op,
            )

        if kind == "error":
            raise payload

        envelope_res, bin_res = payload
        if not envelope_res.get("ok", False):
            error = error_from_wire(envelope_res.get("error"))
            if error.target is None and target is not None:
                error.target = target
            if error.op is None:
                error.op = op
            raise error
        return envelope_res.get("result"), bin_res

    def _write_frame(self, envelope: Dict[str, Any], bin_part: bytes) -> None:
        data = encode_frame(envelope, bin_part)
        stdin = self._proc.stdin
        if stdin is None:
            raise ProtocolError("sidecar stdin is not available", retryable=False)
        with self._write_lock:
            stdin.write(data)
            stdin.flush()

    def _send_cancel(self, rid: int) -> None:
        try:
            self._write_frame({"t": "cancel", "v": 1, "id": rid}, b"")
        except Exception:
            pass

    def _reader_loop(self) -> None:
        stream = self._proc.stdout
        close_error: WreError = ProtocolError("the sidecar closed the stream", retryable=False)
        try:
            if stream is not None:
                while True:
                    frame = read_frame(stream)
                    if frame is None:
                        break
                    envelope, bin_part = frame
                    self._dispatch(envelope, bin_part)
        except ProtocolError as exc:
            close_error = exc
        except Exception as exc:
            close_error = ProtocolError(f"sidecar reader failed: {exc}", retryable=False)
        finally:
            self._fail_all(close_error)

    def _dispatch(self, envelope: Dict[str, Any], bin_part: bytes) -> None:
        t = envelope.get("t")
        rid = envelope.get("id")
        if t == "evt":
            with self._lock:
                call = self._pending.get(rid)
            event_name = envelope.get("event")
            data = envelope.get("data")
            if call is not None and call.on_event is not None:
                try:
                    call.on_event(rid, event_name, data)
                except Exception:
                    pass
            if self._on_event is not None:
                try:
                    self._on_event(rid, event_name, data)
                except Exception:
                    pass
        elif t == "res":
            with self._lock:
                call = self._pending.pop(rid, None)
            if call is not None:
                try:
                    call.queue.put_nowait(("frame", (envelope, bin_part)))
                except queue.Full:
                    pass

    def _fail_all(self, error: WreError) -> None:
        with self._lock:
            pending = list(self._pending.items())
            self._pending.clear()
        for _, call in pending:
            try:
                call.queue.put_nowait(("error", error))
            except queue.Full:
                pass


class Session:
    def __init__(
        self, sidecar: Sidecar, session_id: str, target: str, ops: Sequence[str]
    ) -> None:
        self._sidecar = sidecar
        self._id = session_id
        self._target = target
        self._ops = list(ops)
        self._closed = False

    @property
    def id(self) -> str:
        return self._id

    @property
    def target(self) -> str:
        return self._target

    @property
    def ops(self) -> List[str]:
        return list(self._ops)

    def call(
        self,
        op: str,
        params: Optional[Dict[str, Any]] = None,
        deadline: Optional[float] = None,
        on_event: Optional[EventHandler] = None,
    ) -> Any:
        result, _ = self._sidecar._request(
            op, params, self._id, deadline, on_event, b"", self._target
        )
        return result

    def call_with_binary(
        self,
        op: str,
        params: Optional[Dict[str, Any]] = None,
        deadline: Optional[float] = None,
        on_event: Optional[EventHandler] = None,
        bin_part: bytes = b"",
    ) -> Tuple[Any, bytes]:
        return self._sidecar._request(
            op, params, self._id, deadline, on_event, bin_part, self._target
        )

    def warmup(self) -> Any:
        return self.call("warmup", {})

    def health(self) -> Any:
        return self.call("health", {})

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        try:
            self._sidecar._request("close", {}, self._id, None, None, b"", self._target)
        except Exception:
            pass

    def __enter__(self) -> "Session":
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        self.close()


def _split_session(
    session: Optional[Union[str, "Session"]]
) -> Tuple[Optional[str], Optional[str]]:
    if isinstance(session, Session):
        return session.id, session.target
    return session, None


STDERR_MODES = ("inherit", "ignore", "devnull")


def stderr_mode(requested: str = "ignore") -> str:
    choice = os.environ.get("WRE_STDERR", requested)
    if choice not in STDERR_MODES:
        raise ValueError(f"stderr must be one of {STDERR_MODES}, got {choice!r}")
    return "inherit" if choice == "inherit" else "ignore"


def connect(
    binary: Optional[str] = None,
    args: Sequence[str] = (),
    env: Optional[Mapping[str, str]] = None,
    cwd: Optional[str] = None,
    stderr: str = "ignore",
    on_event: Optional[EventHandler] = None,
    expect_protocol: int = 1,
    expect_schema_hash: Optional[str] = None,
    startup_timeout: float = 30.0,
) -> Sidecar:
    resolved = binary if binary is not None else resolve_binary()
    stderr_target = None if stderr_mode(stderr) == "inherit" else subprocess.DEVNULL

    full_env: Optional[Dict[str, str]] = None
    if env is not None:
        full_env = dict(os.environ)
        full_env.update(env)

    argv = [resolved, "--stdio", *args]
    proc = subprocess.Popen(
        argv,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=stderr_target,
        cwd=cwd,
        env=full_env,
    )

    sidecar = Sidecar(proc, on_event)
    try:
        hello = sidecar.call("hello", {}, deadline=startup_timeout)
    except WreError:
        sidecar.close()
        raise

    protocol = hello.get("protocol")
    if protocol != expect_protocol:
        sidecar.close()
        raise ProtocolError(
            f"protocol mismatch: expected {expect_protocol}, got {protocol}",
            retryable=False,
        )

    schema_hash = hello.get("schema_hash")
    if expect_schema_hash is not None and schema_hash != expect_schema_hash:
        sidecar.close()
        raise ProtocolError(
            f"schema hash mismatch: expected {expect_schema_hash}, got {schema_hash}",
            retryable=False,
        )

    sidecar._hello = hello
    return sidecar
