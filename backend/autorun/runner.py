from __future__ import annotations

import asyncio
import time
import traceback
from typing import Any, Dict, List, Optional, Tuple

from backend.autorun.util.retry_1004 import call_with_1004_retry_async
from backend.autorun.util.suannkou_recommender import plan_pure_pinzu_suu_ankou_v2
from backend.bot.drivers.packet.packet_bot import PacketBot
from backend.model.game_state import GameState

try:
    import backend.app as app_mod
except Exception:
    app_mod = None

from loguru import logger


def _now_wall_ms() -> int:
    return int(time.time() * 1000)


def _now_mono_ms() -> int:
    return int(time.monotonic() * 1000)

# TODO: 开局前、和牌前调整卡维、盗印like的位置；如果目标护身符为非plus且有印章、如果卡包里遇到了印章不对应的该护身符、将其的价值视为0而不是99
class AutoRunner:
    PROBE_DEBUG = True
    HEARTBEAT_INTERVAL = 1.0  # s

    def __init__(self, *, get_config, get_game_state) -> None:
        self._get_config = get_config
        self._get_game_state = get_game_state

        self._lock = asyncio.Lock()

        # 运行/调试模式
        self.mode: str = "continuous"  # "continuous" | "step"
        self._loop_task: Optional[asyncio.Task] = None
        self._heartbeat_task: Optional[asyncio.Task] = None

        # 运行态
        self.running: bool = False
        self.started_at: int = 0  # wall clock epoch ms
        self._started_mono_ms: int = 0  # monotonic start
        self.elapsed_ms: int = 0
        self.runs: int = 0
        self.best_achieved_count: int = 0
        self.current_step: str = "-"
        self.last_error: Optional[str] = None
        self.need_start_game = False

        # 最近一次“手动探测”
        self._last_probe_ts: int = 0
        self._last_probe_ok: Optional[bool] = None
        self._last_probe_reason: str = ""
        self._last_probe_resp: Optional[dict] = None

        # 就绪状态
        self.game_ready_reason: str = "未探测，请点击“刷新状态”"
        self.game_ready_code: str = "NOT_PROBED"  # "", "NOT_PROBED", "BUSINESS_REFUSED", "GAME_NOT_READY", "PROBE_TIMEOUT"
        self._probe_fail_count: int = 0

        # 配置
        self.end_count: int = 1
        self.targets: List[Dict[str, Any]] = []
        self.cutoff_level: int = 0

        self.update_config(self._get_config())

    def _get_packet_bot(self):
        try:
            return getattr(app_mod, "PACKET_BOT", None)
        except Exception:
            return None

    async def _get_broadcast_coro(self):
        try:
            return getattr(app_mod, "broadcast", None)
        except Exception:
            return None

    def update_config(self, cfg: Dict[str, Any]) -> None:
        self.end_count = max(1, int((cfg or {}).get("end_count", 1) or 1))
        self.targets = list((cfg or {}).get("targets") or [])
        try:
            self.cutoff_level = int((cfg or {}).get("cutoff_level", 0) or 0)
        except Exception:
            self.cutoff_level = 0
        if self.PROBE_DEBUG:
            logger.info(f"[autorun] config updated end_count={self.end_count} cutoff_level={self.cutoff_level} targets={len(self.targets)}")

    def _calc_elapsed_ms(self) -> int:
        if not self.running:
            return max(0, self.elapsed_ms)
        now_m = _now_mono_ms()
        add = (now_m - self._started_mono_ms) if self._started_mono_ms else 0
        return max(0, self.elapsed_ms + add)

    def invalidate_probe(self) -> None:
        self._last_probe_ts = 0
        self._last_probe_ok = None
        self._last_probe_reason = ""
        self._last_probe_resp = None
        self.game_ready_reason = "未探测，请点击“刷新状态”"
        self.game_ready_code = "NOT_PROBED"
        self._probe_fail_count = 0
        if self.PROBE_DEBUG:
            logger.info("[autorun] probe state cleared (NOT_PROBED)")

    def _classify_probe_reason(self, reason: str) -> str:
        r = (reason or "").lower().strip()
        not_ready_keys = (
            "addon-or-flow-not-ready", "addon or flow not ready",
            "flow-not-ready", "addon-not-ready", "not_ready", "not-ready", "not ready",
            "no session", "no game", "service unavailable",
        )
        if any(k in r for k in not_ready_keys):
            return "GAME_NOT_READY"
        timeout_keys = (
            "timeout", "timed out", "time out", "deadline",
            "read timeout", "connection refused", "connection reset",
            "cannot connect", "failed to establish", "econn",
            "bad gateway", "network", "winerror", "proxy", "connect error",
        )
        if any(k in r for k in timeout_keys):
            return "PROBE_TIMEOUT"
        if "error code: 1004" in r or "code: 1004" in r:
            return "BUSINESS_REFUSED"
        return ""

    async def refresh_probe_now(self, *, push: bool = False):
        bot = self._get_packet_bot()
        if bot is None:
            ok, reason, resp = False, "PACKET_BOT missing", None
            if self.PROBE_DEBUG:
                logger.warning("[autorun] PACKET_BOT is None (manual probe)")
        else:
            try:
                if self.PROBE_DEBUG:
                    logger.info("[autorun] calling fetch_amulet_activity_data() (manual)")
                ok, reason, resp = await asyncio.to_thread(bot.fetch_amulet_activity_data)
            except Exception as e:
                ok, reason, resp = False, f"probe_error: {e}", None
                logger.exception("[autorun] manual probe exception")

        self._last_probe_ts = _now_mono_ms()
        self._last_probe_ok = ok
        self._last_probe_reason = reason or ""
        self._last_probe_resp = resp

        await self._recompute_ready_flags_from_last_probe()
        if push:
            await self._broadcast_status(safe=True)
        return ok, reason, resp

    async def _recompute_ready_flags_from_last_probe(self) -> None:
        ok = self._last_probe_ok
        reason = self._last_probe_reason
        code = self._classify_probe_reason(reason)

        if ok is None:
            self.game_ready_reason = "未探测，请点击“刷新状态”"
            self.game_ready_code = "NOT_PROBED"
            return

        if ok:
            self.game_ready_reason = ""
            self.game_ready_code = ""
            self._probe_fail_count = 0
            if self.PROBE_DEBUG:
                ...
                # logger.info("[autorun] READY (ok=True)")
            return

        if code == "BUSINESS_REFUSED":
            self.game_ready_reason = ""
            self.game_ready_code = ""
            self._probe_fail_count = 0
            if self.PROBE_DEBUG:
                logger.info("[autorun] READY (business refused 1004)")
            return

        self.game_ready_reason = reason or "unknown"
        self.game_ready_code = code or "PROBE_TIMEOUT"
        self._probe_fail_count += 1
        if self.PROBE_DEBUG:
            logger.info(f"[autorun] NOT READY code={self.game_ready_code} reason={self.game_ready_reason!r}")

    async def is_game_ready_async(self) -> bool:
        await self._recompute_ready_flags_from_last_probe()
        return self.game_ready_code in ("",)

    async def has_live_game_async(self) -> bool:
        resp = self._last_probe_resp or {}
        game = (resp or {}).get("data", {}).get("game")
        return game is not None

    async def set_mode(self, mode: str) -> None:
        if mode not in ("continuous", "step"):
            return
        async with self._lock:
            self.mode = mode
            # 切到 step 时，如果正在跑循环，停掉循环但保留运行态
            if self.mode == "step" and self._loop_task and not self._loop_task.done():
                self._loop_task.cancel()
                self._loop_task = None
            await self._broadcast_status(safe=True)

    async def start(self) -> None:
        async with self._lock:
            if self.running:
                return
            if not await self.is_game_ready_async():
                raise RuntimeError(self.game_ready_reason or "未就绪")

            self.update_config(self._get_config())
            self.running = True
            self.started_at = _now_wall_ms()
            self._started_mono_ms = _now_mono_ms()
            self.current_step = "init"
            self.last_error = None
            self.runs = 0

            self.need_start_game = True

            # 心跳：每秒推一次状态
            if self._heartbeat_task and not self._heartbeat_task.done():
                self._heartbeat_task.cancel()
            self._heartbeat_task = asyncio.create_task(self._heartbeat_loop(), name="autorun.heartbeat")

            # 仅在连续模式下起主循环
            if self.mode == "continuous":
                if self._loop_task and not self._loop_task.done():
                    self._loop_task.cancel()
                self._loop_task = asyncio.create_task(self._main_loop(), name="autorun.loop")

            await self._broadcast_status(safe=True)
            if self.PROBE_DEBUG:
                logger.info(f"[autorun] started (mode={self.mode})")

    async def stop(self) -> None:
        async with self._lock:
            if not self.running:
                return
            self.running = False
            self.elapsed_ms = self._calc_elapsed_ms()
            self._started_mono_ms = 0

            if self._loop_task and not self._loop_task.done():
                self._loop_task.cancel()
            self._loop_task = None

            if self._heartbeat_task and not self._heartbeat_task.done():
                self._heartbeat_task.cancel()
            self._heartbeat_task = None

            self.current_step = "stopped"
            await self._broadcast_status(safe=True)
            if self.PROBE_DEBUG:
                logger.info("[autorun] stopped")

    async def _heartbeat_loop(self) -> None:
        try:
            while self.running:
                await self._broadcast_status(safe=True)
                await asyncio.sleep(self.HEARTBEAT_INTERVAL)
        except asyncio.CancelledError:
            pass

    async def _main_loop(self) -> None:
        try:
            while self.running and self.mode == "continuous":
                try:
                    await self.run_tick()
                except asyncio.CancelledError:
                    raise
                except Exception as e:
                    self.last_error = str(e)
                    logger.exception("[autorun] run_tick error")
                await asyncio.sleep(0.05)
        except asyncio.CancelledError:
            pass

    async def step_once(self) -> None:
        if not self.running:
            raise RuntimeError("未启动，无法单步")
        if self.mode != "step":
            raise RuntimeError("当前非调试模式")
        await self.run_tick()
        await self._broadcast_status(safe=True)

    async def run_tick(self) -> None:
        try:
            await asyncio.sleep(0.1)
            bot: PacketBot = self._get_packet_bot()
            game_state: GameState = self._get_game_state()
            if await self._check_and_finish_if_done():
                return
            if self.need_start_game:
                self.current_step = "start_game"
                await self._broadcast_status(safe=True)
                ok, reason, resp = await call_with_1004_retry_async(
                    bot.start_game,
                    delay_sec=3,
                    interval=0.4,
                    timeout=30,
                    to_thread=True,
                )
                if ok:
                    self.runs += 1
                    self.need_start_game = False
                    return
                self.last_error = reason
                await self.abort(f"fatal: {reason}")
                return
            if game_state.stage == 1:
                self.current_step = "game.select_free_effect"
                await self._broadcast_status(safe=True)
                first_effect = game_state.candidate_effect_list[0].get("id")
                ok, reason, resp = await call_with_1004_retry_async(
                    bot.select_free_effect,
                    selected_id=first_effect,
                    delay_sec=3,
                    interval=0.4,
                    timeout=30,
                    to_thread=True,
                )
                if ok:
                    return
                self.last_error = reason
                await self.abort(f"fatal: {reason}")
                return
            if game_state.stage == 6:
                self.current_step = "game.level_confirm"
                await self._broadcast_status(safe=True)
                ok, reason, resp = await call_with_1004_retry_async(
                    bot.next_level,
                    delay_sec=3,
                    interval=0.4,
                    timeout=30,
                    to_thread=True,
                )
                if ok:
                    return
                self.last_error = reason
                await self.abort(f"fatal: {reason}")
                return
            if game_state.stage == 2:
                self.current_step = f"game.change_tile({game_state.change_tile_count}/{game_state.total_change_tile_count})"
                await self._broadcast_status(safe=True)
                if game_state.change_tile_count >= game_state.total_change_tile_count:
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.op_skip_change,
                        delay_sec=3,
                        interval=0.4,
                        timeout=30,
                        to_thread=True,
                    )
                    if ok:
                        return
                    self.last_error = reason
                    await self.abort(f"fatal: {reason}")
                    return
                # 把非饼全换了
                filtered_ids = [tid for tid in game_state.hand_tiles
                                if (face := game_state.deck_map.get(tid)) is not None
                                and (face == "bd"
                                     or face.endswith("p"))]
                ok, reason, resp = await call_with_1004_retry_async(
                    bot.op_change,
                    tile_ids=filtered_ids,
                    delay_sec=3,
                    interval=0.4,
                    timeout=30,
                    to_thread=True,
                )
                if ok:
                    return
                self.last_error = reason
                await self.abort(f"fatal: {reason}")
                return
            if game_state.stage == 3:
                self.current_step = f"game.discard"
                await self._broadcast_status(safe=True)
                suuannkou = plan_pure_pinzu_suu_ankou_v2(game_state.hand_tiles, game_state.wall_tiles, game_state.deck_map)
                if suuannkou["status"] == "impossible":
                    self.current_step = "game.remake"
                    await self._broadcast_status(safe=True)
                    self.need_start_game = True
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.giveup,
                        delay_sec=3,
                        interval=0.4,
                        timeout=30,
                        to_thread=True,
                    )
                    if ok:
                        return
                    self.last_error = reason
                    await self.abort(f"fatal: {reason}")
                    return
                elif suuannkou["status"] == "win_now":
                    self.current_step = "game.tsumo"
                    await self._broadcast_status(safe=True)
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.op_tsumo,
                        delay_sec=3,
                        interval=0.4,
                        timeout=30,
                        to_thread=True,
                    )
                    if ok:
                        return
                    self.last_error = reason
                    await self.abort(f"fatal: {reason}")
                    return
                elif suuannkou["status"] == "plan":
                    discard = suuannkou["discards"][0]
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.discard_by_tile_id,
                        tile_id=discard,
                        delay_sec=3,
                        interval=0.4,
                        timeout=30,
                        to_thread=True,
                    )
                    if ok:
                        return
                    self.last_error = reason
                    await self.abort(f"fatal: {reason}")
                    return
            if game_state.stage == 4:
                self.current_step = "game.buy_pack"
                await self._broadcast_status(safe=True)
                logger.debug(game_state.goods)
                candidates = [g for g in game_state.goods if not g.get("sold", False)]
                if not candidates:
                    if game_state.refresh_price > game_state.coin:
                        # 如果当前已经到了截至关卡、则remake
                        if self.cutoff_level >= game_state.level:
                            self.current_step = "game.remake"
                            await self._broadcast_status(safe=True)
                            self.need_start_game = True
                            ok, reason, resp = await call_with_1004_retry_async(
                                bot.giveup,
                                delay_sec=3,
                                interval=0.4,
                                timeout=30,
                                to_thread=True,
                            )
                            if ok:
                                return
                            self.last_error = reason
                            await self.abort(f"fatal: {reason}")
                            return
                        self.current_step = "game.end_shopping"
                        await self._broadcast_status(safe=True)
                        ok, reason, resp = await call_with_1004_retry_async(
                            bot.end_shopping,
                            delay_sec=3,
                            interval=0.4,
                            timeout=30,
                            to_thread=True,
                        )
                        if ok:
                            return
                        self.last_error = reason
                        await self.abort(f"fatal: {reason}")
                        return
                    self.current_step = "game.refresh_shop"
                    await self._broadcast_status(safe=True)
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.refresh_shop,
                        delay_sec=3,
                        interval=0.4,
                        timeout=30,
                        to_thread=True,
                    )
                    if ok:
                        return
                    self.last_error = reason
                    await self.abort(f"fatal: {reason}")
                    return
                candidates.sort(key=lambda g: (
                    int(g.get("price", 1_000_000)),
                    int(g.get("goodsId", 1_000_000)),
                    int(g.get("id", 1_000_000)),
                ))
                cheapest = candidates[0]
                if cheapest["price"] > game_state.coin:
                    # 最便宜的都买不起、刷新商店、刷新商店也没钱就下一关
                    if game_state.refresh_price > game_state.coin:
                        self.current_step = "game.end_shopping"
                        await self._broadcast_status(safe=True)
                        ok, reason, resp = await call_with_1004_retry_async(
                            bot.end_shopping,
                            delay_sec=3,
                            interval=0.4,
                            timeout=30,
                            to_thread=True,
                        )
                        if ok:
                            return
                        self.last_error = reason
                        await self.abort(f"fatal: {reason}")
                        return
                    self.current_step = "game.refresh_shop"
                    await self._broadcast_status(safe=True)
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.refresh_shop,
                        delay_sec=3,
                        interval=0.4,
                        timeout=30,
                        to_thread=True,
                    )
                    if ok:
                        return
                    self.last_error = reason
                    await self.abort(f"fatal: {reason}")
                    return
                ok, reason, resp = await call_with_1004_retry_async(
                    bot.buy_pack,
                    good_id=cheapest["id"],
                    delay_sec=3,
                    interval=0.4,
                    timeout=30,
                    to_thread=True,
                )
                if ok:
                    return
                self.last_error = reason
                await self.abort(f"fatal: {reason}")
                return
            if game_state.stage == 5 or game_state.stage == 7:
                self.current_step = "game.select_effect"
                await self._broadcast_status(safe=True)
                # value: 99-目标护身符、2-指引护身符（当前持有的指引护身符未满3个的情况下）、1-幸福护身符、0-普通
                best_raw, best_bid, value = select_amulet_from_candidates(game_state.candidate_effect_list, game_state.effect_list, self.targets)
                need_space = 1
                if best_bid == 600160:
                    need_space = 2
                used_space = total_volume(game_state.effect_list)
                free_space = game_state.max_effect_volume - used_space
                logger.debug(f"free space: {free_space}, needed space: {used_space}, used_space: {used_space}, max space: {game_state.max_effect_volume}")
                # 如果空间充足、直接选择
                if free_space >= need_space:
                    if game_state.stage == 5:
                        ok, reason, resp = await call_with_1004_retry_async(
                            bot.select_effect,
                            selected_id=best_raw,
                            delay_sec=3,
                            interval=0.4,
                            timeout=30,
                            to_thread=True,
                        )
                    else:
                        ok, reason, resp = await call_with_1004_retry_async(
                            bot.select_reward_effect,
                            selected_id=best_raw,
                            delay_sec=3,
                            interval=0.4,
                            timeout=30,
                            to_thread=True,
                        )
                    if ok:
                        # 如果选择的护身符不是什么重要的护身符、直接卖
                        if value == 0:
                            uid = find_uid_for_raw_or_plus(game_state.effect_list, best_raw)
                            if uid:
                                self.current_step = "game.sell_useless_effect"
                                await self._broadcast_status(safe=True)
                                ok, reason, resp = await call_with_1004_retry_async(
                                    bot.sell_effect,
                                    uid=uid,
                                    delay_sec=3,
                                    interval=0.4,
                                    timeout=30,
                                    to_thread=True,
                                )
                                if ok:
                                    return
                                self.last_error = reason
                                await self.abort(f"fatal: {reason}")
                            return
                        return
                    self.last_error = reason
                    await self.abort(f"fatal: {reason}")
                    return
                if value >= 99:
                    # 护身符是目标所需的护身符、但是空间不足、卖掉其他不重要的以换取空间
                    # sort_sell_priority会按照优先级列出可以卖的护身符列表：List[Dict[str, Any]]，其中每个字典包含一个体积（volume）字段，卖掉这个护身符即可获得对应体积字段的空间。按照优先级选出要卖的护身符、以便剩余的空间充足足以买下新护身符，如果卖掉所有的可以卖的护身符列表里的护身符的都没办法腾出足够的空间的时候、跳过购买
                    sell_list = sort_sell_priority(game_state.effect_list, self.targets)
                    to_sell, freed, enough = select_items_to_sell_for_purchase(
                        free_space=free_space,
                        need_space=need_space,
                        sell_candidates=sell_list,
                    )

                    if enough:
                        for it in to_sell:
                            uid = it.get("uid")
                            if uid is None:
                                continue
                            ok, reason, resp = await call_with_1004_retry_async(
                                bot.sell_effect,
                                uid=uid,
                                delay_sec=3,
                                interval=0.6,
                                timeout=30,
                                to_thread=True,
                            )
                            if ok:
                                continue
                            self.last_error = reason
                            await self.abort(f"fatal: {reason}")
                            return
                    else:
                        self.current_step = "game.skip_buy_insufficient_space0"
                        if game_state.stage == 5:
                            ok, reason, resp = await call_with_1004_retry_async(
                                bot.select_effect,
                                selected_id=0,
                                delay_sec=3,
                                interval=0.4,
                                timeout=30,
                                to_thread=True,
                            )
                        else:
                            ok, reason, resp = await call_with_1004_retry_async(
                                bot.select_reward_effect,
                                selected_id=0,
                                delay_sec=3,
                                interval=0.4,
                                timeout=30,
                                to_thread=True,
                            )
                        if ok:
                            return
                        self.last_error = reason
                        await self.abort(f"fatal: {reason}")
                    return
                # 又不重要、空间还不够、直接不买、跳过
                self.current_step = "game.skip_buy_insufficient_space1"
                if game_state.stage == 5:
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.select_effect,
                        selected_id=0,
                        delay_sec=3,
                        interval=0.4,
                        timeout=30,
                        to_thread=True,
                    )
                else:
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.select_reward_effect,
                        selected_id=0,
                        delay_sec=3,
                        interval=0.4,
                        timeout=30,
                        to_thread=True,
                    )
                if ok:
                    return
                self.last_error = reason
                await self.abort(f"fatal: {reason}")
                return
        except Exception as e:
            logger.opt(exception=e).exception("run_tick error")
            full_tb = traceback.format_exc()
            logger.debug("Full traceback:\n{}", full_tb)
            await self.abort(f"fatal: {e}")

    async def abort(self, reason: str = "fatal error", *, push: bool = True) -> None:
        async with self._lock:
            self.last_error = reason or "fatal error"
            self.running = False
            self.elapsed_ms = self._calc_elapsed_ms()
            self._started_mono_ms = 0
            # 停掉主循环
            if self._loop_task and not self._loop_task.done():
                self._loop_task.cancel()
            self._loop_task = None
            # 停掉心跳
            if self._heartbeat_task and not self._heartbeat_task.done():
                self._heartbeat_task.cancel()
            self._heartbeat_task = None
            if push:
                await self._broadcast_status(safe=True)

    async def status_payload_async(self) -> Dict[str, Any]:
        resp = self._last_probe_resp or {}
        has_live_game = (resp or {}).get("data", {}).get("game") is not None
        await self._recompute_ready_flags_from_last_probe()
        return {
            "mode": self.mode,
            "running": self.running,
            "runs": self.runs,
            "elapsed_ms": self._calc_elapsed_ms(),
            "best_achieved_count": self.best_achieved_count,
            "current_step": self.current_step or "-",
            "last_error": self.last_error,
            "started_at": self.started_at or 0,
            "game_ready": (self.game_ready_code == ""),
            "has_live_game": has_live_game,
            "game_ready_reason": self.game_ready_reason,
            "game_ready_code": self.game_ready_code,
            "probe_fail_count": self._probe_fail_count,
            "probe_ok": self._last_probe_ok,
            "probe_reason": self._last_probe_reason,
            "probe_at": self._last_probe_ts,
        }

    async def _broadcast_status(self, safe: bool = False) -> None:
        bc = await self._get_broadcast_coro()
        if bc is None:
            return
        payload = {"type": "autorun_status", "data": await self.status_payload_async()}
        if safe:
            try:
                await bc(payload)
            except Exception:
                pass
        else:
            await bc(payload)

    @staticmethod
    def _extract_amulet_signature(effect_item: Dict[str, Any]) -> Tuple[int, bool, Optional[int]]:
        try:
            raw_id = int(effect_item.get("id"))
        except Exception:
            raw_id = 0
        reg_id = raw_id // 10
        is_plus = (raw_id % 10 == 1)

        badge = effect_item.get("badge")
        if isinstance(badge, dict) and ("id" in badge):
            try:
                badge_id = int(badge["id"])
            except Exception:
                badge_id = None
        else:
            badge_id = None

        return reg_id, is_plus, badge_id

    def amulet_matches_target(self, effect_item: Dict[str, Any], target: Dict[str, Any]) -> bool:
        reg_id, is_plus, badge_id = self._extract_amulet_signature(effect_item)
        kind = target.get("kind")

        if kind == "badge":
            try:
                need_badge = int(target.get("id"))
            except Exception:
                return False
            return (badge_id is not None) and (badge_id == need_badge)

        if kind == "amulet":
            try:
                need_reg = int(target.get("id"))
            except Exception:
                return False
            if reg_id != need_reg:
                return False

            need_plus = bool(target.get("plus", False))
            tb = target.get("badge", None)
            need_badge = None
            if tb is not None and tb != "":
                try:
                    need_badge = int(tb)
                except Exception:
                    need_badge = None

            if need_badge is None:
                return is_plus is True if need_plus else is_plus is False

            if badge_id != need_badge:
                return False
            return is_plus is True if need_plus else is_plus is False

        return False

    def match_targets_for_amulet(self, effect_item: Dict[str, Any], targets: List[Dict[str, Any]]) -> List[int]:
        hits: List[int] = []
        for i, t in enumerate(targets or []):
            if self.amulet_matches_target(effect_item, t):
                hits.append(i)
        return hits

    def count_achieved_now(self) -> int:
        gs = self._get_game_state()
        try:
            d = gs.to_dict() if hasattr(gs, "to_dict") else (gs or {})
        except Exception:
            d = (gs or {})
        eff_list = d.get("effect_list") or []

        hit: set[int] = set()
        for item in eff_list:
            for idx in self.match_targets_for_amulet(item, self.targets):
                hit.add(idx)
        return len(hit)

    async def _check_and_finish_if_done(self) -> bool:
        try:
            achieved = self.count_achieved_now()
        except Exception as e:
            logger.error("count_achieved_now failed: {}", e)
            return False

        # 刷新历史最好
        if achieved > (self.best_achieved_count or 0):
            self.best_achieved_count = achieved

        if achieved >= (self.end_count or 1):
            self.current_step = "goal_met"
            await self._broadcast_status(safe=True)
            await self.stop()
            return True
        return False


