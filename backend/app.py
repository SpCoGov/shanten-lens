from __future__ import annotations

import asyncio
import contextlib
import json
import os
import subprocess
import sys
from pathlib import Path
from time import monotonic
from typing import Dict, Set, Any

from loguru import logger
from platformdirs import user_data_dir
from watchfiles import awatch
from websockets.legacy.server import WebSocketServerProtocol, serve

from backend.config import build_manager
from backend.model.game_state import GameState
from backend.data.registry_loader import load_registry_list
from backend.model.items import AmuletRegistry, BadgeRegistry

GAME_STATE = GameState()


def default_data_root() -> Path:
    return Path(user_data_dir(appname="Shanten Lens", appauthor=None))


DATA_ROOT: Path = default_data_root()
CONF_DIR: Path = DATA_ROOT / "configs"
DATA_DIR: Path = DATA_ROOT / "data"

MANAGER = build_manager(CONF_DIR)
AMULET_REG: AmuletRegistry | None = None
BADGE_REG: BadgeRegistry | None = None


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
CLIENTS_LOCK = asyncio.Lock()


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


async def broadcast(pkt: Dict[str, Any]):
    dead: list[WebSocketServerProtocol] = []
    async with CLIENTS_LOCK:
        for c in list(CLIENTS):
            try:
                await c.send(json.dumps(pkt, ensure_ascii=False))
            except Exception:
                dead.append(c)
        for c in dead:
            CLIENTS.discard(c)


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
    async with CLIENTS_LOCK:
        CLIENTS.add(ws)

    await ws_send(ws, {"type": "update_fuse_config", "data": MANAGER.to_table_payload("fuse")})
    await ws_send(ws, {"type": "update_registry", "data": _registry_payload()})
    await ws_send(ws, {"type": "update_config", "data": MANAGER.to_payload()})
    await ws_send(ws, {"type": "update_gamestate", "data": GAME_STATE.to_dict()})

    try:
        async for raw in ws:
            try:
                pkt = json.loads(raw)
            except Exception:
                continue

            t = pkt.get("type")
            data = pkt.get("data", {})

            if t == "keep_alive":
                # no-op
                pass

            elif t == "edit_config":
                if isinstance(data, dict):
                    written_paths = MANAGER.apply_patch(data)
                    now_ms = monotonic() * 1000
                    for p in written_paths:
                        recent_writes[str(p)] = now_ms
                    await broadcast({"type": "update_config", "data": MANAGER.to_payload()})

            elif t == "request_update":
                await ws_send(ws, {"type": "update_fuse_config", "data": MANAGER.to_table_payload("fuse")})
                await ws_send(ws, {"type": "update_config", "data": MANAGER.to_payload()})
                await ws_send(ws, {"type": "update_gamestate", "data": GAME_STATE.to_dict()})
                await ws_send(ws, {"type": "update_registry", "data": _registry_payload()})

            elif t == "open_config_dir":
                try:
                    _open_dir(str(CONF_DIR))
                    await ws_send(ws, {"type": "open_result", "data": {"ok": True}})
                except Exception as e:
                    await ws_send(ws, {"type": "open_result", "data": {"ok": False, "error": str(e)}})
            elif t == "inject_select_pack_now":
                from backend.mitm.addon import WS_ADDON_INSTANCE, WsAddon
                addon: WsAddon = WS_ADDON_INSTANCE
                if not addon:
                    await ws_send(ws, {"type": "inject_ack", "data": {"ok": False, "detail": "ws-addon-not-ready"}})
                else:
                    ok, detail = addon.inject_now(
                        method=".lq.Lobby.amuletActivitySelectPack",
                        data={
                            "activityId": int(data.get("activityId")),
                            "type": int(data.get("type")),
                            "tileList": list(map(int, data.get("tileList", []))),
                        },
                        t="Req",
                        peer_key=data.get("peerKey"),
                    )
                    await ws_send(ws, {"type": "inject_ack", "data": {"ok": ok, "detail": detail}})

    except Exception:
        pass
    finally:
        async with CLIENTS_LOCK:
            CLIENTS.discard(ws)


async def _watch_configs():
    CONF_DIR.mkdir(parents=True, exist_ok=True)
    logger.info(f"configs watching: {CONF_DIR}")
    async for changes in awatch(str(CONF_DIR)):
        now = monotonic() * 1000
        should_broadcast_normal = False
        should_broadcast_fuse = False

        for _typ, path in changes:
            ts = recent_writes.get(str(path))
            if ts and (now - ts) < SELF_WIN_MS:
                continue
            tname, changed = MANAGER.handle_file_change(Path(path))
            if changed:
                if tname == "fuse":
                    should_broadcast_fuse = True
                else:
                    should_broadcast_normal = True

        if should_broadcast_normal:
            await broadcast({"type": "update_config", "data": MANAGER.to_payload()})
            logger.info("config updated & broadcast")

        if should_broadcast_fuse:
            await broadcast({"type": "update_fuse_config", "data": MANAGER.to_table_payload("fuse")})
            logger.info("fuse config updated & broadcast")


async def run_ws_server(host: str, port: int):
    watcher_cfg = asyncio.create_task(_watch_configs())
    watcher_reg = asyncio.create_task(_watch_data_tables())

    async with serve(ws_handler, host, port, max_size=2 ** 20):
        logger.info(f"Websocket listening on ws://{host}:{port}/")
        try:
            await asyncio.Future()
        finally:
            watcher_cfg.cancel()
            watcher_reg.cancel()
            with contextlib.suppress(Exception):
                await watcher_cfg
                await watcher_reg
