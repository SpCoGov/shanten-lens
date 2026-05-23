from __future__ import annotations

import asyncio
import contextlib
import ctypes
import json
import os
import platform
import subprocess
import sys
import time
from pathlib import Path
from time import monotonic
from typing import Dict, Set, Any

from loguru import logger
from platformdirs import user_data_dir
from watchfiles import awatch
from websockets.exceptions import ConnectionClosed
from websockets.legacy.server import WebSocketServerProtocol, serve

from backend.autorun.runner import AutoRunner
from backend.autorun.util.retry_1004 import call_with_1004_retry_async
from backend.bot.drivers.packet.packet_bot import PacketBot
from backend.config import build_manager
from backend.data.registry_loader import load_registry_list
from backend.model.game_state import GameState
from backend.model.items import AmuletRegistry, BadgeRegistry
from backend.packet_monitor import PACKET_MONITOR
from backend.ui_runtime import start_ui_loop_once, get_ui_loop, post_coro, mark_ui_services_started
from backend.version import APP_VERSION

GAME_STATE = GameState()
PACKET_BOT: PacketBot
APP_LOOP: asyncio.AbstractEventLoop | None = None
UI_STOP: asyncio.Event | None = None


def get_app_loop() -> asyncio.AbstractEventLoop:
    return get_ui_loop()


try:
    ctypes.windll.user32.SetProcessDPIAware()
except Exception:
    pass


def default_data_root() -> Path:
    return Path(user_data_dir(appname="Shanten Lens", appauthor=None))


def setup_logging():
    LOG_DIR.mkdir(parents=True, exist_ok=True)

    log_file = LOG_DIR / "{time:YYYYMMDD_HHmmss}.log"

    logger.remove()

    # stdout_level = "DEBUG" if MANAGER.get("general.debug", False) else "INFO"
    console_sink = sys.stdout if sys.stdout is not None else sys.stderr
    if console_sink is not None:
        try:
            logger.add(
                console_sink,
                level="INFO",
                backtrace=True,
                diagnose=False,
                enqueue=True,
            )
        except TypeError:
            console_sink = None
        except Exception:
            console_sink = None

    if console_sink is not None:
        pass

    try:
        logger.add(
            str(log_file),
            level="DEBUG",
            rotation="20 MB",
            retention="14 days",
            compression="zip",
            encoding="utf-8",
            backtrace=True,
            diagnose=False,
            enqueue=True,
            format=(
                "{time:YYYY-MM-DD HH:mm:ss.SSS} | {level: <8} | "
                "{process.name}:{thread.name} | {name}:{function}:{line} - {message}"
            ),
        )
    except Exception:
        pass


DATA_ROOT: Path = default_data_root()
CONF_DIR: Path = DATA_ROOT / "configs"
DATA_DIR: Path = DATA_ROOT / "data"
LOG_DIR: Path = DATA_ROOT / "logs"

MANAGER = build_manager(CONF_DIR)

setup_logging()
AMULET_REG: AmuletRegistry | None = None
BADGE_REG: BadgeRegistry | None = None

AUTORUNNER = AutoRunner(
    get_config=lambda: MANAGER.to_table_payload("autorun"),
    get_game_state=lambda: GAME_STATE,
)


def _load_registries() -> None:
    global AMULET_REG, BADGE_REG
    amulets_list = load_registry_list("amulets", external_dir=DATA_DIR, write_back_if_missing=True)
    badges_list = load_registry_list("badges", external_dir=DATA_DIR, write_back_if_missing=True)
    AMULET_REG = AmuletRegistry.from_json_obj(amulets_list)
    BADGE_REG = BadgeRegistry.from_json_obj(badges_list)


def _registry_payload() -> Dict[str, Any]:
    assert AMULET_REG and BADGE_REG
    return {
        "amulets": AMULET_REG.to_json_obj(),
        "badges": BADGE_REG.to_json_obj(),
    }


def set_data_root(path: str | Path) -> None:
    global DATA_ROOT, CONF_DIR, MANAGER
    DATA_ROOT = Path(path)
    CONF_DIR = DATA_ROOT / "configs"
    MANAGER = build_manager(CONF_DIR)
    _load_registries()


