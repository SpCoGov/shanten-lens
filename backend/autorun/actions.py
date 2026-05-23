from __future__ import annotations

from typing import Any, Callable, Optional, Tuple

from backend.autorun.util.retry_1004 import call_with_1004_retry_async


class AutoRunActions:
    def __init__(self, runner: Any, bot: Any) -> None:
        self.runner = runner
        self.bot = bot

    async def set_step(self, step: str) -> None:
        self.runner.current_step = step
        await self.runner._broadcast_status(safe=True)

    async def bot_call(
            self,
            func: Callable[..., Any],
            *,
            delay_sec: float = 3,
            interval: float = 3,
            timeout: float = 30,
            **kwargs: Any,
    ) -> Tuple[bool, str, Optional[dict]]:
        return await call_with_1004_retry_async(
            func,
            delay_sec=delay_sec,
            interval=interval,
            timeout=timeout,
            to_thread=True,
            **kwargs,
        )

    async def abort(self, reason: str) -> None:
        self.runner.last_error = reason
        await self.runner.abort(f"fatal: {reason}")

    async def run_or_abort(
            self,
            func: Callable[..., Any],
            *,
            log_operation: bool = False,
            action: Optional[str] = None,
            op_reason: str = "",
            op_details: Optional[dict] = None,
            retry_2691_refresh: bool = False,
            retry_2699_refresh: bool = False,
            delay_sec: float = 3,
            interval: float = 3,
            timeout: float = 30,
            **kwargs: Any,
    ) -> Tuple[bool, Optional[dict]]:
        ok, reason, resp = await self.bot_call(
            func,
            delay_sec=delay_sec,
            interval=interval,
            timeout=timeout,
            **kwargs,
        )
        action_name = action or getattr(func, "__name__", "bot_call")
        details = dict(op_details or {})
        if kwargs:
            details["args"] = dict(kwargs)
        if ok:
            if log_operation and hasattr(self.runner, "record_operation"):
                self.runner.record_operation(action_name, reason=op_reason, result="ok", details=details)
            return True, resp
        if retry_2691_refresh and reason == "error code: 2691":
            self.bot.fetch_amulet_activity_data()
            if log_operation and hasattr(self.runner, "record_operation"):
                self.runner.record_operation(action_name, reason=op_reason or "packet_state_stale", result=reason, details=details)
            return False, resp
        if retry_2699_refresh and reason == "error code: 2699":
            self.bot.fetch_amulet_activity_data()
            if log_operation and hasattr(self.runner, "record_operation"):
                self.runner.record_operation(action_name, reason=op_reason or "sell_target_missing", result=reason, details=details)
            return False, resp
        if log_operation and hasattr(self.runner, "record_operation"):
            self.runner.record_operation(action_name, reason=op_reason, result=reason, details=details)
        await self.abort(reason)
        return False, resp

    async def remake(self, reason: str, game_state: Any) -> None:
        await self.set_step("game.remake")
        self.runner._record_remake_snapshot(reason, game_state)
        await self.runner._broadcast_status(safe=True)
        self.runner.need_start_game = True
        await self.run_or_abort(self.bot.giveup)
