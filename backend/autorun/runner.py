from __future__ import annotations

import asyncio
import smtplib
import socket
import ssl
import time
import traceback
from email.mime.text import MIMEText
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


SMTP_TIMEOUT_SEC = 12
NEED_PIONNER_BADGE_COUNT = 4


class AutoRunner:
    PROBE_DEBUG = False
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
        self.op_interval_ms: int = 1000
        self.email_notify: dict = {}

        self.update_config(self._get_config())

    def _targets_status_lines(self, effect_list: List[Dict[str, Any]], targets: List[Dict[str, Any]]) -> List[str]:
        """逐个目标给出‘已拥有/未拥有’，并在目标为 amulet 时标注 plus / badge 需求。"""
        lines: List[str] = []
        # 先构建已拥有集合，便于快速判断
        owned: List[Tuple[int, bool, Optional[int]]] = []
        for e in effect_list or []:
            owned.append(self._extract_amulet_signature(e))

        def has_amulet(reg_id: int, need_plus: bool, need_badge: Optional[int]) -> bool:
            for (r, p, b) in owned:
                if r != reg_id:
                    continue
                if need_badge is not None and b != need_badge:
                    continue
                # 需要 plus 时必须 plus；不需要 plus 时要求非 plus
                if need_plus and not p:
                    continue
                if (not need_plus) and p:
                    continue
                return True
            return False

        for i, t in enumerate(targets or []):
            kind = t.get("kind")
            if kind == "badge":
                try:
                    bid = int(t.get("id"))
                except Exception:
                    bid = -1
                # 任意带该 badge 的护身符算满足
                ok = any((b == bid) for (_r, _p, b) in owned)
                lines.append(f"- 目标#{i + 1} 印章: {bid} —— {'已拥有✓' if ok else '未拥有×'}")
            elif kind == "amulet":
                try:
                    reg = int(t.get("id"))
                except Exception:
                    reg = -1
                need_plus = bool(t.get("plus", False))
                tb = t.get("badge", None)
                need_badge: Optional[int]
                if tb not in (None, ""):
                    try:
                        need_badge = int(tb)
                    except Exception:
                        need_badge = None
                else:
                    need_badge = None
                ok = has_amulet(reg, need_plus, need_badge)
                plus_txt = "plus=是" if need_plus else "plus=否"
                badge_txt = f", 需印章={need_badge}" if need_badge is not None else ""
                lines.append(f"- 目标#{i + 1} 护身符: reg={reg}（{plus_txt}{badge_txt}） —— {'已拥有✓' if ok else '未拥有×'}")
            else:
                lines.append(f"- 目标#{i + 1} 未知类型 —— 跳过")
        return lines

    @staticmethod
    def _fmt_ms(ms: int) -> str:
        ms = max(0, int(ms or 0))
        s = ms // 1000
        hh = s // 3600
        mm = (s % 3600) // 60
        ss = s % 60
        return f"{hh:02d}:{mm:02d}:{ss:02d}"

    @staticmethod
    def _amulet_sig_str(effect_item: Dict[str, Any]) -> str:
        reg, is_plus, badge = AutoRunner._extract_amulet_signature(effect_item)
        plus = "+" if is_plus else ""
        btxt = f", badge={badge}" if badge is not None else ""
        return f"reg={reg}{plus}{btxt}"

    def _owned_amulets_lines(self, effect_list: List[Dict[str, Any]]) -> List[str]:
        lines: List[str] = []
        for e in effect_list or []:
            lines.append(f"  • {self._amulet_sig_str(e)}")
        return lines or ["  （无）"]

    def _get_effect_list_snapshot(self) -> List[Dict[str, Any]]:
        gs = self._get_game_state()
        try:
            d = gs.to_dict() if hasattr(gs, "to_dict") else (gs or {})
        except Exception:
            d = (gs or {})
        return d.get("effect_list") or []

    def _notify_email_success_sync(self) -> None:
        cfg = self.email_notify or {}
        if not cfg.get("enabled"):
            return
        elapsed = self._calc_elapsed_ms()
        effect_list = self._get_effect_list_snapshot()
        lines_targets = self._targets_status_lines(effect_list, self.targets)
        lines_owned = self._owned_amulets_lines(effect_list)

        subject = "【Shanten Lens】自动化完成 ✓（目标已达成）"
        body = "\n".join([
            "自动化已完成（达到结束条件）。",
            f"- 运行时长：{self._fmt_ms(elapsed)}",
            f"- 已运行局数：{self.runs}",
            f"- 达成目标数：{self.best_achieved_count}/{self.end_count}",
            "",
            "目标达成情况：",
            *lines_targets,
            "",
            "当前已拥有护身符：",
            *lines_owned,
        ])
        ok, reason = self.send_email_notify(subject, body)
        if app_mod and hasattr(app_mod, "broadcast_sync_ui_toast"):
            # 需要的话，可以在后端也给前端弹个 toast
            try:
                app_mod.broadcast_sync_ui_toast("success", "目标已达成，邮件已发送" if ok else f"目标已达成，邮件发送失败：{reason}")
            except Exception:
                pass

    def _notify_email_failure_sync(self, reason_text: str) -> None:
        cfg = self.email_notify or {}
        if not cfg.get("enabled"):
            return
        elapsed = self._calc_elapsed_ms()
        subject = "【Shanten Lens】自动化中止 ✗"
        body = "\n".join([
            "自动化因错误中止。",
            f"- 错误原因：{reason_text or (self.last_error or 'unknown')}",
            f"- 最后步骤：{self.current_step or '-'}",
            f"- 运行时长：{self._fmt_ms(elapsed)}",
            f"- 已运行局数：{self.runs}",
        ])
        ok, reason = self.send_email_notify(subject, body)
        if app_mod and hasattr(app_mod, "broadcast_sync_ui_toast"):
            try:
                app_mod.broadcast_sync_ui_toast("error", "运行中止，邮件已发送" if ok else f"运行中止，邮件发送失败：{reason}")
            except Exception:
                pass

    def _preferred_flow_status(self) -> tuple[Optional[bool], Optional[str]]:
        packet_bot: PacketBot = self._get_packet_bot()
        if not packet_bot or not hasattr(packet_bot, "get_addon"):
            return None, None

        addon = packet_bot.get_addon()
        if not addon:
            return None, None

        flow = getattr(addon, "preferred_flow", None)
        peer_key = getattr(addon, "preferred_peer_key", None)

        if flow is None:
            return False, peer_key  # 可能為 None

        if not peer_key:
            try:
                cip = flow.client_conn.address[0]
                sip = flow.server_conn.address[0]
                peer_key = f"{cip}|{sip}"
            except Exception:
                peer_key = None

        try:
            ws = getattr(flow, "websocket", None)
            if ws is None:
                return False, peer_key
        except Exception:
            return False, peer_key

        return True, peer_key

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
        self.op_interval_ms = max(1, int((cfg or {}).get("op_interval_ms", 1000)))
        self.email_notify = (cfg or {}).get("email_notify")
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

    def send_email_notify(self, subject: str, body: str, *, to_override: Optional[str] = None) -> Tuple[bool, Optional[str]]:
        cfg = self.email_notify or {}
        if not cfg.get("enabled"):
            return False, "email-notify-disabled"

        host = (cfg.get("host") or "").strip()
        port = int(cfg.get("port") or 0)
        use_ssl = bool(cfg.get("ssl")) or (port == 465)  # 兼容常见 465=SSL
        from_addr = (cfg.get("from") or "").strip()
        pwd = cfg.get("pass") or ""
        to_addr = (to_override or cfg.get("to") or "").strip()

        # 基本校验
        if not host or not port:
            return False, "smtp-host-or-port-missing"
        if "@" not in (from_addr or ""):
            return False, "from-address-invalid"
        if "@" not in (to_addr or ""):
            return False, "to-address-invalid"
        if not pwd:
            return False, "smtp-password-missing"

        # 构造邮件
        msg = MIMEText(body or "", "plain", "utf-8")
        msg["Subject"] = subject or ""
        msg["From"] = from_addr
        msg["To"] = to_addr

        # 发送
        try:
            socket.setdefaulttimeout(SMTP_TIMEOUT_SEC)
            if use_ssl:
                ctx = ssl.create_default_context()
                with smtplib.SMTP_SSL(host, port, timeout=SMTP_TIMEOUT_SEC, context=ctx) as s:
                    s.login(from_addr, pwd)
                    s.sendmail(from_addr, [to_addr], msg.as_string())
            else:
                ctx = ssl.create_default_context()
                with smtplib.SMTP(host, port, timeout=SMTP_TIMEOUT_SEC) as s:
                    # 587 常见：先 EHLO 再 STARTTLS
                    try:
                        s.ehlo()
                        s.starttls(context=ctx)
                        s.ehlo()
                    except smtplib.SMTPException:
                        # 某些服务器不要求/不支持 STARTTLS，允许跳过
                        pass
                    s.login(from_addr, pwd)
                    s.sendmail(from_addr, [to_addr], msg.as_string())
            return True, None
        except smtplib.SMTPAuthenticationError as e:
            return False, f"smtp-auth-failed:{e.smtp_code or ''}"
        except smtplib.SMTPConnectError as e:
            return False, f"smtp-connect-failed:{e.smtp_code or ''}"
        except smtplib.SMTPServerDisconnected:
            return False, "smtp-disconnected"
        except smtplib.SMTPRecipientsRefused:
            return False, "smtp-recipient-refused"
        except smtplib.SMTPException as e:
            return False, f"smtp-error:{e.__class__.__name__}"
        except Exception as e:
            return False, f"error:{e.__class__.__name__}"

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
            self.elapsed_ms = 0
            self.started_at = _now_wall_ms()
            self._started_mono_ms = _now_mono_ms()
            self.current_step = "init"
            self.last_error = None
            self.runs = 0
            self.best_achieved_count = 0

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

    async def stop(self, *, final_step: Optional[str] = None) -> None:
        async with self._lock:
            if not self.running:
                # 即使未运行，也允许更新最终标签（例如为了在 UI 上保留最后状态）
                if final_step:
                    self.current_step = final_step
                    await self._broadcast_status(safe=True)
                return

            self.elapsed_ms = self._calc_elapsed_ms()
            self.running = False
            self._started_mono_ms = 0

            if self._loop_task and not self._loop_task.done():
                self._loop_task.cancel()
            self._loop_task = None

            if self._heartbeat_task and not self._heartbeat_task.done():
                self._heartbeat_task.cancel()
            self._heartbeat_task = None

            # 如果传入了最终标签，就保留它；否则使用默认的 "stopped"
            self.current_step = final_step or "stopped"

            await self._broadcast_status(safe=True)
            if self.PROBE_DEBUG:
                logger.info(f"[autorun] stopped (final_step={self.current_step})")

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
                await asyncio.sleep(self.op_interval_ms / 1000)
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
                    interval=3,
                    timeout=3000,
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
                    interval=3,
                    timeout=3000,
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
                # 这里是游戏开始前 —— 先尝试按 [卡维, 盗印like, 其他] 排序
                try:
                    uids = self._sorted_uids_by_mode(game_state.effect_list or [], mode="pre_start")
                    if uids:
                        self.current_step = "game.pre_start_sort"
                        await self._broadcast_status(safe=True)
                        ok_sort, reason_sort, _ = await call_with_1004_retry_async(
                            bot.sort_effect,
                            sorted_uid=uids,
                            delay_sec=2.0,
                            interval=0.3,
                            timeout=10,
                            to_thread=True,
                        )
                        if not ok_sort:
                            logger.warning(f"pre-start sort_effect failed: {reason_sort}")
                except Exception as _e:
                    logger.warning(f"pre-start sort attempt error: {_e}")
                ok, reason, resp = await call_with_1004_retry_async(
                    bot.next_level,
                    delay_sec=3,
                    interval=3,
                    timeout=3000,
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
                        interval=3,
                        timeout=3000,
                        to_thread=True,
                    )
                    if ok:
                        return
                    self.last_error = reason
                    await self.abort(f"fatal: {reason}")
                    return
                prefer_keep = [tid for tid in game_state.hand_tiles if (face := game_state.deck_map.get(tid)) is not None and (face == "bd" or face.endswith("p"))]
                # 901是每次只能替换三张牌的debuff
                if 901 in (game_state.boss_buff or []):
                    keep_target = max(0, len(game_state.hand_tiles) - 3)
                    if len(prefer_keep) >= keep_target:
                        filtered_ids = prefer_keep[:keep_target]
                    else:
                        rest = [tid for tid in game_state.hand_tiles if tid not in prefer_keep]
                        take_more = rest[: max(0, keep_target - len(prefer_keep))]
                        filtered_ids = prefer_keep + take_more
                else:
                    filtered_ids = prefer_keep
                ok, reason, resp = await call_with_1004_retry_async(
                    bot.op_change,
                    tile_ids=filtered_ids,
                    delay_sec=3,
                    interval=3,
                    timeout=3000,
                    to_thread=True,
                )
                if ok:
                    return
                self.last_error = reason
                await self.abort(f"fatal: {reason}")
                return
            if game_state.stage == 3:
                self.current_step = f"game.discard({game_state.level})"
                await self._broadcast_status(safe=True)
                suuannkou = plan_pure_pinzu_suu_ankou_v2(game_state.hand_tiles, game_state.wall_tiles, game_state.deck_map)
                if suuannkou["status"] == "impossible":
                    self.current_step = "game.remake"
                    await self._broadcast_status(safe=True)
                    self.need_start_game = True
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.giveup,
                        delay_sec=3,
                        interval=3,
                        timeout=3000,
                        to_thread=True,
                    )
                    if ok:
                        return
                    self.last_error = reason
                    await self.abort(f"fatal: {reason}")
                    return
                elif suuannkou["status"] == "win_now":
                    # 这里是和牌前 —— 先尝试按 [盗印like, 卡维, 其他] 排序
                    try:
                        uids = self._sorted_uids_by_mode(game_state.effect_list or [], mode="pre_win")
                        if uids:
                            self.current_step = "game.pre_win_sort"
                            await self._broadcast_status(safe=True)
                            ok_sort, reason_sort, resp = await call_with_1004_retry_async(
                                bot.sort_effect,
                                sorted_uid=uids,
                                delay_sec=2.0,
                                interval=0.3,
                                timeout=10,
                                to_thread=True,
                            )
                            if not ok_sort:
                                logger.warning(f"pre-win sort_effect failed: {reason_sort}, resp: {resp}")
                    except Exception as _e:
                        logger.warning(f"pre-win sort attempt error: {_e}")
                    self.current_step = "game.tsumo"
                    await self._broadcast_status(safe=True)
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.op_tsumo,
                        delay_sec=3,
                        interval=3,
                        timeout=3000,
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
                        interval=3,
                        timeout=3000,
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
                candidates = [g for g in game_state.goods if not g.get("sold", False)]
                if not candidates:
                    if game_state.refresh_price > game_state.coin:
                        # 如果当前已经到了截至关卡、则remake
                        if self.cutoff_level <= game_state.level:
                            self.current_step = "game.remake"
                            await self._broadcast_status(safe=True)
                            self.need_start_game = True
                            ok, reason, resp = await call_with_1004_retry_async(
                                bot.giveup,
                                delay_sec=3,
                                interval=3,
                                timeout=3000,
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
                            interval=3,
                            timeout=3000,
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
                        interval=3,
                        timeout=3000,
                        to_thread=True,
                    )
                    if ok:
                        # 刷新成功后：卖掉“带 600110 印章 且 非目标所需”的任意一个护身符
                        victim_uid: Optional[int] = None
                        for it in (game_state.effect_list or []):
                            bid = None
                            b = it.get("badge")
                            if isinstance(b, dict) and "id" in b:
                                try:
                                    bid = int(b["id"])
                                except Exception:
                                    bid = None
                            if bid == 600110 and not _is_needed_for_any_target(it, self.targets):
                                uid = it.get("uid")
                                if uid is not None:
                                    try:
                                        victim_uid = int(uid)
                                    except Exception:
                                        victim_uid = None
                                break  # 只挑一个

                        if victim_uid is not None:
                            self.current_step = "game.sell_happiness_after_refresh"
                            await self._broadcast_status(safe=True)
                            ok2, reason2, _ = await call_with_1004_retry_async(
                                bot.sell_effect,
                                uid=victim_uid,
                                delay_sec=3,
                                interval=3,
                                timeout=30,
                                to_thread=True,
                            )
                            if not ok2:
                                self.last_error = reason2
                                await self.abort(f"fatal: {reason2}")
                                return
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
                        if self.cutoff_level <= game_state.level:
                            self.current_step = "game.remake"
                            await self._broadcast_status(safe=True)
                            self.need_start_game = True
                            ok, reason, resp = await call_with_1004_retry_async(
                                bot.giveup,
                                delay_sec=3,
                                interval=3,
                                timeout=3000,
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
                            interval=3,
                            timeout=3000,
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
                        interval=3,
                        timeout=3000,
                        to_thread=True,
                    )
                    if ok:
                        # 刷新成功后：卖掉“带 600110 印章 且 非目标所需”的任意一个护身符
                        victim_uid: Optional[int] = None
                        for it in (game_state.effect_list or []):
                            bid = None
                            b = it.get("badge")
                            if isinstance(b, dict) and "id" in b:
                                try:
                                    bid = int(b["id"])
                                except Exception:
                                    bid = None
                            if bid == 600110 and not _is_needed_for_any_target(it, self.targets):
                                uid = it.get("uid")
                                if uid is not None:
                                    try:
                                        victim_uid = int(uid)
                                    except Exception:
                                        victim_uid = None
                                break  # 只挑一个

                        if victim_uid is not None:
                            self.current_step = "game.sell_happiness_after_refresh"
                            await self._broadcast_status(safe=True)
                            ok2, reason2, _ = await call_with_1004_retry_async(
                                bot.sell_effect,
                                uid=victim_uid,
                                delay_sec=3,
                                interval=3,
                                timeout=30,
                                to_thread=True,
                            )
                            if not ok2:
                                self.last_error = reason2
                                await self.abort(f"fatal: {reason2}")
                                return
                        return
                    self.last_error = reason
                    await self.abort(f"fatal: {reason}")
                    return
                ok, reason, resp = await call_with_1004_retry_async(
                    bot.buy_pack,
                    good_id=cheapest["id"],
                    delay_sec=3,
                    interval=3,
                    timeout=3000,
                    to_thread=True,
                )
                if ok:
                    return
                if reason == "error code: 2691":
                    bot.fetch_amulet_activity_data()
                    return
                self.last_error = reason
                await self.abort(f"fatal: {reason}")
                return
            if game_state.stage == 5 or game_state.stage == 7:
                if game_state.stage == 5:
                    self.current_step = "game.select_effect"
                else:
                    self.current_step = "game.select_reward_effect"
                await self._broadcast_status(safe=True)
                # value: 99-目标护身符、2-指引护身符（当前持有的指引护身符未满3个的情况下）、1-幸福护身符、0-普通
                best_raw, best_bid, value, sell_uid = select_amulet_from_candidates(game_state.candidate_effect_list, game_state.effect_list, self.targets)
                if sell_uid:
                    # 先卖掉
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.sell_effect, uid=sell_uid, delay_sec=3, interval=3, timeout=30, to_thread=True
                    )
                    if not ok:
                        self.last_error = reason
                        await self.abort(f"fatal: {reason}")
                        return
                need_space = 1
                if best_bid == 600160:
                    need_space = 2
                used_space = total_volume(game_state.effect_list)
                free_space = game_state.max_effect_volume - used_space
                # 如果空间充足、直接选择
                if free_space >= need_space:
                    if game_state.stage == 5:
                        ok, reason, resp = await call_with_1004_retry_async(
                            bot.select_effect,
                            selected_id=best_raw,
                            delay_sec=3,
                            interval=3,
                            timeout=3000,
                            to_thread=True,
                        )
                    else:
                        ok, reason, resp = await call_with_1004_retry_async(
                            bot.select_reward_effect,
                            selected_id=best_raw,
                            delay_sec=3,
                            interval=3,
                            timeout=3000,
                            to_thread=True,
                        )
                    if ok:
                        if value == 0:
                            reg_id = _reg_id_of_raw(best_raw)
                            if reg_id == 146:
                                return
                            uid = _pick_uid_to_sell_same_reg(game_state.effect_list, reg_id, self.targets)
                            if uid:
                                self.current_step = "game.sell_useless_effect"
                                await self._broadcast_status(safe=True)
                                ok, reason, resp = await call_with_1004_retry_async(
                                    bot.sell_effect,
                                    uid=uid,
                                    delay_sec=3,
                                    interval=3,
                                    timeout=3000,
                                    to_thread=True,
                                )
                                if ok:
                                    return
                                if reason == "error code: 2699":
                                    bot.fetch_amulet_activity_data()
                                    return
                                self.last_error = reason
                                await self.abort(f"fatal: {reason}")
                            return
                        return
                    if reason == "error code: 2691":
                        bot.fetch_amulet_activity_data()
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
                            self.current_step = "game.selling_to_make_space"
                            await self._broadcast_status(safe=True)
                            ok, reason, resp = await call_with_1004_retry_async(
                                bot.sell_effect,
                                uid=uid,
                                delay_sec=3,
                                interval=0.6,
                                timeout=3000,
                                to_thread=True,
                            )
                            if ok:
                                continue
                            self.last_error = reason
                            await self.abort(f"fatal: {reason}")
                            return
                    else:
                        self.current_step = "game.skip_buy_insufficient_space0"
                        await self._broadcast_status(safe=True)
                        if game_state.stage == 5:
                            ok, reason, resp = await call_with_1004_retry_async(
                                bot.select_effect,
                                selected_id=0,
                                delay_sec=3,
                                interval=3,
                                timeout=3000,
                                to_thread=True,
                            )
                        else:
                            ok, reason, resp = await call_with_1004_retry_async(
                                bot.select_reward_effect,
                                selected_id=0,
                                delay_sec=3,
                                interval=3,
                                timeout=3000,
                                to_thread=True,
                            )
                        if ok:
                            return
                        self.last_error = reason
                        await self.abort(f"fatal: {reason}")
                    return
                # 又不重要、空间还不够、直接不买、跳过
                logger.debug(f"not enough space to buy 1: max: {game_state.max_effect_volume}, used: {used_space}, free: {free_space}")
                self.current_step = "game.skip_buy_insufficient_space1"
                await self._broadcast_status(safe=True)
                if game_state.stage == 5:
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.select_effect,
                        selected_id=0,
                        delay_sec=3,
                        interval=3,
                        timeout=3000,
                        to_thread=True,
                    )
                else:
                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.select_reward_effect,
                        selected_id=0,
                        delay_sec=3,
                        interval=3,
                        timeout=3000,
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

            try:
                self._notify_email_failure_sync(self.last_error)
            except Exception:
                logger.exception("send failure email failed")

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
        pf_ready, pf_peer = self._preferred_flow_status()
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

            "preferred_flow_ready": pf_ready,
            "preferred_flow_peer": pf_peer,
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
        total_value = 0
        for idx in hit:
            try:
                t = self.targets[idx]
            except Exception:
                continue
            total_value += _target_value(t)
        return total_value

    async def _check_and_finish_if_done(self) -> bool:
        try:
            achieved = self.count_achieved_now()
        except Exception as e:
            logger.error("count_achieved_now failed: {}", e)
            return False

        if achieved > (self.best_achieved_count or 0):
            self.best_achieved_count = achieved

        if achieved >= (self.end_count or 1):
            # 先设置并广播达成态
            self.current_step = "goal_met"
            await self._broadcast_status(safe=True)

            try:
                self._notify_email_success_sync()
            except Exception:
                logger.exception("send success email failed")

            await self.stop(final_step="goal_met")
            return True
        return False

    @staticmethod
    def _base(raw: Any) -> int:
        try:
            return int(raw or 0) // 10
        except Exception:
            return 0

    @staticmethod
    def _first_src_base(row: Dict[str, Any]) -> Optional[int]:
        if not isinstance(row, dict):
            return None
        store = row.get("store")
        if not isinstance(store, list) or not store:
            return None
        try:
            return int(store[0]) // 10
        except Exception:
            return None

    @staticmethod
    def _is_theft_like(row: Dict[str, Any]) -> bool:
        # 盗印 229；黑客 232 / 不稳定 228 若来源是盗印，也算 theft-like
        ID_UNSTABLE, ID_THEFT, ID_HACKER = 228, 229, 232
        b = AutoRunner._base(row.get("id"))
        if b == ID_THEFT:
            return True
        if b in (ID_HACKER, ID_UNSTABLE) and AutoRunner._first_src_base(row) == ID_THEFT:
            return True
        return False

    @staticmethod
    def _is_kavi(row: Dict[str, Any]) -> bool:
        return AutoRunner._base(row.get("id")) == 230

    def _sorted_uids_by_mode(self, effect_list: List[Dict[str, Any]], mode: str) -> Optional[List[int]]:
        if not isinstance(effect_list, list):
            return None

        kavi, theftlike, others = [], [], []
        for row in effect_list:
            if self._is_kavi(row):
                kavi.append(row)
            elif self._is_theft_like(row):
                theftlike.append(row)
            else:
                others.append(row)

        if mode == "pre_start":
            new_order = kavi + theftlike + others
        elif mode == "pre_win":
            new_order = theftlike + kavi + others
        else:
            return None

        def _uids(arr: List[Dict[str, Any]]) -> List[int]:
            out = []
            for r in arr:
                try:
                    out.append(int(r.get("uid")))
                except Exception:
                    pass
            return out

        new_uids = _uids(new_order)
        old_uids = _uids(effect_list)

        if len(new_uids) != len(old_uids) or set(new_uids) != set(old_uids):
            return None
        if new_uids == old_uids:
            return None
        return new_uids


