from __future__ import annotations
import argparse
import asyncio
from pathlib import Path
from loguru import logger

from backend.app import set_data_root, MANAGER, GAME_STATE, start_ui_services
from backend.bot.drivers.packet.packet_bot import PacketBot
from backend.mitm import MitmBridge, hooks


def parse_args():
    p = argparse.ArgumentParser()
    p.add_argument("--host", type=str, default="127.0.0.1")
    p.add_argument("--port", type=int, default=8787)
    p.add_argument("--data-root", type=str, help="自定义数据根目录")
    return p.parse_args()


async def main():
    args = parse_args()
    if args.data_root:
        set_data_root(Path(args.data_root))

    bridge = MitmBridge(MANAGER.get("backend.mitm_port", 10999))
    bridge.set_hooks(on_outbound=hooks.on_outbound, on_inbound=hooks.on_inbound)

    from backend import app as _app
    _app.PACKET_BOT = PacketBot(
        addon_getter=lambda: bridge.addon,  # 直接闭包引用，不用 globals hack
        activity_id=250811,
        state_getter=lambda: GAME_STATE,
    )

    start_ui_services(
        host="127.0.0.1",
        ws_port=int(MANAGER.get("ws_port", 8787)),
    )
    logger.info("UI services started (ws/http/watchers on UI loop).")

    try:
        await bridge.start()
    except asyncio.CancelledError:
        pass
    finally:
        logger.info("MITM stopped. Bye.")


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        logger.info("Interrupted. Bye.")
