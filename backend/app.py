import argparse
import asyncio
import json
import os
import subprocess
import sys
from pathlib import Path
from time import monotonic
from typing import Set, Dict, Any

from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from platformdirs import user_data_dir
from watchfiles import awatch

from .config_models import ConfigManager

# ---------- 启动参数 ----------
parser = argparse.ArgumentParser()
parser.add_argument("--host", type=str, default="127.0.0.1")
parser.add_argument("--port", type=int, default=8787)
parser.add_argument("--data-root", type=str)
args, _ = parser.parse_known_args()


def default_data_root() -> Path:
    return Path(user_data_dir(appname="Shanten Lens", appauthor=None))


recent_writes: dict[str, float] = {}
SELF_WIN_MS = 500

DATA_ROOT = Path(args.data_root) if args.data_root else default_data_root()
CONF_DIR = DATA_ROOT / "configs"

# ---------- 默认配置表定义（示例，可以按需扩展/修改命名） ----------
DEFAULT_TABLES: Dict[str, Dict[str, Any]] = {
    "general": {
        "language": "zh-CN",
        "theme": "system"
    },
    "backend": {
        "host": "127.0.0.1",
        "port": 8787,
        "mitm_port": 10999,
        "max_workers": 2,
    }
}

# ---------- 全局状态 ----------
app = FastAPI()
app.add_middleware(CORSMiddleware, allow_origins=["*"], allow_credentials=False, allow_methods=["*"], allow_headers=["*"])

MANAGER = ConfigManager(CONF_DIR, DEFAULT_TABLES)
MANAGER.load_all()

CLIENTS: Set[WebSocket] = set()
CLIENTS_LOCK = asyncio.Lock()

# 可选：忽略自写变更的时间窗（毫秒），避免写盘后马上又被监听触发二次广播
last_self_write_ms = 0.0


# ---------- WS 只保留 {"type":"...","data":{}} ----------
@app.websocket("/ws")
async def ws_endpoint(ws: WebSocket):
    await ws.accept()
    async with CLIENTS_LOCK:
        CLIENTS.add(ws)

    # 刚连上就推送一次全量配置
    await _ws_send(ws, {"type": "update_config", "data": MANAGER.to_payload()})

    try:
        while True:
            raw = await ws.receive_text()
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

            elif t == "request_update":
                await _ws_send(ws, {"type": "update_config", "data": MANAGER.to_payload()})

            elif t == "open_config_dir":
                try:
                    _open_dir(str(CONF_DIR))
                    await _ws_send(ws, {"type": "open_result", "data": {"ok": True}})
                except Exception as e:
                    await _ws_send(ws, {"type": "open_result", "data": {"ok": False, "error": str(e)}})

    except WebSocketDisconnect:
        pass
    finally:
        async with CLIENTS_LOCK:
            CLIENTS.discard(ws)


def _open_dir(path: str):
    if sys.platform.startswith("win"):
        os.startfile(path)  # type: ignore
    elif sys.platform == "darwin":
        subprocess.Popen(["open", path])
    else:
        subprocess.Popen(["xdg-open", path])


async def _ws_send(ws: WebSocket, pkt: Dict[str, Any]):
    try:
        await ws.send_text(json.dumps(pkt, ensure_ascii=False))
    except Exception:
        pass


async def _broadcast(pkt: Dict[str, Any]):
    to_remove = []
    async with CLIENTS_LOCK:
        for c in list(CLIENTS):
            try:
                await c.send_text(json.dumps(pkt, ensure_ascii=False))
            except Exception:
                to_remove.append(c)
        for c in to_remove:
            CLIENTS.discard(c)


# ---------- 监听配置目录：文件被外部改动时，广播全量 ----------
async def _watch_configs():
    CONF_DIR.mkdir(parents=True, exist_ok=True)
    print(f"[configs] watching: {CONF_DIR}")
    async for changes in awatch(str(CONF_DIR)):
        now = monotonic() * 1000
        should_broadcast = False
        for _typ, path in changes:
            # 忽略自写窗口内的事件
            ts = recent_writes.get(str(path))
            if ts and (now - ts) < SELF_WIN_MS:
                continue
            _tname, changed = MANAGER.handle_file_change(Path(path))
            should_broadcast = should_broadcast or changed
        if should_broadcast:
            await _broadcast({"type": "update_config", "data": MANAGER.to_payload()})
            print("[configs] updated & broadcast")


@app.on_event("startup")
async def _on_startup():
    app.state._watch_task = asyncio.create_task(_watch_configs())


@app.on_event("shutdown")
async def _on_shutdown():
    t = getattr(app.state, "_watch_task", None)
    if t:
        t.cancel()
        try:
            await t
        except Exception:
            pass