def _target_value(t: Dict[str, Any]) -> int:
    try:
        v = int(t.get("value", 1))
        return max(0, v)
    except Exception:
        return 1


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


def _required_nonplus_badges_for_reg(targets: List[Dict[str, Any]], reg_id: int) -> set[int]:
    req: set[int] = set()
    for t in targets or []:
        if t.get("kind") != "amulet":
            continue
        try:
            tid = int(t.get("id"))
        except Exception:
            continue
        if tid != reg_id:
            continue
        if not bool(t.get("plus", False)):
            tb = t.get("badge", None)
            if tb not in (None, "",):
                try:
                    req.add(int(tb))
                except Exception:
                    pass
    return req


def _find_owned_uid_for_reg(effect_list: List[Dict[str, Any]], reg_id: int) -> Optional[int]:
    for e in effect_list or []:
        try:
            raw = int(e.get("id", 0))
            if raw // 10 == reg_id:
                uid = e.get("uid")
                return int(uid) if uid is not None else None
        except Exception:
            continue
    return None


def _owned_effect_value_for_selling(e: Dict[str, Any], targets: List[Dict[str, Any]]) -> int:
    if _is_needed_for_any_target(e, targets):
        return 10 ** 9  # 目标需要，绝不卖

    reg_id, is_plus, bid = _extract_amulet_signature(e)

    # 基础价值
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

    if bid == 600050:
        base *= 3

    # 提升重要护身符与关键印章
    if bid == 600070:  # 指引
        base += 10000
    if bid == 600110:  # 幸福
        base += 1000
    if reg_id == 146:  # 车轮
        base += 10000

    return base


