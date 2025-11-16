from __future__ import annotations

import asyncio
import contextlib
import ctypes
import json
import os
import platform
import subprocess
import sys
from pathlib import Path
from time import monotonic
from typing import Dict, Set, Any

import uvicorn
from fastapi import FastAPI, Query
from loguru import logger
from platformdirs import user_data_dir
from watchfiles import awatch
from websockets.legacy.server import WebSocketServerProtocol, serve

from backend.autorun.runner import AutoRunner
from backend.autorun.util.retry_1004 import call_with_1004_retry_async
from backend.bot import BotPipeline, BotConfig
from backend.bot.drivers.packet.packet_bot import PacketBot
from backend.config import build_manager
from backend.data.registry_loader import load_registry_list
from backend.model.game_state import GameState
from backend.model.items import AmuletRegistry, BadgeRegistry
from backend.ui_runtime import start_ui_loop_once, get_ui_loop, post_coro, mark_ui_services_started

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

cfg = BotConfig(
    screen_width=1920,
    screen_height=1080,
    window_title_keyword="雀魂",

    hand_bar_norm=(0.10243, 0.85450, 0.82811, 0.99513),
    button_bar_norm=(0.47454, 0.70785, 0.82542, 0.81008),

    hand_slots=14,
    hand_margin=0.02,
    hand_left_comp_px=0,
    hand_right_comp_px=10,
    button_order=[4, 8, 100],
    button_margin=0.06,

    btn_x_left=6.40, btn_x_right=10.875, btn_y_line=6.45,

    # 点击确认参数
    ack_timeout_sec=1.6, ack_retry=2, ack_settle_ms=140, ack_check_ms=70,
)
pipeline = BotPipeline(cfg)


def default_data_root() -> Path:
    return Path(user_data_dir(appname="Shanten Lens", appauthor=None))


def setup_logging():
    LOG_DIR.mkdir(parents=True, exist_ok=True)

    log_file = LOG_DIR / "{time:YYYYMMDD_HHmmss}.log"

    logger.remove()

    logger.add(
        sys.stdout,
        level="INFO",
        backtrace=True,
        diagnose=False,
        enqueue=True,
    )

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


DATA_ROOT: Path = default_data_root()
CONF_DIR: Path = DATA_ROOT / "configs"
DATA_DIR: Path = DATA_ROOT / "data"
LOG_DIR: Path = DATA_ROOT / "logs"

setup_logging()

MANAGER = build_manager(CONF_DIR)
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

    api_port = int(MANAGER.get("api_port", 8788))
    api_task = asyncio.create_task(run_http_server(host, api_port))

    antiafk_task = asyncio.create_task(anti_afk_loop())

    async with serve(ws_handler, host, ws_port, max_size=2 ** 20):
        logger.info(f"Websocket listening on ws://{host}:{ws_port}/")
        try:
            await UI_STOP.wait()
        except asyncio.CancelledError:
            pass
        finally:
            for t in (watcher_cfg, watcher_reg, api_task, antiafk_task):
                t.cancel()
            await asyncio.gather(watcher_cfg, watcher_reg, api_task, antiafk_task, return_exceptions=True)


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


def _open_dir(path: str):
    if sys.platform.startswith("win"):
        os.startfile(path)  # type: ignore
    elif sys.platform == "darwin":
        subprocess.Popen(["open", path])
    else:
        subprocess.Popen(["xdg-open", path])


recent_writes: dict[str, float] = {}
SELF_WIN_MS = 500  # 毫秒

api_app = FastAPI(title="Shanten Lens API", version="1.0.0")


@api_app.get("/api/gamestate/record")
def api_record():
    return {"type": "request_gamestate", "data": GAME_STATE.record}


@api_app.get("/api/gamestate/effect_list")
def api_effect_list():
    return {"type": "request_effect_list", "data": GAME_STATE.effect_list}


@api_app.get("/api/gamestate/level")
def api_level():
    return {"type": "request_level", "data": GAME_STATE.level}


@api_app.get("/api/discard")
def api_discard(tile_id: int = Query(..., description="要丢的牌的 tile_id")):
    return {"type": "discard", "data": {"ok": pipeline.click_discard_by_tile_id(
        tile_id=tile_id,
        hand_ids_with_draw=GAME_STATE.hand_tiles,
        id2label=GAME_STATE.deck_map,
        allow_tsumogiri=False
    )}}


@api_app.get("/api/testmove")
def api_testmove():
    pipeline.selftest_move()
    return {"type": "testmove", "data": {"ok": True}}


@api_app.get("/api/buy")
def api_buy(good_id: int = Query(...)):
    try:
        ok, reason, resp = PACKET_BOT.buy_pack(good_id)
        return {"type": "give_up", "data": {"ok": ok, "reason": reason, "resp": resp or {}}}
    except Exception as e:
        logger.error(f"reload give_up failed: {e}")


