from __future__ import annotations

import asyncio
from typing import Any, Callable, Dict, List, Mapping, Optional, Sequence, Tuple, Union

from .sidecar import Session, Sidecar, connect

EventHandler = Callable[[int, str, Any], None]


class AsyncSidecar:
    def __init__(self, sidecar: Sidecar) -> None:
        self._sidecar = sidecar

    @property
    def hello(self) -> Dict[str, Any]:
        return self._sidecar.hello

    @property
    def pid(self) -> int:
        return self._sidecar.pid

    async def describe(self) -> Any:
        return await asyncio.to_thread(self._sidecar.describe)

    async def targets(self) -> Any:
        return await asyncio.to_thread(self._sidecar.targets)

    async def metrics(self) -> Any:
        return await asyncio.to_thread(self._sidecar.metrics)

    async def open(self, target: str, config: Optional[Dict[str, Any]] = None) -> "AsyncSession":
        session = await asyncio.to_thread(self._sidecar.open, target, config)
        return AsyncSession(session)

    async def call(
        self,
        op: str,
        params: Optional[Dict[str, Any]] = None,
        session: Optional[Union[str, Session]] = None,
        deadline: Optional[float] = None,
        on_event: Optional[EventHandler] = None,
        bin_part: bytes = b"",
    ) -> Any:
        return await asyncio.to_thread(
            self._sidecar.call, op, params, session, deadline, on_event, bin_part
        )

    async def call_with_binary(
        self,
        op: str,
        params: Optional[Dict[str, Any]] = None,
        session: Optional[Union[str, Session]] = None,
        deadline: Optional[float] = None,
        on_event: Optional[EventHandler] = None,
        bin_part: bytes = b"",
    ) -> Tuple[Any, bytes]:
        return await asyncio.to_thread(
            self._sidecar.call_with_binary, op, params, session, deadline, on_event, bin_part
        )

    async def close(self) -> None:
        await asyncio.to_thread(self._sidecar.close)

    async def shutdown(self) -> None:
        await asyncio.to_thread(self._sidecar.shutdown)

    async def __aenter__(self) -> "AsyncSidecar":
        return self

    async def __aexit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        await self.close()


class AsyncSession:
    def __init__(self, session: Session) -> None:
        self._session = session

    @property
    def id(self) -> str:
        return self._session.id

    @property
    def target(self) -> str:
        return self._session.target

    @property
    def ops(self) -> List[str]:
        return self._session.ops

    async def call(
        self,
        op: str,
        params: Optional[Dict[str, Any]] = None,
        deadline: Optional[float] = None,
        on_event: Optional[EventHandler] = None,
    ) -> Any:
        return await asyncio.to_thread(self._session.call, op, params, deadline, on_event)

    async def call_with_binary(
        self,
        op: str,
        params: Optional[Dict[str, Any]] = None,
        deadline: Optional[float] = None,
        on_event: Optional[EventHandler] = None,
        bin_part: bytes = b"",
    ) -> Tuple[Any, bytes]:
        return await asyncio.to_thread(
            self._session.call_with_binary, op, params, deadline, on_event, bin_part
        )

    async def warmup(self) -> Any:
        return await asyncio.to_thread(self._session.warmup)

    async def health(self) -> Any:
        return await asyncio.to_thread(self._session.health)

    async def close(self) -> None:
        await asyncio.to_thread(self._session.close)

    async def __aenter__(self) -> "AsyncSession":
        return self

    async def __aexit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        await self.close()


async def connect_async(
    binary: Optional[str] = None,
    args: Sequence[str] = (),
    env: Optional[Mapping[str, str]] = None,
    cwd: Optional[str] = None,
    stderr: str = "ignore",
    on_event: Optional[EventHandler] = None,
    expect_protocol: int = 1,
    expect_schema_hash: Optional[str] = None,
    startup_timeout: float = 30.0,
) -> AsyncSidecar:
    sidecar = await asyncio.to_thread(
        connect,
        binary=binary,
        args=args,
        env=env,
        cwd=cwd,
        stderr=stderr,
        on_event=on_event,
        expect_protocol=expect_protocol,
        expect_schema_hash=expect_schema_hash,
        startup_timeout=startup_timeout,
    )
    return AsyncSidecar(sidecar)