def _pick_uid_to_sell_same_reg(effect_list: List[Dict[str, Any]], reg_id: int, targets: List[Dict[str, Any]]) -> Optional[int]:
    cands: List[Dict[str, Any]] = []
    for e in effect_list or []:
        try:
            rid = int(e.get("id", 0)) // 10
            if rid == reg_id:
                cands.append(e)
        except Exception:
            continue

    if not cands:
        return None

    worst = min(
        cands,
        key=lambda x: (_owned_effect_value_for_selling(x, targets), int(x.get("uid") or 1_000_000_000))
    )
    uid = worst.get("uid")
    try:
        return int(uid) if uid is not None else None
    except Exception:
        return None


def select_amulet_from_candidates(
        candidate_effect_list: List[Dict[str, Any]],
        effect_list: List[Dict[str, Any]],
        targets: List[Dict[str, Any]],
) -> Tuple[Optional[int], Optional[int], Optional[int], Optional[int]]:
    if not candidate_effect_list:
        return None, None, None, None

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

    zero_raw_ids: set[int] = set()

    for c in candidate_effect_list:
        raw_id = int(c.get("id", 0))
        if raw_id <= 0:
            continue
        reg_id = _reg_id_of_raw(raw_id)
        bid = _candidate_badge_id(c)

        # 命中“目标印章”直接 99
        if bid is not None and bid in want_badges:
            return raw_id, bid, 99, None

        # 命中“目标护身符 reg”
        if reg_id in want_amulet_regs:
            required_badges = _required_nonplus_badges_for_reg(targets, reg_id)
            if required_badges:
                # 非 plus 且目标指定 badge，候选必须匹配该 badge，否则这张候选记为 0 分
                if bid in required_badges:
                    return raw_id, bid, 99, None
                else:
                    zero_raw_ids.add(raw_id)
            else:
                # 无额外 badge 要求，直接 99
                return raw_id, bid, 99, None

    # 指引 600070 未满 3 个
    WANT_BADGE_STACK = 600070
    if _owned_count_with_badge(effect_list, WANT_BADGE_STACK) < NEED_PIONNER_BADGE_COUNT:
        for c in candidate_effect_list:
            bid = _candidate_badge_id(c)
            if bid == WANT_BADGE_STACK:
                return int(c["id"]), bid, 2, None

    # 幸福 600110
    for c in candidate_effect_list:
        bid = _candidate_badge_id(c)
        if bid == 600110:
            return int(c["id"]), bid, 1, None

    # 按价值挑最好；同时，当某候选被标记为 zero_raw_ids 时，尝试给出 sell_uid
    best_raw: Optional[int] = None
    best_bid: Optional[int] = None
    best_val = -10 ** 9
    best_sell_uid: Optional[int] = None

    for c in candidate_effect_list:
        try:
            raw_id = int(c.get("id", 0))
        except Exception:
            continue
        if raw_id <= 0:
            continue

        bid = _candidate_badge_id(c)
        reg_id = _reg_id_of_raw(raw_id)

        if raw_id in zero_raw_ids:
            # 价值强制为 0；若背包已有同 reg 且该已拥有并非目标需要，建议先卖
            val = 0
            uid = _find_owned_uid_for_reg(effect_list, reg_id)
            if uid is not None:
                # 确认这件现有的不被目标需要
                owned_item = next((e for e in effect_list if e.get("uid") == uid), None)
                if owned_item is not None and not _is_needed_for_any_target(owned_item, targets):
                    sell_uid = uid
                else:
                    sell_uid = None
            else:
                sell_uid = None
        else:
            val = _candidate_value(raw_id, bid)
            sell_uid = None

        if val > best_val:
            best_val = val
            best_raw = raw_id
            best_bid = bid
            best_sell_uid = sell_uid

    return best_raw, best_bid, 0 if best_raw is not None and best_raw in zero_raw_ids else (0 if best_val <= 0 else 0), best_sell_uid


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
        if badge_id == KEEP_BADGE and demoted_taken < NEED_PIONNER_BADGE_COUNT:
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
