from __future__ import annotations
import asyncio
from time import monotonic
from typing import Any, Callable, Optional, Tuple

import backend.app


def _is_1004(reason: str | None) -> bool:
    r = (reason or "").lower()
    return "no-preferred-flow" in r or "timeout" in r
    # or "error code: 26104" in r


async def call_with_1004_retry_async(
        func: Callable[..., Tuple[bool, str, Optional[dict]]],
        *args,
        interval: float = 0.6,
        timeout: Optional[float] = None,
        to_thread: bool = False,
        **kwargs,
) -> Tuple[bool, str, Optional[dict]]:
    start = monotonic()
    attempt = 0
    last = (False, "not-called", None)

    while True:
        attempt += 1
        try:
            if to_thread:
                res = await asyncio.to_thread(func, *args, **kwargs)
            else:
                # func 也可以是 async；这行兼容两种情况
                maybe = func(*args, **kwargs)
                res = await maybe if asyncio.iscoroutine(maybe) else maybe
        except Exception as e:
            return False, f"call-exception:{e}", None

        ok, reason, resp = res  # type: ignore
        last = res

        if not _is_1004(reason):
            # 非 1004：无论 ok 与否，都结束重试
            return res
        # backend.app.PACKET_BOT.heartbeat()
        # reason 是 1004 → 继续重试
        if timeout is not None and (monotonic() - start) >= timeout:
            return False, f"retry-timeout(1004) after {attempt} tries", None

        await asyncio.sleep(interval)


def call_with_1004_retry(
        func: Callable[..., Tuple[bool, str, Optional[dict]]],
        *args,
        interval: float = 0.6,
        timeout: Optional[float] = None,
        **kwargs,
) -> Tuple[bool, str, Optional[dict]]:
    import time
    start = monotonic()
    attempt = 0

    while True:
        attempt += 1
        try:
            ok, reason, resp = func(*args, **kwargs)
        except Exception as e:
            return False, f"call-exception:{e}", None

        if not _is_1004(reason):
            return ok, reason, resp
        # backend.app.PACKET_BOT.heartbeat()
        if timeout is not None and (monotonic() - start) >= timeout:
            return False, f"retry-timeout(1004) after {attempt} tries", None

        time.sleep(interval)