@api_app.get("/api/start")
def api_start():
    try:
        return {"type": "start", "data": {"ok": PACKET_BOT.start_game()}}
    except Exception as e:
        logger.error(f"reload start failed: {e}")


@api_app.get("/api/fetch_amulet_activity_data")
def api_fetch_amulet_activity_data():
    try:
        return {"type": "fetch_amulet_activity_data", "data": {"ok": PACKET_BOT.fetch_amulet_activity_data(delay_sec=3)}}
    except Exception as e:
        logger.error(f"reload start failed: {e}")


async def run_http_server(host: str, port: int):
    config = uvicorn.Config(api_app, host=host, port=port, log_level="info")
    server = uvicorn.Server(config)
    await server.serve()


async def ws_handler(ws: WebSocketServerProtocol):
    CLIENTS.add(ws)

    await ws_send(ws, {"type": "update_fuse_config", "data": MANAGER.to_table_payload("fuse")})
    await ws_send(ws, {"type": "update_autorun_config", "data": MANAGER.to_table_payload("autorun")})
    await ws_send(ws, {"type": "update_registry", "data": _registry_payload()})
    await ws_send(ws, {"type": "update_config", "data": MANAGER.to_payload()})
    await ws_send(ws, {"type": "update_gamestate", "data": GAME_STATE.to_dict()})
    await ws_send(ws, {"type": "autorun_status", "data": await AUTORUNNER.status_payload_async()})

    try:
        async for raw in ws:
            try:
                pkt = json.loads(raw)
            except Exception:
                continue

            t = pkt.get("type")
            data = pkt.get("data", {})

            if t == "keep_alive":
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

            elif t == "open_config_dir":
                try:
                    _open_dir(str(CONF_DIR))
                    await ws_send(ws, {"type": "open_result", "data": {"ok": True}})
                except Exception as e:
                    await ws_send(ws, {"type": "open_result", "data": {"ok": False, "error": str(e)}})

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

                    if not ok:
                        low = (reason or "").lower()
                        if "addon-or-flow-not-ready" in low or "not ready" in low:
                            return await _result(False, "游戏未启动或流程未就绪")
                        if "timeout" in low:
                            return await _result(False, "连接超时，请检查游戏/代理")
                        return await _result(False, f"探测失败：{reason or 'unknown'}")

                    has_game = bool((resp or {}).get("data", {}).get("data", {}).get("game"))
                    if has_game and not force:
                        return await _result(False, "检测到已有对局，是否放弃当前对局并开始？", requires_confirmation=True)

                    if has_game and force:
                        logger.info("force start")
                        ok2, reason2, _ = await call_with_1004_retry_async(
                            bot.giveup,
                            delay_sec=8,
                            interval=0.6,
                            timeout=30,
                            to_thread=True,
                        )
                        if not ok2:
                            return await _result(False, f"放弃当前对局失败：{reason2 or 'unknown'}")

                        await call_with_1004_retry_async(
                            bot.fetch_amulet_activity_data,
                            delay_sec=8,
                            interval=0.6,
                            timeout=10,
                            to_thread=True,
                        )

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
                            "data": {"kind": "success", "msg": "测试邮件已发送", "duration": 1800}
                        })
                    else:
                        await ws_send(ws, {
                            "type": "ui_toast",
                            "data": {"kind": "error", "msg": f"发送失败: {reason or ''}", "duration": 2600}
                        })
            elif t == "msgbox_result":
                from backend.msgbox import handle_msgbox_result
                handle_msgbox_result(pkt)
    except Exception:
        pass
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


async def anti_afk_loop():
    if platform.system().lower() != "windows":
        logger.info("anti-AFK disabled: non-Windows platform")
        return

    logger.info("anti-AFK loop started")
    await asyncio.sleep(1.0)

    while True:
        try:
            enabled = bool(MANAGER.get("game.anti_afk", False))
            interval = 30
            edge_ratio = 0.015

            if enabled:
                ok1 = pipeline.click_left_center_once()
                if not ok1:
                    logger.debug("anti-AFK: first click (left-center) skipped/failed")

                await asyncio.sleep(3)

                ok2 = pipeline.click_left_edge_nudged_once(edge_ratio)
                if not ok2:
                    logger.debug("anti-AFK: second click (left-edge-nudged) skipped/failed")

                remaining = max(0, interval - 3)
                await asyncio.sleep(remaining)
            else:
                await asyncio.sleep(2.0)

        except asyncio.CancelledError:
            raise
        except Exception:
            logger.exception("anti-AFK loop error")
            await asyncio.sleep(3.0)