def _reg_id_of_raw(raw_id: int) -> int:
    return int(raw_id) // 10


def _candidate_badge_id(c: Dict[str, Any]) -> Optional[int]:
    try:
        bid = int(c.get("badgeId", 0))
        return bid if bid > 0 else None
    except Exception:
        return None


def _owned_badge_ids(effect_list: List[Dict[str, Any]]) -> List[Optional[int]]:
    res: List[Optional[int]] = []
    for e in effect_list or []:
        b = e.get("badge")
        if isinstance(b, dict) and "id" in b:
            try:
                res.append(int(b["id"]))
            except Exception:
                res.append(None)
        else:
            res.append(None)
    return res


def _owned_count_with_badge(effect_list: List[Dict[str, Any]], want_badge: int) -> int:
    cnt = 0
    for e in effect_list or []:
        b = e.get("badge")
        if isinstance(b, dict) and "id" in b:
            try:
                if int(b["id"]) == int(want_badge):
                    cnt += 1
            except Exception:
                pass
    return cnt


def _candidate_value(raw_id: int, badge_id: Optional[int]) -> int:
    reg_id = _reg_id_of_raw(raw_id)
    base = 0
    try:
        amulet_reg = app_mod.AMULET_REG
        if amulet_reg is not None:
            item = amulet_reg.get(reg_id)
            if item and getattr(item, "rarity", None) is not None:
                rarity_val = int(getattr(item.rarity, "value", 0))
                base = rarity_val * 3
    except Exception:
        base = 0
    if badge_id == 600050:
        base *= 3
    return base