_load_registries()

CLIENTS: Set[WebSocketServerProtocol] = set()
PACKET_MONITOR_ENABLED = False
PACKET_MONITOR_BLOCKED_METHODS: Set[str] = {".lq.Route.heartbeat"}


def packet_monitor_settings_payload() -> Dict[str, Any]:
    return {
        "enabled": PACKET_MONITOR_ENABLED,
        "blockedMethods": sorted(PACKET_MONITOR_BLOCKED_METHODS),
    }


def should_emit_packet_monitor(method: str) -> bool:
    if not PACKET_MONITOR_ENABLED:
        return False
    return method not in PACKET_MONITOR_BLOCKED_METHODS


def packet_monitor_snapshot_payload() -> Dict[str, Any]:
    if not PACKET_MONITOR_ENABLED:
        return {"packets": []}
    blocked = PACKET_MONITOR_BLOCKED_METHODS
    packets = [pkt for pkt in PACKET_MONITOR.snapshot() if str(pkt.get("method") or "") not in blocked]
    return {"packets": packets}


async def _broadcast_on_ui_loop(pkt: Dict[str, Any]) -> None:
    dead: list[WebSocketServerProtocol] = []
    for c in list(CLIENTS):
        try:
            await c.send(json.dumps(pkt, ensure_ascii=False))
        except Exception:
            dead.append(c)
    for c in dead:
        CLIENTS.discard(c)


async def _ui_services_main(host: str, ws_port: int):
    global UI_STOP
    if UI_STOP is None:
        UI_STOP = asyncio.Event()
    watcher_cfg = asyncio.create_task(_watch_configs())
    watcher_reg = asyncio.create_task(_watch_data_tables())

    async with serve(
            ws_handler,
            host,
            ws_port,
            max_size=2 ** 20,
            ping_interval=20,
            ping_timeout=60,
    ):
        logger.info(f"Websocket listening on ws://{host}:{ws_port}/")
        try:
            await UI_STOP.wait()
        except asyncio.CancelledError:
            pass
        finally:
            for t in (watcher_cfg, watcher_reg):
                t.cancel()
            await asyncio.gather(watcher_cfg, watcher_reg, return_exceptions=True)


_UI_TASK_FUT = None


def start_ui_services(host: str = "127.0.0.1", ws_port: int = 8787) -> None:
    if not mark_ui_services_started():
        return
    loop = start_ui_loop_once()
    global _UI_TASK_FUT
    _UI_TASK_FUT = asyncio.run_coroutine_threadsafe(_ui_services_main(host, ws_port), loop)


async def _watch_data_tables():
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    logger.info(f"registries watching: {DATA_DIR}")
    async for changes in awatch(str(DATA_DIR)):
        try:
            _load_registries()
            await broadcast({"type": "update_registry", "data": _registry_payload()})
            logger.info("registry updated & broadcast")
        except Exception as e:
            logger.error(f"reload registries failed: {e}")


async def ws_send(ws: WebSocketServerProtocol, pkt: Dict[str, Any]):
    try:
        await ws.send(json.dumps(pkt, ensure_ascii=False))
    except Exception:
        pass


async def broadcast(pkt: Dict[str, Any]) -> None:
    ui_loop = get_ui_loop()
    try:
        cur_loop = asyncio.get_running_loop()
    except RuntimeError:
        cur_loop = None

    if cur_loop is ui_loop:
        await _broadcast_on_ui_loop(pkt)
        return

    fut = asyncio.run_coroutine_threadsafe(_broadcast_on_ui_loop(pkt), ui_loop)
    await asyncio.wrap_future(fut)


def post_broadcast(pkt: Dict[str, Any]) -> None:
    post_coro(_broadcast_on_ui_loop(pkt))


def broadcast_sync_ui_toast(
        kind: str,
        msg: str = "",
        *,
        msg_key: str = "",
        msg_values: Dict[str, Any] | None = None,
        duration: int = 2200,
) -> None:
    data: Dict[str, Any] = {"kind": kind, "duration": duration}
    if msg_key:
        data["msg_key"] = msg_key
        data["msg_values"] = msg_values or {}
    else:
        data["msg"] = msg
    post_broadcast({"type": "ui_toast", "data": data})


