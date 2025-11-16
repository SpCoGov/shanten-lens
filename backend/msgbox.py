from __future__ import annotations

import asyncio
import secrets
import threading
from typing import Dict, MutableMapping, Final

from loguru import logger

from backend.app import post_broadcast

_PENDING_ASYNC: MutableMapping[str, asyncio.Future] = {}
_PENDING_SYNC: MutableMapping[str, tuple[threading.Event, Dict[str, bool]]] = {}
_DEFAULT_TIMEOUT: Final[float] = 45.0


def _new_id() -> str:
    return secrets.token_hex(8)


async def ui_confirm(*, title_key, message_key, values=None,
                     ok_key="common.ok", cancel_key="common.cancel",
                     timeout=_DEFAULT_TIMEOUT) -> bool:
    mid = _new_id()
    loop = asyncio.get_running_loop()
    fut: asyncio.Future = loop.create_future()
    _PENDING_ASYNC[mid] = fut

    pkt = {
        "type": "msgbox",
        "data": {
            "id": mid,
            "title": title_key,
            "message": message_key,
            "okText": ok_key,
            "cancelText": cancel_key,
            "values": values or {},
        },
    }
    post_broadcast(pkt)

    try:
        ok: bool = await asyncio.wait_for(fut, timeout=timeout)
        return bool(ok)
    except asyncio.TimeoutError:
        return False
    finally:
        _PENDING_ASYNC.pop(mid, None)


def ui_confirm_sync(*, title_key, message_key, values=None,
                    ok_key="common.continue", cancel_key="common.cancel",
                    timeout=_DEFAULT_TIMEOUT) -> bool:
    logger.info("ui_confirm_sync: on ui confirm")
    mid = _new_id()
    ev = threading.Event()
    holder: Dict[str, bool] = {"ok": False}
    _PENDING_SYNC[mid] = (ev, holder)

    pkt = {
        "type": "msgbox",
        "data": {
            "id": mid,
            "title": title_key,
            "message": message_key,
            "okText": ok_key,
            "cancelText": cancel_key,
            "values": values or {},
        },
    }
    post_broadcast(pkt)

    signaled = ev.wait(timeout)
    _PENDING_SYNC.pop(mid, None)
    return holder["ok"] if signaled else False


def _complete(mid: str, ok: bool) -> None:
    fut = _PENDING_ASYNC.get(mid)
    if fut and not fut.done():
        try:
            fut.set_result(bool(ok))
        except Exception:
            logger.exception("set_result failed")

    pair = _PENDING_SYNC.get(mid)
    if pair:
        ev, holder = pair
        holder["ok"] = bool(ok)
        ev.set()


def handle_msgbox_result(pkt: dict) -> None:
    try:
        data = pkt.get("data") or {}
        mid = str(data.get("id") or "")
        ok = bool(data.get("ok"))
        if mid:
            _complete(mid, ok)
    except Exception:
        logger.exception("handle_msgbox_result failed")


def _ui_confirm_blocking(*, title_key, message_key, values=None,
                         ok_key="common.continue", cancel_key="common.cancel",
                         timeout=45.0) -> bool:
    return ui_confirm_sync(
        title_key=title_key,
        message_key=message_key,
        values=values or {},
        ok_key=ok_key,
        cancel_key=cancel_key,
        timeout=timeout,
    )