def select_amulet_from_candidates(
        candidate_effect_list: List[Dict[str, Any]],
        effect_list: List[Dict[str, Any]],
        targets: List[Dict[str, Any]],
) -> Tuple[Optional[int], Optional[int], Optional[int]]:
    if not candidate_effect_list:
        return None, None, None

    want_badges = set()
    want_amulet_regs = set()
    for t in targets or []:
        k = t.get("kind")
        if k == "badge":
            try:
                want_badges.add(int(t.get("id")))
            except Exception:
                pass
        elif k == "amulet":
            try:
                want_amulet_regs.add(int(t.get("id")))
            except Exception:
                pass

    for c in candidate_effect_list:
        raw_id = int(c.get("id", 0))
        if raw_id <= 0:
            continue
        reg_id = _reg_id_of_raw(raw_id)
        bid = _candidate_badge_id(c)
        if reg_id in want_amulet_regs or (bid is not None and bid in want_badges):
            return raw_id, bid, 99

    WANT_BADGE_STACK = 600070
    owned_cnt_600070 = _owned_count_with_badge(effect_list, WANT_BADGE_STACK)
    if owned_cnt_600070 < 3:
        for c in candidate_effect_list:
            bid = _candidate_badge_id(c)
            if bid == WANT_BADGE_STACK:
                return int(c["id"]), bid, 2

    for c in candidate_effect_list:
        bid = _candidate_badge_id(c)
        if bid == 600110:
            return int(c["id"]), bid, 1

    best_raw: Optional[int] = None
    best_bid: Optional[int] = None
    best_val = -10 ** 9
    for c in candidate_effect_list:
        try:
            raw_id = int(c.get("id", 0))
        except Exception:
            continue
        if raw_id <= 0:
            continue
        bid = _candidate_badge_id(c)
        val = _candidate_value(raw_id, bid)
        if val > best_val:
            best_val = val
            best_raw = raw_id
            best_bid = bid

    return best_raw, best_bid, 0