def _open_dir(path: str):
    if sys.platform.startswith("win"):
        os.startfile(path)  # type: ignore
    elif sys.platform == "darwin":
        subprocess.Popen(["open", path])
    else:
        subprocess.Popen(["xdg-open", path])


recent_writes: dict[str, float] = {}
SELF_WIN_MS = 500  # 毫秒


async def ws_handler(ws: WebSocketServerProtocol):
    CLIENTS.add(ws)

    await ws_send(ws, {"type": "backend_hello", "data": {"version": APP_VERSION}})
    await ws_send(ws, {"type": "update_fuse_config", "data": MANAGER.to_table_payload("fuse")})
    await ws_send(ws, {"type": "update_autorun_config", "data": MANAGER.to_table_payload("autorun")})
    await ws_send(ws, {"type": "update_registry", "data": _registry_payload()})
    await ws_send(ws, {"type": "update_config", "data": MANAGER.to_payload()})
    await ws_send(ws, {"type": "update_gamestate", "data": GAME_STATE.to_dict()})
    await ws_send(ws, {"type": "autorun_status", "data": await AUTORUNNER.status_payload_async()})
    await ws_send(ws, {"type": "packet_monitor_settings", "data": packet_monitor_settings_payload()})
    await ws_send(ws, {"type": "packet_monitor_snapshot", "data": packet_monitor_snapshot_payload()})

    try:
        async for raw in ws:
            try:
                pkt = json.loads(raw)
            except Exception:
                continue

            t = pkt.get("type")
            data = pkt.get("data", {})

            if t == "frontend_hello":
                frontend_version = str((data or {}).get("version") or "")
                if frontend_version and frontend_version != APP_VERSION:
                    await ws_send(ws, {
                        "type": "version_mismatch",
                        "data": {
                            "frontendVersion": frontend_version,
                            "backendVersion": APP_VERSION,
                        },
                    })

            elif t == "keep_alive":
                pass

            elif t == "edit_config":
                if isinstance(data, dict):
                    written_paths = MANAGER.apply_patch(data)
                    now_ms = monotonic() * 1000
                    for p in written_paths:
                        recent_writes[str(p)] = now_ms
                    if "autorun" in data:
                        AUTORUNNER.update_config(MANAGER.to_table_payload("autorun"))
                        await broadcast({"type": "update_autorun_config", "data": MANAGER.to_table_payload("autorun")})
                        await broadcast({"type": "autorun_status", "data": await AUTORUNNER.status_payload_async()})
                    if "fuse" in data:
                        await broadcast({"type": "update_fuse_config", "data": MANAGER.to_table_payload("fuse")})
                    await broadcast({"type": "update_config", "data": MANAGER.to_payload()})

            elif t == "request_update":
                await ws_send(ws, {"type": "update_fuse_config", "data": MANAGER.to_table_payload("fuse")})
                await ws_send(ws, {"type": "update_autorun_config", "data": MANAGER.to_table_payload("autorun")})
                await ws_send(ws, {"type": "update_config", "data": MANAGER.to_payload()})
                await ws_send(ws, {"type": "update_gamestate", "data": GAME_STATE.to_dict()})
                await ws_send(ws, {"type": "update_registry", "data": _registry_payload()})

            elif t == "fetch_amulet_activity_data":
                try:
                    activity_id = int((data or {}).get("activityId", getattr(PACKET_BOT, "activity_id", 250811)))

                    addon = PACKET_BOT.get_addon()
                    if not addon:
                        await ws_send(ws, {"type": "ui_toast", "data": {"kind": "error", "msg": "注入失败：addon 未就绪"}})
                        continue

                    ok, detail, msg_id = addon.inject_now(
                        method=".lq.Lobby.fetchAmuletActivityData",
                        data={"activityId": activity_id},
                        t="Req",
                    )
                    if not ok:
                        await ws_send(ws, {"type": "ui_toast", "data": {"kind": "error", "msg": f"注入失败：{detail}"}})
                    else:
                        await ws_send(ws, {"type": "ui_toast", "data": {"kind": "success", "msg": f"已发送刷新请求（msg_id={msg_id}）"}})
                except Exception as e:
                    await ws_send(ws, {"type": "ui_toast", "data": {"kind": "error", "msg": f"注入异常：{e}"}})

            elif t == "upgrade_shop_buff":
                try:
                    activity_id = int((data or {}).get("activityId", getattr(PACKET_BOT, "activity_id", 250811)))
                    buff_id = int((data or {}).get("id", 0))
                    upgrade_costs = [5, 10, 15, 20, 50, 100, 150, 200]
                    current_level = int((GAME_STATE.shop_buff_list or {}).get(buff_id, 0))
                    current_coin = int(getattr(GAME_STATE, "coin", 0) or 0)

                    if current_level >= len(upgrade_costs):
                        await ws_send(ws, {
                            "type": "upgrade_shop_buff_result",
                            "data": {"ok": False, "reason": "maxed", "id": buff_id, "level": current_level},
                        })
                        continue

                    required_coin = upgrade_costs[current_level]
                    if current_coin < required_coin:
                        await ws_send(ws, {
                            "type": "upgrade_shop_buff_result",
                            "data": {
                                "ok": False,
                                "reason": "insufficient_coin",
                                "id": buff_id,
                                "level": current_level,
                                "cost": required_coin,
                                "coin": current_coin,
                            },
                        })
                        continue

                    addon = PACKET_BOT.get_addon()
                    if not addon:
                        await ws_send(ws, {
                            "type": "upgrade_shop_buff_result",
                            "data": {"ok": False, "reason": "addon-not-ready", "id": buff_id, "level": current_level},
                        })
                        continue

                    ok, detail, msg_id = addon.inject_now(
                        method=".lq.Lobby.amuletActivityUpgradeShopBuff",
                        data={"activityId": activity_id, "id": buff_id},
                        t="Req",
                    )
                    await ws_send(ws, {
                        "type": "upgrade_shop_buff_result",
                        "data": {
                            "ok": ok,
                            "reason": detail if not ok else "",
                            "id": buff_id,
                            "level": current_level,
                            "cost": required_coin,
                            "coin": current_coin,
                            "msg_id": msg_id,
                        },
                    })
                except Exception as e:
                    await ws_send(ws, {
                        "type": "upgrade_shop_buff_result",
                        "data": {"ok": False, "reason": f"exception:{e}"},
                    })

            elif t == "amulet_hotkey_action":
                action = str((data or {}).get("action") or "")

                async def _hotkey_result(ok: bool, reason: str = "", **extra):
                    await ws_send(ws, {
                        "type": "amulet_hotkey_action_result",
                        "data": {"ok": ok, "reason": reason, "action": action, **extra},
                    })

                try:
                    bot = getattr(sys.modules.get("backend.app"), "PACKET_BOT", None)
                    if not bot:
                        await _hotkey_result(False, "addon-not-ready")
                        continue

                    if action == "sort_effect":
                        raw_sorted_uid = (data or {}).get("sortedUid") or []
                        if not isinstance(raw_sorted_uid, list):
                            await _hotkey_result(False, "invalid-sorted-uid")
                            continue
                        try:
                            sorted_uid = [int(uid) for uid in raw_sorted_uid]
                        except Exception:
                            await _hotkey_result(False, "invalid-sorted-uid")
                            continue
                        ok, reason, _ = await call_with_1004_retry_async(
                            bot.sort_effect,
                            sorted_uid=sorted_uid,
                            delay_sec=3,
                            interval=0.4,
                            timeout=12,
                            to_thread=True,
                        )
                        await _hotkey_result(ok, "" if ok else reason, sortedUid=sorted_uid)
                        continue

                    current_stage = int(getattr(GAME_STATE, "stage", -1) or -1)
                    if current_stage not in {1, 4, 5, 7}:
                        await _hotkey_result(False, "stage-not-allowed", stage=current_stage)
                        continue

                    if action == "buy_pack":
                        if current_stage != 4:
                            await _hotkey_result(False, "stage-not-allowed", stage=current_stage)
                            continue
                        good_id = int((data or {}).get("goodId", 0) or 0)
                        ok, reason, _ = await call_with_1004_retry_async(
                            bot.buy_pack,
                            good_id=good_id,
                            delay_sec=3,
                            interval=0.4,
                            timeout=12,
                            to_thread=True,
                        )
                        await _hotkey_result(ok, "" if ok else reason, goodId=good_id)
                        continue

                    if action == "refresh_shop":
                        if current_stage != 4:
                            await _hotkey_result(False, "stage-not-allowed", stage=current_stage)
                            continue
                        ok, reason, _ = await call_with_1004_retry_async(
                            bot.refresh_shop,
                            delay_sec=3,
                            interval=0.4,
                            timeout=12,
                            to_thread=True,
                        )
                        await _hotkey_result(ok, "" if ok else reason)
                        continue

                    if action == "select_candidate":
                        selected_id = int((data or {}).get("selectedId", 0) or 0)
                        if current_stage == 1:
                            if selected_id == 0:
                                await _hotkey_result(False, "skip-not-allowed", stage=current_stage)
                                continue
                            fn = bot.select_free_effect
                        elif current_stage == 5:
                            fn = bot.select_effect
                        elif current_stage == 7:
                            fn = bot.select_reward_effect
                        else:
                            await _hotkey_result(False, "stage-not-allowed", stage=current_stage)
                            continue
                        ok, reason, _ = await call_with_1004_retry_async(
                            fn,
                            selected_id=selected_id,
                            delay_sec=3,
                            interval=0.4,
                            timeout=12,
                            to_thread=True,
                        )
                        await _hotkey_result(ok, "" if ok else reason, selectedId=selected_id)
                        continue

                    if action == "sell_effect":
                        uid = int((data or {}).get("uid", 0) or 0)
                        if uid <= 0:
                            await _hotkey_result(False, "unknown id")
                            continue
                        ok, reason, _ = await call_with_1004_retry_async(
                            bot.sell_effect,
                            uid=uid,
                            delay_sec=3,
                            interval=0.4,
                            timeout=12,
                            to_thread=True,
                        )
                        await _hotkey_result(ok, "" if ok else reason, uid=uid)
                        continue

                    if action == "sell_recent":
                        effect_list = list(getattr(GAME_STATE, "effect_list", None) or [])
                        uid = None
                        if str((data or {}).get("mode") or "last_list") == "last_selected":
                            raw_id = int((data or {}).get("rawId", 0) or 0)
                            for effect in reversed(effect_list):
                                if not isinstance(effect, dict):
                                    continue
                                try:
                                    if int(effect.get("id", 0) or 0) == raw_id:
                                        uid = int(effect.get("uid", 0) or 0)
                                        break
                                except Exception:
                                    pass
                            if not uid:
                                await _hotkey_result(False, "selected-effect-not-found", rawId=raw_id)
                                continue
                        else:
                            for effect in reversed(effect_list):
                                if not isinstance(effect, dict):
                                    continue
                                try:
                                    uid = int(effect.get("uid", 0) or 0)
                                except Exception:
                                    uid = None
                                if uid:
                                    break
                        if not uid:
                            await _hotkey_result(False, "no-effects")
                            continue
                        ok, reason, _ = await call_with_1004_retry_async(
                            bot.sell_effect,
                            uid=uid,
                            delay_sec=3,
                            interval=0.4,
                            timeout=12,
                            to_thread=True,
                        )
                        await _hotkey_result(ok, "" if ok else reason, uid=uid)
                        continue

                    await _hotkey_result(False, "unknown-action")
                except Exception as e:
                    await _hotkey_result(False, f"exception:{e}")

            elif t == "open_config_dir":
                try:
                    _open_dir(str(CONF_DIR))
                    await ws_send(ws, {"type": "open_result", "data": {"ok": True}})
                except Exception as e:
                    await ws_send(ws, {"type": "open_result", "data": {"ok": False, "error": str(e)}})

            elif t == "packet_monitor_request_snapshot":
                await ws_send(ws, {"type": "packet_monitor_snapshot", "data": packet_monitor_snapshot_payload()})

            elif t == "mitm_dump_flows":
                try:
                    bot = globals().get("PACKET_BOT")
                    addon = bot.get_addon() if bot and hasattr(bot, "get_addon") else None
                    if not addon:
                        await ws_send(ws, {
                            "type": "mitm_dump_flows_result",
                            "data": {"ok": False, "reason": "addon-not-ready", "flows": []},
                        })
                        await ws_send(ws, {
                            "type": "ui_toast",
                            "data": {"kind": "error", "msg": "MITM addon 未就绪，无法打印 flow", "duration": 2600},
                        })
                        continue

                    flows = addon.dump_current_flows()
                    await ws_send(ws, {
                        "type": "mitm_dump_flows_result",
                        "data": {"ok": True, "flows": flows, "count": len(flows)},
                    })
                    await ws_send(ws, {
                        "type": "ui_toast",
                        "data": {"kind": "success", "msg": f"已打印 {len(flows)} 个 MITM flow 到日志", "duration": 2200},
                    })
                except Exception as e:
                    logger.exception("mitm dump flows failed")
                    await ws_send(ws, {
                        "type": "mitm_dump_flows_result",
                        "data": {"ok": False, "reason": str(e), "flows": []},
                    })
                    await ws_send(ws, {
                        "type": "ui_toast",
                        "data": {"kind": "error", "msg": f"打印 MITM flow 失败：{e}", "duration": 2600},
                    })

            elif t == "packet_monitor_update_settings":
                enabled = bool((data or {}).get("enabled", False))
                blocked_methods_raw = (data or {}).get("blockedMethods") or []
                blocked_methods = {
                    str(item).strip()
                    for item in blocked_methods_raw
                    if str(item).strip()
                }

                global PACKET_MONITOR_ENABLED, PACKET_MONITOR_BLOCKED_METHODS
                PACKET_MONITOR_ENABLED = enabled
                PACKET_MONITOR_BLOCKED_METHODS = blocked_methods
                if not PACKET_MONITOR_ENABLED:
                    PACKET_MONITOR.clear()

                await ws_send(ws, {"type": "packet_monitor_settings", "data": packet_monitor_settings_payload()})
                await ws_send(ws, {"type": "packet_monitor_snapshot", "data": packet_monitor_snapshot_payload()})

            elif t == "packet_monitor_replay":
                method = str((data or {}).get("method") or "")
                payload = (data or {}).get("payload")
                if not method or not isinstance(payload, dict):
                    await ws_send(ws, {
                        "type": "packet_monitor_replay_result",
                        "data": {"ok": False, "reason": "invalid-payload"},
                    })
                    continue

                addon = PACKET_BOT.get_addon()
                if not addon:
                    await ws_send(ws, {
                        "type": "packet_monitor_replay_result",
                        "data": {"ok": False, "reason": "addon-not-ready"},
                    })
                    await ws_send(ws, {
                        "type": "ui_toast",
                        "data": {"kind": "error", "msg": "重放失败：addon 未就绪"},
                    })
                    continue

                ok, reason, msg_id = addon.inject_now(method=method, data=payload, t="Req")
                await ws_send(ws, {
                    "type": "packet_monitor_replay_result",
                    "data": {"ok": ok, "reason": reason, "msg_id": msg_id, "method": method},
                })
                await ws_send(ws, {
                    "type": "ui_toast",
                    "data": {
                        "kind": "success" if ok else "error",
                        "msg": f"重放成功：{method}（msg_id={msg_id}）" if ok else f"重放失败：{reason}",
                    },
                })

            elif t == "autorun_control":
                action = (data or {}).get("action")
                force = bool((data or {}).get("force", False))

                async def _result(ok: bool, reason: str = "", **extra):
                    await ws_send(ws, {"type": "autorun_control_result", "data": {"ok": ok, "reason": reason, **extra}})
                    await ws_send(ws, {"type": "autorun_status", "data": await AUTORUNNER.status_payload_async()})

                if action == "probe":
                    await AUTORUNNER.refresh_probe_now(push=True)
                    continue
                if action == "start":
                    bot = getattr(sys.modules.get("backend.app"), "PACKET_BOT", None)

                    ok, reason, resp = await call_with_1004_retry_async(
                        bot.fetch_amulet_activity_data,
                        delay_sec=8,
                        interval=0.4,
                        timeout=20,
                        to_thread=True,
                    )
                    AUTORUNNER._last_probe_ok = ok
                    AUTORUNNER._last_probe_reason = reason or ""
                    AUTORUNNER._last_probe_resp = resp
                    AUTORUNNER._last_probe_ts = int(time.time() * 1000)
                    await AUTORUNNER._recompute_ready_flags_from_last_probe()

                    if not ok:
                        low = (reason or "").lower()
                        if "addon-or-flow-not-ready" in low or "not ready" in low:
                            return await _result(False, "游戏未启动或流程未就绪")
                        if "timeout" in low:
                            return await _result(False, "连接超时，请检查游戏/代理")
                        return await _result(False, f"探测失败：{reason or 'unknown'}")

                    has_game = AUTORUNNER.probe_has_live_game(resp)
                    if has_game and not force:
                        return await _result(
                            False,
                            "",
                            reason_key="autorun.confirm_existing_game",
                            requires_confirmation=True,
                        )

                    if has_game and force:
                        ok2, reason2, _ = await call_with_1004_retry_async(
                            bot.giveup,
                            delay_sec=8,
                            interval=0.6,
                            timeout=30,
                            to_thread=True,
                        )
                        if not ok2:
                            return await _result(False, f"放弃当前对局失败：{reason2 or 'unknown'}")

                        ok3, reason3, resp3 = await call_with_1004_retry_async(
                            bot.fetch_amulet_activity_data,
                            delay_sec=8,
                            interval=0.6,
                            timeout=10,
                            to_thread=True,
                        )
                        AUTORUNNER._last_probe_ok = ok3
                        AUTORUNNER._last_probe_reason = reason3 or ""
                        AUTORUNNER._last_probe_resp = resp3
                        AUTORUNNER._last_probe_ts = int(time.time() * 1000)
                        await AUTORUNNER._recompute_ready_flags_from_last_probe()

                    try:
                        await AUTORUNNER.start()
                    except Exception as e:
                        return await _result(False, f"开启自动化失败：{e}")

                    return await _result(True, "")
                elif action == "stop":
                    if not AUTORUNNER.running:
                        return await _result(True, "")
                    await AUTORUNNER.stop()
                    return await _result(True, "")
                elif action == "set_mode":
                    mode = (data or {}).get("mode")
                    await AUTORUNNER.set_mode(mode)
                    return await _result(True, "")

                elif action == "step":
                    try:
                        await AUTORUNNER.step_once()
                        return await _result(True, "")
                    except Exception as e:
                        return await _result(False, str(e))
                elif action == "notify_test_email":
                    ok, reason = AUTORUNNER.send_email_notify(
                        subject="Shanten Lens 测试通知",
                        body="这是一封测试邮件：自动化完成/出错后会发送类似的邮件。",
                    )
                    if ok:
                        await ws_send(ws, {
                            "type": "ui_toast",
                            "data": {"kind": "success", "msg_key": "autorun.email_toast.test_success", "duration": 1800}
                        })
                    else:
                        payload = reason or {}
                        await ws_send(ws, {
                            "type": "ui_toast",
                            "data": {
                                "kind": "error",
                                "msg_key": "autorun.email_toast.test_failed",
                                "msg_values": {
                                    "reason_key": payload.get("key", "autorun.email_error.unknown"),
                                    "reason_values": payload.get("values", {}),
                                },
                                "duration": 6000,
                            }
                        })
            elif t == "souzu_switch_control":
                action = (data or {}).get("action")
                if action == "start":
                    from backend.mitm.hooks import start_switch_recommendation_search
                    opts = (data or {}).get("options") or {}
                    await start_switch_recommendation_search(
                        skip_signatures=list(opts.get("skip_signatures") or []),
                        wall_limit=int(opts.get("wall_limit", 36) or 36),
                        search_algorithm=str(opts.get("search_algorithm", "target_enumeration_search") or "target_enumeration_search"),
                    )
                elif action == "start_debug":
                    from backend.mitm.hooks import start_switch_recommendation_debug_search
                    opts = (data or {}).get("options") or {}
                    await start_switch_recommendation_debug_search(
                        snapshot=(data or {}).get("snapshot") or {},
                        skip_signatures=list(opts.get("skip_signatures") or []),
                        wall_limit=int(opts.get("wall_limit", 36) or 36),
                        search_algorithm=str(opts.get("search_algorithm", "target_enumeration_search") or "target_enumeration_search"),
                    )
                elif action == "validate_manual_debug":
                    from backend.mitm.hooks import validate_manual_switch_debug_plan
                    opts = (data or {}).get("options") or {}
                    await validate_manual_switch_debug_plan(
                        snapshot=(data or {}).get("snapshot") or {},
                        quad_groups=list((data or {}).get("quad_groups") or []),
                        structure_groups=dict((data or {}).get("structure_groups") or {}),
                        wall_limit=int(opts.get("wall_limit", 36) or 36),
                    )
                elif action == "stop":
                    from backend.mitm.hooks import stop_switch_recommendation_search
                    await stop_switch_recommendation_search(
                        notify_client=bool((data or {}).get("notify", True))
                    )
                elif action == "list_quads":
                    from backend.mitm.hooks import broadcast_switch_quad_catalog
                    opts = (data or {}).get("options") or {}
                    await broadcast_switch_quad_catalog(
                        wall_limit=int(opts.get("wall_limit", 36) or 36)
                    )
                elif action == "execute_plan":
                    from backend.mitm.hooks import execute_current_switch_plan
                    ok, reason = await execute_current_switch_plan()
                    await ws_send(ws, {
                        "type": "souzu_switch_control_result",
                        "data": {"action": "execute_plan", "ok": ok, "reason": reason},
                    })
                elif action == "execute_full_plan":
                    from backend.mitm.hooks import execute_current_full_plan
                    ok, reason = await execute_current_full_plan()
                    await ws_send(ws, {
                        "type": "souzu_switch_control_result",
                        "data": {"action": "execute_full_plan", "ok": ok, "reason": reason},
                    })
            elif t == "msgbox_result":
                from backend.msgbox import handle_msgbox_result
                handle_msgbox_result(pkt)
    except ConnectionClosed as e:
        logger.info("websocket disconnected: code={} reason={}", e.code, e.reason or "")
    except Exception:
        logger.exception("ws_handler failed")
    finally:
        CLIENTS.discard(ws)


