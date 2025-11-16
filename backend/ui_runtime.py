from __future__ import annotations
import asyncio
from threading import Thread, Lock, current_thread
from loguru import logger
from typing import Optional, Coroutine, Any

_UI_LOOP: Optional[asyncio.AbstractEventLoop] = None
_UI_THREAD: Optional[Thread] = None
_UI_LOCK = Lock()
_UI_SERVICES_STARTED = False


def _runner(loop: asyncio.AbstractEventLoop):
    asyncio.set_event_loop(loop)
    logger.info("UI loop started on thread {}", current_thread().name)
    if loop.is_running():
        return
    try:
        loop.run_forever()
    finally:
        try:
            pending = asyncio.all_tasks(loop)
            for t in pending:
                t.cancel()
        except Exception:
            pass
        try:
            loop.stop()
        except Exception:
            pass


def start_ui_loop_once() -> asyncio.AbstractEventLoop:
    global _UI_LOOP, _UI_THREAD
    with _UI_LOCK:
        if _UI_LOOP and _UI_THREAD and _UI_THREAD.is_alive():
            return _UI_LOOP
        if _UI_LOOP and _UI_LOOP.is_running():
            return _UI_LOOP

        loop = asyncio.new_event_loop()
        th = Thread(target=_runner, args=(loop,), name="UI-EventLoop", daemon=True)
        th.start()
        _UI_LOOP, _UI_THREAD = loop, th
        return loop


def get_ui_loop() -> asyncio.AbstractEventLoop:
    loop = start_ui_loop_once()
    return loop


def post_coro(coro: Coroutine[Any, Any, Any]) -> None:
    loop = get_ui_loop()
    asyncio.run_coroutine_threadsafe(coro, loop)


def mark_ui_services_started() -> bool:
    global _UI_SERVICES_STARTED
    with _UI_LOCK:
        if _UI_SERVICES_STARTED:
            return False
        _UI_SERVICES_STARTED = True
        return True
