from __future__ import annotations

import asyncio
import smtplib
import socket
import ssl
import time
import traceback
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from html import escape
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from backend.autorun.stages import handle_autorun_tick
from backend.autorun.strategy import DefaultAutoRunStrategy, default_select_amulet_from_candidates, select_items_to_sell_for_purchase as strategy_select_items_to_sell_for_purchase
from backend.autorun.value import (
    calc_candidate_price,
    calc_target_achievement_value,
    candidate_badge_id,
    extract_amulet_signature,
    is_needed_for_any_target,
    reg_id_of_raw,
    target_value,
    total_volume as calc_total_volume,
)
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
EMAIL_ASSETS_BASE_URL = "https://raw.githubusercontent.com/SpCoGov/shanten-lens/refs/heads/2.0/app/public/assets"
EMAIL_AMULET_ASSETS_BASE_URL = f"{EMAIL_ASSETS_BASE_URL}/amulet"
EMAIL_BADGE_ASSETS_BASE_URL = f"{EMAIL_ASSETS_BASE_URL}/badge"


def _format_smtp_response(error: smtplib.SMTPResponseException) -> str:
    response = error.smtp_error
    if isinstance(response, bytes):
        response = response.decode("utf-8", errors="replace")
    response_text = str(response or "").strip()
    code_text = str(error.smtp_code or "").strip()
    if code_text and response_text:
        return f"{code_text} {response_text}"
    return code_text or response_text


def _email_error_payload(error: Exception, *, host: str, port: int, use_ssl: bool) -> Dict[str, Any]:
    target = f"{host}:{port}"
    values: Dict[str, Any] = {
        "host": host,
        "port": port,
        "target": target,
        "ssl": "on" if use_ssl else "off",
    }
    detail = str(error).strip()
    if detail:
        values["detail"] = detail
    values["error"] = error.__class__.__name__

    if isinstance(error, smtplib.SMTPAuthenticationError):
        response = _format_smtp_response(error)
        if response:
            values["detail"] = response
        return {"key": "autorun.email_error.auth_failed", "values": values}
    if isinstance(error, smtplib.SMTPConnectError):
        response = _format_smtp_response(error)
        if response:
            values["detail"] = response
        return {"key": "autorun.email_error.connect_failed", "values": values}
    if isinstance(error, smtplib.SMTPRecipientsRefused):
        return {"key": "autorun.email_error.recipient_refused", "values": values}
    if isinstance(error, smtplib.SMTPServerDisconnected):
        return {"key": "autorun.email_error.disconnected", "values": values}
    if isinstance(error, ssl.SSLError):
        return {"key": "autorun.email_error.ssl_failed", "values": values}
    if isinstance(error, socket.gaierror):
        return {"key": "autorun.email_error.dns_failed", "values": values}
    if isinstance(error, (socket.timeout, TimeoutError)):
        return {"key": "autorun.email_error.timeout", "values": values}
    if isinstance(error, OSError):
        return {"key": "autorun.email_error.os_error", "values": values}
    if isinstance(error, smtplib.SMTPException):
        return {"key": "autorun.email_error.smtp_error", "values": values}
    return {"key": "autorun.email_error.unknown", "values": values}
EMAIL_ASSETS_ROOT = Path(__file__).resolve().parents[2] / "app" / "public" / "assets"
EMAIL_AMULET_ASSETS_ROOT = EMAIL_ASSETS_ROOT / "amulet"
EMAIL_BADGE_ASSETS_ROOT = EMAIL_ASSETS_ROOT / "badge"
EMAIL_RARITY_BG_INDEX = {
    "PURPLE": 1,
    "ORANGE": 2,
    "BLUE": 3,
    "GREEN": 4,
    "GRAY": 5,
}