async def _watch_configs():
    CONF_DIR.mkdir(parents=True, exist_ok=True)
    logger.info(f"configs watching: {CONF_DIR}")
    async for changes in awatch(str(CONF_DIR)):
        now = monotonic() * 1000
        should_broadcast_normal = False
        should_broadcast_fuse = False
        should_broadcast_autorun = False

        for _typ, path in changes:
            ts = recent_writes.get(str(path))
            if ts and (now - ts) < SELF_WIN_MS:
                continue
            tname, changed = MANAGER.handle_file_change(Path(path))
            if changed:
                if tname == "fuse":
                    should_broadcast_fuse = True
                elif tname == "autorun":
                    should_broadcast_autorun = True
                else:
                    should_broadcast_normal = True

        if should_broadcast_normal:
            await broadcast({"type": "update_config", "data": MANAGER.to_payload()})
            logger.info("config updated & broadcast")

        if should_broadcast_fuse:
            await broadcast({"type": "update_fuse_config", "data": MANAGER.to_table_payload("fuse")})
            logger.info("fuse config updated & broadcast")

        if should_broadcast_autorun:
            await broadcast({"type": "update_autorun_config", "data": MANAGER.to_table_payload("autorun")})
            AUTORUNNER.update_config(MANAGER.to_table_payload("autorun"))
            logger.info("autorun config updated & broadcast")