def total_volume(effect_list: List[Dict[str, Any]]) -> int:
    s = 0
    for it in effect_list or []:
        try:
            v = int(it.get("volume", 0))
        except Exception:
            v = 0
        s += max(0, v)
    return s


def find_uid_for_raw_or_plus(effect_list: List[Dict[str, Any]], best_raw: int) -> Optional[int]:
    try:
        raw = int(best_raw)
    except Exception:
        return None
    if raw <= 0:
        return None
    reg = raw // 10
    target_ids = {raw, reg * 10 + 1}  # 同号非plus/plus都匹配
    for it in effect_list or []:
        try:
            if int(it.get("id", -1)) in target_ids:
                uid = it.get("uid")
                return int(uid) if uid is not None else None
        except Exception:
            continue
    return None


def _extract_amulet_signature(effect_item: Dict[str, Any]) -> Tuple[int, bool, Optional[int]]:
    try:
        raw_id = int(effect_item.get("id", 0))
    except Exception:
        raw_id = 0
    reg_id = raw_id // 10
    is_plus = (raw_id % 10 == 1)

    badge = effect_item.get("badge")
    if isinstance(badge, dict) and "id" in badge:
        try:
            badge_id = int(badge["id"])
        except Exception:
            badge_id = None
    else:
        badge_id = None

    return reg_id, is_plus, badge_id


