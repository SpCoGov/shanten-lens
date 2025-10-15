from __future__ import annotations

import asyncio
import contextlib
import json
import os
import subprocess
import sys
from loguru import logger
from pathlib import Path
from time import monotonic
from typing import Dict, Set, Any

from websockets.legacy.server import WebSocketServerProtocol, serve

from platformdirs import user_data_dir
from watchfiles import awatch

from .config import build_manager  # 唯一入口：构建配置管理器
from .model.game_state import GameState

GAME_STATE = GameState()


def default_data_root() -> Path:
    return Path(user_data_dir(appname="Shanten Lens", appauthor=None))


DATA_ROOT: Path = default_data_root()
CONF_DIR: Path = DATA_ROOT / "configs"

# MANAGER：全局配置管理器（注册表见 backend/config/registry.py）
MANAGER = build_manager(CONF_DIR)


def set_data_root(path: str | Path) -> None:
    """可在运行前覆盖数据根目录（例如命令行参数传入）。"""
    global DATA_ROOT, CONF_DIR, MANAGER
    DATA_ROOT = Path(path)
    CONF_DIR = DATA_ROOT / "configs"
    MANAGER = build_manager(CONF_DIR)


# ========= WS 客户端与广播工具 =========

CLIENTS: Set[WebSocketServerProtocol] = set()
CLIENTS_LOCK = asyncio.Lock()


async def ws_send(ws: WebSocketServerProtocol, pkt: Dict[str, Any]):
    try:
        await ws.send(json.dumps(pkt, ensure_ascii=False))
    except Exception:
        # 单个连接失败忽略
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


# ========= 自写写盘抑制（避免监听器重复广播） =========

recent_writes: dict[str, float] = {}
SELF_WIN_MS = 500  # 毫秒


# ========= WebSocket handler =========

async def ws_handler(ws: WebSocketServerProtocol):
    async with CLIENTS_LOCK:
        CLIENTS.add(ws)

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
                # data: { "<table>": { "k": v, ... }, ... }
                if isinstance(data, dict):
                    written_paths = MANAGER.apply_patch(data)  # List[Path]
                    # 标记自写时间，监听器看见立即变化会被抑制
                    now_ms = monotonic() * 1000
                    for p in written_paths:
                        recent_writes[str(p)] = now_ms
                    # 主动广播一次（监听器会抑制自写，避免重复）
                    await broadcast({"type": "update_config", "data": MANAGER.to_payload()})

            elif t == "request_update":
                await ws_send(ws, {"type": "update_config", "data": MANAGER.to_payload()})
                await ws_send(ws, {"type": "update_gamestate", "data": GAME_STATE.to_dict()})

            elif t == "open_config_dir":
                try:
                    _open_dir(str(CONF_DIR))
                    await ws_send(ws, {"type": "open_result", "data": {"ok": True}})
                except Exception as e:
                    await ws_send(ws, {"type": "open_result", "data": {"ok": False, "error": str(e)}})
            elif t == "inject_select_pack_now":
                # data = { activityId: int, type: int, tileList?: int[], peerKey?: string }
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
            # 其余类型保留拓展

    except Exception:
        # 连接关闭或异常：移除客户端
        pass
    finally:
        async with CLIENTS_LOCK:
            CLIENTS.discard(ws)


# ========= 配置目录监听：外改 -> 合并 -> 广播 =========

async def _watch_configs():
    CONF_DIR.mkdir(parents=True, exist_ok=True)
    logger.info(f"configs watching: {CONF_DIR}")
    async for changes in awatch(str(CONF_DIR)):
        now = monotonic() * 1000
        should_broadcast = False
        for _typ, path in changes:
            ts = recent_writes.get(str(path))
            if ts and (now - ts) < SELF_WIN_MS:
                # 自己刚写的，跳过
                continue
            _tname, changed = MANAGER.handle_file_change(Path(path))
            should_broadcast = should_broadcast or changed

        if should_broadcast:
            await broadcast({"type": "update_config", "data": MANAGER.to_payload()})
            logger.info("config updated & broadcast")


async def run_ws_server(host: str, port: int):
    """
    启动 WebSocket 服务与配置监听。常驻协程，取消即退出。
    """
    watcher = asyncio.create_task(_watch_configs())
    async with serve(ws_handler, host, port, max_size=2 ** 20):
        logger.info(f"Websocket listening on ws://{host}:{port}/")
        try:
            await asyncio.Future()  # run forever
        finally:
            watcher.cancel()
            with contextlib.suppress(Exception):
                await watcher