class AutoRunner:
    PROBE_DEBUG = False
    HEARTBEAT_INTERVAL = 1.0  # s
    MAX_CONSECUTIVE_ABORT_TICKS = 3

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
        self.remake_records: List[Dict[str, Any]] = []
        self.best_remake_record: Optional[Dict[str, Any]] = None
        self.operation_records: List[Dict[str, Any]] = []
        self.operation_count_by_run: Dict[int, int] = {}
        self._operation_seq: int = 0
        self.current_step: str = "-"
        self.last_error: Optional[str] = None
        self.need_start_game = False
        self._consecutive_abort_reasons: List[str] = []
        self._deferred_abort_this_tick = False

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
        self.need_pionner_badge_count: int = NEED_PIONNER_BADGE_COUNT
        self.record_detailed_operations: bool = False
        self.email_notify: dict = {}
        self.strategy = DefaultAutoRunStrategy()

        self.update_config(self._get_config())


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

    def _get_effect_list_snapshot(self) -> List[Dict[str, Any]]:
        gs = self._get_game_state()
        try:
            d = gs.to_dict() if hasattr(gs, "to_dict") else (gs or {})
        except Exception:
            d = (gs or {})
        return d.get("effect_list") or []

    def _preferred_flow_status(self) -> tuple[Optional[bool], Optional[str]]:
        packet_bot: PacketBot = self._get_packet_bot()
        if not packet_bot or not hasattr(packet_bot, "get_addon"):
            return None, None

        addon = packet_bot.get_addon()
        if not addon:
            return None, None

        flow = getattr(addon, "preferred_flow", None)
        peer_key = getattr(addon, "preferred_peer_key", None)

        if not peer_key:
            return False, None

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
        try:
            self.need_pionner_badge_count = max(
                0,
                int((cfg or {}).get("need_pionner_badge_count", NEED_PIONNER_BADGE_COUNT)),
            )
        except (TypeError, ValueError):
            self.need_pionner_badge_count = NEED_PIONNER_BADGE_COUNT
        record_detailed_operations = bool((cfg or {}).get("record_detailed_operations", False))
        if not record_detailed_operations and self.record_detailed_operations:
            self.operation_records = []
            self.operation_count_by_run = {}
        self.record_detailed_operations = record_detailed_operations
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

    def _pad4(self, n: int) -> str:
        return str(int(n)).zfill(4)

    def _inline_img_tag(self, src: Optional[str], alt: str, style: str) -> str:
        if not src:
            return ""
        return f'<img src="{escape(src, quote=True)}" alt="{escape(alt)}" style="{style}">'

    def _get_amulet_item(self, reg_id: int):
        if not app_mod or not getattr(app_mod, "AMULET_REG", None):
            return None
        try:
            return app_mod.AMULET_REG.get(int(reg_id))
        except Exception:
            return None

    def _get_badge_item(self, badge_id: Optional[int]):
        if badge_id is None or not app_mod or not getattr(app_mod, "BADGE_REG", None):
            return None
        try:
            return app_mod.BADGE_REG.get(int(badge_id))
        except Exception:
            return None

    def _get_amulet_registry(self):
        try:
            return getattr(app_mod, "AMULET_REG", None)
        except Exception:
            return None

    def _notify_email_success_sync(self) -> None:
        cfg = self.email_notify or {}
        if not cfg.get("enabled"):
            return
        elapsed = self._calc_elapsed_ms()
        effect_list = self._get_effect_list_snapshot()
        plain_body, html_body = self._build_success_email_bodies(effect_list, elapsed)
        subject = "[Shanten Lens] 自动化完成（目标已达成）"
        ok, reason = self.send_email_notify(subject, plain_body, html_body=html_body)
        if app_mod and hasattr(app_mod, "broadcast_sync_ui_toast"):
            try:
                if ok:
                    app_mod.broadcast_sync_ui_toast("success", msg_key="autorun.email_toast.success_done")
                else:
                    payload = reason or {}
                    app_mod.broadcast_sync_ui_toast(
                        "error",
                        msg_key="autorun.email_toast.success_notify_failed",
                        msg_values={
                            "reason_key": payload.get("key", "autorun.email_error.unknown"),
                            "reason_values": payload.get("values", {}),
                        },
                    )
            except Exception:
                pass


    def _asset_url(self, kind: str, filename: str) -> str:
        base = EMAIL_AMULET_ASSETS_BASE_URL if kind == "amulet" else EMAIL_BADGE_ASSETS_BASE_URL
        return f"{base}/{filename}"

    def _build_effect_display_info(self, effect_item: Dict[str, Any]) -> Dict[str, Any]:
        reg_id, is_plus, badge_id = self._extract_amulet_signature(effect_item)
        amulet = self._get_amulet_item(reg_id)
        badge = self._get_badge_item(badge_id)
        rarity_name = getattr(getattr(amulet, "rarity", None), "name", "GREEN")
        bg_index = EMAIL_RARITY_BG_INDEX.get(str(rarity_name).upper(), 4)
        try:
            volume = int(effect_item.get("volume") or 1)
        except Exception:
            volume = 1
        bg_file = f"fu_bg{bg_index}_widen.png" if volume == 2 else f"fu_bg{bg_index}.png"
        icon_id = getattr(amulet, "icon_id", 0) if amulet else 0
        icon_file = f"fu_{self._pad4(icon_id)}.png" if amulet else None
        return {
            "reg_id": reg_id,
            "is_plus": is_plus,
            "badge_id": badge_id,
            "amulet": amulet,
            "badge": badge,
            "bg_url": self._asset_url("amulet", bg_file),
            "icon_url": self._asset_url("amulet", icon_file) if icon_file else None,
            "badge_url": self._asset_url("badge", f"badge_{badge_id}.png") if badge_id is not None else None,
            "plus_url": self._asset_url("amulet", "plus.png") if is_plus else None,
        }

    def _amulet_summary_text(self, effect_item: Dict[str, Any]) -> str:
        info = self._build_effect_display_info(effect_item)
        name = getattr(info["amulet"], "name", None) or f"未知护身符 #{info['reg_id']}"
        parts = [name]
        if info["is_plus"]:
            parts.append("Plus")
        if info["badge"] is not None:
            parts.append(f"印章:{getattr(info['badge'], 'name', info['badge_id'])}")
        elif info["badge_id"] is not None:
            parts.append(f"印章:{info['badge_id']}")
        return " / ".join(parts)

    def _target_summary_text(self, target: Dict[str, Any]) -> str:
        if target.get("kind") == "badge":
            try:
                badge_id = int(target.get("id"))
            except Exception:
                badge_id = -1
            badge = self._get_badge_item(badge_id)
            badge_name = getattr(badge, "name", None) or f"未知印章 #{badge_id}"
            return f"印章: {badge_name}"
        if target.get("kind") == "amulet":
            target_effect = {
                "id": int(target.get("id") or 0) * 10 + (1 if target.get("plus") else 0),
                "badge": {"id": int(target["badge"])} if target.get("badge") not in (None, "") else None,
                "volume": int(target.get("volume") or 1),
            }
            return self._amulet_summary_text(target_effect)
        return "未知目标"

    def _target_is_owned(self, effect_list: List[Dict[str, Any]], target: Dict[str, Any]) -> bool:
        if target.get("kind") == "badge":
            try:
                want_badge = int(target.get("id"))
            except Exception:
                return False
            for effect_item in effect_list or []:
                _reg_id, _is_plus, badge_id = self._extract_amulet_signature(effect_item)
                if badge_id == want_badge:
                    return True
            return False
        if target.get("kind") == "amulet":
            for effect_item in effect_list or []:
                if self.amulet_matches_target(effect_item, target):
                    return True
        return False

    def _targets_status_lines(self, effect_list: List[Dict[str, Any]], targets: List[Dict[str, Any]]) -> List[str]:
        lines: List[str] = []
        for i, target in enumerate(targets or []):
            ok = self._target_is_owned(effect_list, target)
            lines.append(f"- 目标#{i + 1} {self._target_summary_text(target)} - {'已拥有✓' if ok else '未拥有×'}")
        return lines

    def _owned_amulets_lines(self, effect_list: List[Dict[str, Any]]) -> List[str]:
        lines: List[str] = []
        for effect_item in effect_list or []:
            lines.append(f"  • {self._amulet_summary_text(effect_item)}")
        return lines or ["  （无）"]

    def _render_stat_pills(self, items: List[Tuple[str, str]]) -> str:
        return "".join([
            f'<div style="display:inline-block;min-width:120px;margin:0 12px 12px 0;padding:12px 14px;border:1px solid #d0d5dd;border-radius:12px;background:#fff;">'
            f'<div style="color:#475467;font-size:12px;">{escape(label)}</div>'
            f'<div style="margin-top:6px;color:#101828;font-size:22px;font-weight:700;">{escape(value)}</div>'
            '</div>'
            for label, value in items
        ])

    def _render_card_grid(self, cards: List[str], empty_text: str) -> str:
        if not cards:
            return f'<div style="color:#475467;font-size:14px;">{escape(empty_text)}</div>'
        return "".join(cards)

    def _render_email_section(self, title: str, content_html: str, *, subtitle: Optional[str] = None) -> str:
        subtitle_html = f'<div style="margin-top:6px;color:#475467;font-size:14px;">{escape(subtitle)}</div>' if subtitle else ""
        return (
            '<div style="margin-top:28px;padding-top:24px;border-top:1px solid #eaecf0;">'
            f'<div style="font-size:20px;font-weight:800;color:#101828;">{escape(title)}</div>'
            f'{subtitle_html}'
            f'<div style="margin-top:16px;">{content_html}</div>'
            '</div>'
        )

    def _wrap_email_html(self, title: str, subtitle: str, body_html: str, *, accent: str) -> str:
        return (
            '<html><body style="margin:0;padding:24px;background:#f5f7fb;color:#101828;font-family:Segoe UI,Microsoft YaHei,Arial,sans-serif;">'
            '<div style="max-width:1080px;margin:0 auto;">'
            '<div style="padding:24px;border-radius:18px;background:#ffffff;border:1px solid #d0d5dd;">'
            f'<div style="display:inline-block;padding:6px 10px;border-radius:999px;background:{accent};color:#ffffff;font-size:12px;font-weight:700;">Shanten Lens</div>'
            f'<div style="margin-top:14px;font-size:28px;font-weight:800;color:#101828;">{escape(title)}</div>'
            f'<div style="margin-top:8px;color:#475467;font-size:14px;">{escape(subtitle)}</div>'
            f'{body_html}'
            '</div></div></body></html>'
        )

    def _unwrap_email_html(self, html_doc: str) -> str:
        marker = '<div style="max-width:1080px;margin:0 auto;">'
        start = html_doc.find(marker)
        if start < 0:
            return html_doc
        end = html_doc.rfind("</body></html>")
        if end < 0:
            return html_doc[start:]
        return html_doc[start:end]

    def _build_failure_email_bodies(self, reason_text: str, elapsed_ms: int) -> Tuple[str, str]:
        reason = reason_text or (self.last_error or "unknown")
        plain_body = "\n".join([
            "自动化因错误中止。",
            f"- 错误原因：{reason}",
            f"- 最后步骤：{self.current_step or '-'}",
            f"- 运行时长：{self._fmt_ms(elapsed_ms)}",
            f"- 已运行局数：{self.runs}",
        ])
        alert_html = (
            '<div style="margin-top:20px;padding:16px 18px;border:1px solid #fecdca;border-radius:14px;background:#fef3f2;">'
            '<div style="color:#b42318;font-size:15px;font-weight:800;">错误详情</div>'
            f'<div style="margin-top:8px;color:#7a271a;font-size:14px;line-height:1.6;">{escape(reason)}</div>'
            '</div>'
        )
        body_html = (
            f'<div style="margin-top:20px;">{self._render_stat_pills([("最后步骤", self.current_step or "-"), ("运行时长", self._fmt_ms(elapsed_ms)), ("已运行局数", str(self.runs))])}</div>'
            + alert_html
        )
        html_body = self._wrap_email_html("自动化中止", "运行过程中触发错误，下面是本次终止摘要。", body_html, accent="#d92d20")
        return plain_body, html_body

    def _notify_email_failure_sync(self, reason_text: str) -> None:
        cfg = self.email_notify or {}
        if not cfg.get("enabled"):
            return
        elapsed = self._calc_elapsed_ms()
        plain_body, html_body = self._build_failure_email_bodies(reason_text, elapsed)
        subject = "[Shanten Lens] 自动化中止"
        ok, reason = self.send_email_notify(subject, plain_body, html_body=html_body)
        if app_mod and hasattr(app_mod, "broadcast_sync_ui_toast"):
            try:
                if ok:
                    app_mod.broadcast_sync_ui_toast("error", msg_key="autorun.email_toast.failure_done")
                else:
                    payload = reason or {}
                    app_mod.broadcast_sync_ui_toast(
                        "error",
                        msg_key="autorun.email_toast.failure_notify_failed",
                        msg_values={
                            "reason_key": payload.get("key", "autorun.email_error.unknown"),
                            "reason_values": payload.get("values", {}),
                        },
                    )
            except Exception:
                pass

    def _build_amulet_card_html(
            self,
            effect_item: Dict[str, Any],
            *,
            footer: Optional[str] = None,
            status_label: Optional[str] = None,
            status_ok: Optional[bool] = None,
    ) -> str:
        info = self._build_effect_display_info(effect_item)
        name = getattr(info["amulet"], "name", None) or f"未知护身符 #{info['reg_id']}"
        card_width = 160
        card_height = 220
        status_html = ""
        if status_label:
            status_bg = "#16a34a" if status_ok else "#dc2626"
            status_html = (
                f'<div style="display:inline-block;margin-bottom:8px;padding:4px 8px;border-radius:999px;'
                f'background:{status_bg};color:#fff;font-size:12px;font-weight:700;line-height:1.2;">{escape(status_label)}</div>'
            )
        icon_html = self._inline_img_tag(
            info.get("icon_url"),
            name,
            f"display:block;width:{card_width}px;height:{card_height}px;border:1px solid #d0d5dd;border-radius:14px;background:#f8fafc;",
        )
        footer_html = f'<div style="margin-top:6px;color:#475467;font-size:12px;">{escape(footer)}</div>' if footer else ""
        return (
            '<div style="display:inline-block;vertical-align:top;width:180px;margin:0 12px 16px 0;">'
            f'{status_html}'
            f'{icon_html}'
            f'<div style="margin-top:8px;color:#101828;font-size:14px;font-weight:700;line-height:1.4;">{escape(name)}</div>'
            f'{footer_html}'
            '</div>'
        )

    def _build_badge_target_html(
            self,
            badge_id: int,
            *,
            status_label: Optional[str] = None,
            status_ok: Optional[bool] = None,
    ) -> str:
        badge = self._get_badge_item(badge_id)
        name = getattr(badge, "name", None) or f"未知印章 #{badge_id}"
        status_html = ""
        if status_label:
            status_bg = "#16a34a" if status_ok else "#dc2626"
            status_html = (
                f'<div style="display:inline-block;margin-bottom:8px;padding:4px 8px;border-radius:999px;'
                f'background:{status_bg};color:#fff;font-size:12px;font-weight:700;line-height:1.2;">{escape(status_label)}</div>'
            )
        icon_html = self._inline_img_tag(
            self._asset_url("badge", f"badge_{badge_id}.png") if badge_id > 0 else None,
            name,
            "display:block;width:96px;height:96px;object-fit:contain;border:1px solid #d0d5dd;border-radius:14px;background:#f8fafc;padding:12px;",
        )
        if not icon_html:
            icon_html = (
                '<div style="width:96px;height:96px;border:1px solid #d0d5dd;border-radius:14px;'
                'background:#f8fafc;padding:12px;color:#667085;font-size:12px;line-height:96px;text-align:center;">无图标</div>'
            )
        return (
            '<div style="display:inline-block;vertical-align:top;width:140px;margin:0 12px 16px 0;">'
            f'{status_html}'
            f'{icon_html}'
            f'<div style="margin-top:8px;color:#101828;font-size:14px;font-weight:700;line-height:1.4;">{escape(name)}</div>'
            f'<div style="margin-top:4px;color:#475467;font-size:12px;">ID {escape(str(badge_id))}</div>'
            '</div>'
        )

    def _build_success_email_bodies(self, effect_list: List[Dict[str, Any]], elapsed_ms: int) -> Tuple[str, str]:
        plain_body = "\n".join([
            "自动化已完成（达到结束条件）。",
            f"- 运行时长：{self._fmt_ms(elapsed_ms)}",
            f"- 已运行局数：{self.runs}",
            f"- 达成目标数：{self.best_achieved_count}/{self.end_count}",
            "",
            "目标达成情况：",
            *self._targets_status_lines(effect_list, self.targets),
            "",
            "当前已拥有护身符：",
            *self._owned_amulets_lines(effect_list),
        ])

        target_cards: List[str] = []
        for idx, target in enumerate(self.targets or []):
            owned = self._target_is_owned(effect_list, target)
            status_label = f"目标#{idx + 1} {'已拥有' if owned else '未拥有'}"
            if target.get("kind") == "badge":
                try:
                    badge_id = int(target.get("id"))
                except Exception:
                    badge_id = -1
                target_cards.append(self._build_badge_target_html(badge_id, status_label=status_label, status_ok=owned))
                continue
            if target.get("kind") == "amulet":
                target_effect = {
                    "id": int(target.get("id") or 0) * 10 + (1 if target.get("plus") else 0),
                    "badge": {"id": int(target["badge"])} if target.get("badge") not in (None, "") else None,
                    "volume": int(target.get("volume") or 1),
                }
                footer_parts: List[str] = []
                if target.get("plus"):
                    footer_parts.append("需要 Plus")
                if target.get("badge") not in (None, ""):
                    badge = self._get_badge_item(int(target["badge"]))
                    footer_parts.append(f"需要印章: {getattr(badge, 'name', target['badge'])}")
                target_cards.append(self._build_amulet_card_html(
                    target_effect,
                    footer=" / ".join(footer_parts) if footer_parts else None,
                    status_label=status_label,
                    status_ok=owned,
                ))

        owned_cards = [
            self._build_amulet_card_html(effect_item)
            for effect_item in (effect_list or [])
        ]
        body_html = (
            f'<div style="margin-top:20px;">{self._render_stat_pills([("运行时长", self._fmt_ms(elapsed_ms)), ("已运行局数", str(self.runs)), ("达成目标数", f"{self.best_achieved_count}/{self.end_count}")])}</div>'
            + self._render_email_section("目标达成情况", self._render_card_grid(target_cards, "未配置目标。"))
            + self._render_email_section("当前已拥有护身符", self._render_card_grid(owned_cards, "当前没有已持有护身符。"))
        )
        html_body = self._wrap_email_html("自动化已完成", "达到结束条件后生成的护身符汇总。", body_html, accent="#1570ef")
        return plain_body, html_body

    def build_test_email_bodies(self) -> Tuple[str, str]:
        effect_list = self._get_effect_list_snapshot()
        success_plain, success_html = self._build_success_email_bodies(effect_list, self._calc_elapsed_ms())
        failure_plain, failure_html = self._build_failure_email_bodies("这是测试邮件中的示例错误：网络超时 / 目标条件不满足。", self._calc_elapsed_ms())
        plain_body = "\n\n".join([
            "这是一封测试邮件，下面会预览成功通知和失败通知样式。",
            "=== 成功样式预览 ===",
            success_plain,
            "=== 错误样式预览 ===",
            failure_plain,
        ])
        html_body = self._wrap_email_html(
            "测试通知预览",
            "这封测试邮件同时展示成功通知和错误通知的最终样式。",
            self._render_email_section("成功通知样式", self._unwrap_email_html(success_html), subtitle="自动化完成时会发送类似内容。")
            + self._render_email_section("错误通知样式", self._unwrap_email_html(failure_html), subtitle="自动化出错中止时会发送类似内容。"),
            accent="#7a5af8",
        )
        return plain_body, html_body

    def send_email_notify(
            self,
            subject: str,
            body: str,
            *,
            to_override: Optional[str] = None,
            html_body: Optional[str] = None,
            inline_images: Optional[Dict[str, Dict[str, Any]]] = None,
    ) -> Tuple[bool, Optional[Dict[str, Any]]]:
        cfg = self.email_notify or {}
        if not cfg.get("enabled"):
            return False, {"key": "autorun.email_error.disabled", "values": {}}

        host = (cfg.get("host") or "").strip()
        port = int(cfg.get("port") or 0)
        use_ssl = bool(cfg.get("ssl")) or (port == 465)
        from_addr = (cfg.get("from") or "").strip()
        pwd = cfg.get("pass") or ""
        to_addr = (to_override or cfg.get("to") or "").strip()

        if not host or not port:
            return False, {"key": "autorun.email_error.host_or_port_missing", "values": {}}
        if "@" not in (from_addr or ""):
            return False, {"key": "autorun.email_error.from_invalid", "values": {"email": from_addr}}
        if "@" not in (to_addr or ""):
            return False, {"key": "autorun.email_error.to_invalid", "values": {"email": to_addr}}
        if not pwd:
            return False, {"key": "autorun.email_error.password_missing", "values": {}}

        if html_body is None and subject.startswith("Shanten Lens "):
            body, html_body = self.build_test_email_bodies()

        if html_body:
            msg = MIMEMultipart("alternative")
            msg.attach(MIMEText(body or "", "plain", "utf-8"))
            msg.attach(MIMEText(html_body, "html", "utf-8"))
        else:
            msg = MIMEText(body or "", "plain", "utf-8")

        msg["Subject"] = subject or ""
        msg["From"] = from_addr
        msg["To"] = to_addr

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
                    try:
                        s.ehlo()
                        s.starttls(context=ctx)
                        s.ehlo()
                    except smtplib.SMTPException:
                        pass
                    s.login(from_addr, pwd)
                    s.sendmail(from_addr, [to_addr], msg.as_string())
            return True, None
        except Exception as e:
            return False, _email_error_payload(e, host=host, port=port, use_ssl=use_ssl)

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
        return self.probe_has_live_game(self._last_probe_resp)

    @staticmethod
    def probe_has_live_game(resp: Optional[dict]) -> bool:
        resp_data = (resp or {}).get("data") or {}
        game = resp_data.get("game")
        if game is None:
            nested_data = resp_data.get("data") or {}
            game = nested_data.get("game")
        return isinstance(game, dict) and not bool(game.get("ended", False))

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
            self.remake_records = []
            self.best_remake_record = None
            self.operation_records = []
            self.operation_count_by_run = {}
            self._operation_seq = 0

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
                self._deferred_abort_this_tick = False
                try:
                    await self.run_tick()
                except asyncio.CancelledError:
                    raise
                except Exception as e:
                    self.last_error = str(e)
                    logger.exception("[autorun] run_tick error")
                if not self._deferred_abort_this_tick:
                    self._consecutive_abort_reasons.clear()
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
            if hasattr(self.strategy, "clear_trace"):
                self.strategy.clear_trace()
            await handle_autorun_tick(self, bot, game_state)
        except Exception as e:
            logger.opt(exception=e).exception("run_tick error")
            full_tb = traceback.format_exc()
            logger.debug("Full traceback:\n{}", full_tb)
            await self.abort(f"fatal: {e}")

    async def abort(self, reason: str = "fatal error", *, push: bool = True) -> None:
        async with self._lock:
            reason = reason or "fatal error"
            current_task = asyncio.current_task()
            is_main_loop_abort = (
                self.mode == "continuous"
                and self.running
                and self._loop_task is current_task
            )

            # 如果需要收到retry-timeout的时候立刻停止的话、把下面的字符串改成"retry-timeout"
            if is_main_loop_abort and "retry-timeoutA" not in reason:
                self.last_error = reason
                self._deferred_abort_this_tick = True
                self._consecutive_abort_reasons.append(reason)
                abort_count = len(self._consecutive_abort_reasons)
                logger.warning(
                    "[autorun] abort requested during main loop tick ({}/{}); skip this tick: {}",
                    abort_count,
                    self.MAX_CONSECUTIVE_ABORT_TICKS,
                    reason,
                )

                if abort_count < self.MAX_CONSECUTIVE_ABORT_TICKS:
                    if push:
                        await self._broadcast_status(safe=True)
                    return

                reasons = "\n".join(
                    f"{idx}. {abort_reason}"
                    for idx, abort_reason in enumerate(self._consecutive_abort_reasons, start=1)
                )
                logger.warning(
                    "[autorun] consecutive abort threshold reached; stopping main loop:\n{}",
                    reasons,
                )
                reason = (
                    f"fatal: consecutive aborts reached {self.MAX_CONSECUTIVE_ABORT_TICKS} ticks\n"
                    f"{reasons}"
                )
                self._consecutive_abort_reasons.clear()
            elif is_main_loop_abort:
                self._consecutive_abort_reasons.clear()
                logger.warning("[autorun] retry-timeout abort; stopping main loop immediately: {}", reason)

            self.last_error = reason

            try:
                self._notify_email_failure_sync(self.last_error)
            except Exception:
                logger.exception("send failure email failed")

            self.running = False
            self.elapsed_ms = self._calc_elapsed_ms()
            self._started_mono_ms = 0
            # 停掉主循环
            if self._loop_task and not self._loop_task.done() and self._loop_task is not current_task:
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
        resp_data = (resp or {}).get("data") or {}
        has_live_game = (resp_data.get("game") or (resp_data.get("data") or {}).get("game")) is not None
        await self._recompute_ready_flags_from_last_probe()
        pf_ready, pf_peer = self._preferred_flow_status()
        try:
            current_achieved_count = self.count_achieved_now()
        except Exception:
            current_achieved_count = 0
        return {
            "mode": self.mode,
            "running": self.running,
            "runs": self.runs,
            "elapsed_ms": self._calc_elapsed_ms(),
            "best_achieved_count": self.best_achieved_count,
            "current_achieved_count": current_achieved_count,
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
            "remake_records": self.remake_records,
            "best_remake_record": self.best_remake_record,
            "record_detailed_operations": self.record_detailed_operations,
            "operation_records": self.operation_records if self.record_detailed_operations else [],
            "operation_count_by_run": self.operation_count_by_run if self.record_detailed_operations else {},
            "decision_trace": [entry.__dict__ for entry in getattr(self.strategy, "decision_trace", [])],
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
        from backend.autorun.value import amulet_matches_target

        return amulet_matches_target(effect_item, target)

    def match_targets_for_amulet(self, effect_item: Dict[str, Any], targets: List[Dict[str, Any]]) -> List[int]:
        from backend.autorun.value import match_targets_for_amulet

        return match_targets_for_amulet(effect_item, targets)

    def count_achieved_for_effect_list(self, eff_list: List[Dict[str, Any]]) -> int:
        return calc_target_achievement_value(eff_list, self.targets)

    def count_achieved_now(self) -> int:
        gs = self._get_game_state()
        try:
            d = gs.to_dict() if hasattr(gs, "to_dict") else (gs or {})
        except Exception:
            d = (gs or {})
        eff_list = d.get("effect_list") or []
        return self.count_achieved_for_effect_list(eff_list)

    @staticmethod
    def _compact_effect_item(effect_item: Dict[str, Any]) -> Dict[str, Any]:
        item: Dict[str, Any] = {}
        for key in ("id", "uid", "volume"):
            if key in effect_item:
                item[key] = effect_item.get(key)
        badge = effect_item.get("badge")
        if isinstance(badge, dict):
            compact_badge: Dict[str, Any] = {}
            for key in ("id",):
                if key in badge:
                    compact_badge[key] = badge.get(key)
            if compact_badge:
                item["badge"] = compact_badge
        return item

    @staticmethod
    def _json_safe(value: Any) -> Any:
        if isinstance(value, dict):
            return {str(k): AutoRunner._json_safe(v) for k, v in value.items()}
        if isinstance(value, list):
            return [AutoRunner._json_safe(v) for v in value]
        if isinstance(value, tuple):
            return [AutoRunner._json_safe(v) for v in value]
        if isinstance(value, (str, int, float, bool)) or value is None:
            return value
        return str(value)

    def record_operation(
            self,
            action: str,
            *,
            reason: str = "",
            result: Optional[str] = None,
            details: Optional[Dict[str, Any]] = None,
            game_state: Optional[GameState] = None,
            run_index: Optional[int] = None,
    ) -> Dict[str, Any]:
        if not self.record_detailed_operations:
            return {}

        gs = game_state
        if gs is None:
            try:
                gs = self._get_game_state()
            except Exception:
                gs = None

        if run_index is None:
            run_index = int(self.runs or 0)
            if action == "start_game" and self.need_start_game:
                run_index += 1
        run_index = max(0, int(run_index or 0))

        current_count = int(self.operation_count_by_run.get(run_index, 0)) + 1
        self.operation_count_by_run[run_index] = current_count
        self._operation_seq += 1

        record = {
            "seq": self._operation_seq,
            "run_index": run_index,
            "op_index": current_count,
            "ts": _now_wall_ms(),
            "stage": getattr(gs, "stage", None),
            "level": getattr(gs, "level", None),
            "step": self.current_step or "-",
            "action": action,
            "reason": reason or "",
            "result": result or "",
            "details": self._json_safe(details or {}),
        }
        self.operation_records.append(record)
        if len(self.operation_records) > 500:
            del self.operation_records[:-500]
        return record

    def _record_remake_snapshot(self, reason: str, game_state: GameState) -> None:
        effect_list = [self._compact_effect_item(dict(it)) for it in (getattr(game_state, "effect_list", None) or [])]
        try:
            achieved_value = self.count_achieved_for_effect_list(effect_list)
        except Exception:
            achieved_value = 0
        run_index = int(self.runs or 0)
        run_operation_records = [
            dict(item)
            for item in (self.operation_records or [])
            if int(item.get("run_index") or 0) == run_index
        ] if self.record_detailed_operations else []
        record = {
            "seq": len(self.remake_records) + 1,
            "run_index": run_index,
            "ts": _now_wall_ms(),
            "reason": reason or "remake",
            "stage": getattr(game_state, "stage", None),
            "level": getattr(game_state, "level", None),
            "target_value": achieved_value,
            "amulet_count": len(effect_list),
            "effect_list": effect_list,
            "operation_count": len(run_operation_records) if self.record_detailed_operations else 0,
            "operation_records": self._json_safe(run_operation_records) if self.record_detailed_operations else [],
        }
        self.remake_records.append(record)
        best = self.best_remake_record
        if best is None or int(record.get("target_value") or 0) > int(best.get("target_value") or 0):
            self.best_remake_record = record

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

def _target_value(t: Dict[str, Any]) -> int:
    return target_value(t)


def _reg_id_of_raw(raw_id: int) -> int:
    return reg_id_of_raw(raw_id)


def _candidate_badge_id(c: Dict[str, Any]) -> Optional[int]:
    return candidate_badge_id(c)


def _owned_badge_ids(effect_list: List[Dict[str, Any]]) -> List[Optional[int]]:
    res: List[Optional[int]] = []
    for e in effect_list or []:
        _reg, _plus, badge_id = extract_amulet_signature(e)
        res.append(badge_id)
    return res


def _owned_count_with_badge(effect_list: List[Dict[str, Any]], want_badge: int) -> int:
    return sum(1 for badge_id in _owned_badge_ids(effect_list) if badge_id == int(want_badge))


def _candidate_price(raw_id: int, badge_id: Optional[int]) -> int:
    return calc_candidate_price({"id": raw_id, "badgeId": badge_id or 0}, amulet_registry=getattr(app_mod, "AMULET_REG", None))


def _selected_effect_value(
        effect_item: Dict[str, Any],
        effect_list_before_select: List[Dict[str, Any]],
        targets: List[Dict[str, Any]],
        need_pionner_badge_count: int = NEED_PIONNER_BADGE_COUNT,
) -> int:
    raw_id = int(effect_item.get("id", 0) or 0)
    if raw_id <= 0:
        return 0
    _reg, _is_plus, badge_id = extract_amulet_signature(effect_item)
    decision = default_select_amulet_from_candidates(
        [{"id": raw_id, "badgeId": badge_id or 0}],
        effect_list_before_select,
        targets,
        need_pionner_badge_count=need_pionner_badge_count,
        amulet_registry=getattr(app_mod, "AMULET_REG", None),
    )
    return decision.selection_value


def _extract_new_amulets_from_select_resp(resp: Optional[dict]) -> List[Dict[str, Any]]:
    from backend.autorun.stages import _extract_new_amulets_from_select_resp as extract_new

    return extract_new(resp)


def _required_nonplus_badges_for_reg(targets: List[Dict[str, Any]], reg_id: int) -> set[int]:
    req: set[int] = set()
    for t in targets or []:
        if t.get("kind") != "amulet":
            continue
        try:
            tid = int(t.get("id"))
        except Exception:
            continue
        if tid != reg_id or bool(t.get("plus", False)):
            continue
        tb = t.get("badge", None)
        if tb not in (None, ""):
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
    if is_needed_for_any_target(e, targets):
        return 10 ** 9
    reg_id, _is_plus, badge_id = extract_amulet_signature(e)
    base = _candidate_price(int(e.get("id", 0) or 0), badge_id)
    if badge_id == 600070:
        base += 10000
    if badge_id == 600110:
        base += 1000
    if reg_id == 146:
        base += 10000
    return base


def _pick_uid_to_sell_same_reg(effect_list: List[Dict[str, Any]], reg_id: int, targets: List[Dict[str, Any]]) -> Optional[int]:
    cands: List[Dict[str, Any]] = []
    for e in effect_list or []:
        try:
            if int(e.get("id", 0)) // 10 == reg_id:
                cands.append(e)
        except Exception:
            continue
    if not cands:
        return None
    worst = min(cands, key=lambda x: (_owned_effect_value_for_selling(x, targets), int(x.get("uid") or 1_000_000_000)))
    uid = worst.get("uid")
    try:
        return int(uid) if uid is not None else None
    except Exception:
        return None


def select_amulet_from_candidates(
        candidate_effect_list: List[Dict[str, Any]],
        effect_list: List[Dict[str, Any]],
        targets: List[Dict[str, Any]],
        need_pionner_badge_count: int = NEED_PIONNER_BADGE_COUNT,
) -> Tuple[Optional[int], Optional[int], Optional[int], Optional[int]]:
    decision = default_select_amulet_from_candidates(
        candidate_effect_list,
        effect_list,
        targets,
        need_pionner_badge_count=need_pionner_badge_count,
        amulet_registry=getattr(app_mod, "AMULET_REG", None),
    )
    return decision.raw_id, decision.badge_id, decision.selection_value, decision.sell_uid


def total_volume(effect_list: List[Dict[str, Any]]) -> int:
    return calc_total_volume(effect_list)


def find_uid_for_raw_or_plus(effect_list: List[Dict[str, Any]], best_raw: int) -> Optional[int]:
    try:
        raw = int(best_raw)
    except Exception:
        return None
    if raw <= 0:
        return None
    reg = raw // 10
    target_ids = {raw, reg * 10 + 1}
    for it in effect_list or []:
        try:
            if int(it.get("id", -1)) in target_ids:
                uid = it.get("uid")
                return int(uid) if uid is not None else None
        except Exception:
            continue
    return None


def _extract_amulet_signature(effect_item: Dict[str, Any]) -> Tuple[int, bool, Optional[int]]:
    return extract_amulet_signature(effect_item)


def _is_needed_for_any_target(effect_item: Dict[str, Any], targets: List[Dict[str, Any]]) -> bool:
    return is_needed_for_any_target(effect_item, targets)


def sort_sell_priority(
        effect_list: List[Dict[str, Any]],
        targets: List[Dict[str, Any]],
        need_pionner_badge_count: int = NEED_PIONNER_BADGE_COUNT,
) -> List[Dict[str, Any]]:
    class _State:
        pass

    state = _State()
    state.effect_list = effect_list
    state.candidate_effect_list = []
    strategy = DefaultAutoRunStrategy()
    from backend.autorun.strategy import AutoRunStrategyContext
    ranked = strategy.rank_sell_candidates(AutoRunStrategyContext(
        game_state=state,
        targets=targets,
        need_pionner_badge_count=need_pionner_badge_count,
        amulet_registry=getattr(app_mod, "AMULET_REG", None),
    ))
    return [candidate.item for candidate in ranked]


def select_items_to_sell_for_purchase(
        free_space: int,
        need_space: int,
        sell_candidates: List[Dict[str, Any]],
) -> Tuple[List[Dict[str, Any]], int, bool]:
    return strategy_select_items_to_sell_for_purchase(free_space, need_space, sell_candidates)