def _is_needed_for_any_target(effect_item: Dict[str, Any], targets: List[Dict[str, Any]]) -> bool:
    reg_id, _is_plus, badge_id = _extract_amulet_signature(effect_item)

    for t in targets or []:
        k = t.get("kind")
        if k == "badge":
            try:
                need_badge = int(t.get("id"))
            except Exception:
                continue
            if badge_id is not None and badge_id == need_badge:
                return True
        elif k == "amulet":
            try:
                need_reg = int(t.get("id"))
            except Exception:
                continue
            if reg_id == need_reg:
                return True
    return False


def sort_sell_priority(effect_list: List[Dict[str, Any]], targets: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    if not effect_list:
        return []

    KEEP_BADGE = 600070
    demoted: List[Dict[str, Any]] = []
    normal: List[Dict[str, Any]] = []
    demoted_taken = 0

    for it in effect_list:
        if _is_needed_for_any_target(it, targets):
            continue  # 目标需要的护身符：移出结果

        _, __, badge_id = _extract_amulet_signature(it)
        if badge_id == KEEP_BADGE and demoted_taken < 3:
            demoted.append(it)  # 降权：排在最后
            demoted_taken += 1
        else:
            normal.append(it)  # 照常顺序

    # 正常项在前，降权项在后（降权＝卖得更晚）
    return normal + demoted


def select_items_to_sell_for_purchase(
        free_space: int,
        need_space: int,
        sell_candidates: List[Dict[str, Any]],
) -> Tuple[List[Dict[str, Any]], int, bool]:
    if need_space <= free_space:
        return [], 0, True

    gap = need_space - max(0, free_space)
    chosen: List[Dict[str, Any]] = []
    freed = 0
    for it in sell_candidates:
        v = int(it.get("volume", 0) or 0)
        if v <= 0:
            continue
        chosen.append(it)
        freed += v
        if freed >= gap:
            return chosen, freed, True
    return chosen, freed, False
