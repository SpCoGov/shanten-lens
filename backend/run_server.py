import argparse
import asyncio
from pathlib import Path

from backend.app import run_ws_server, set_data_root, MANAGER
from backend.mitm import MitmBridge, hooks


def parse_args():
    p = argparse.ArgumentParser()
    p.add_argument("--host", type=str, default="127.0.0.1")
    p.add_argument("--port", type=int, default=8787)
    p.add_argument("--data-root", type=str, help="自定义数据根目录（配置将落盘在 <root>/configs）")
    return p.parse_args()


async def main():
    args = parse_args()

    if args.data_root:
        set_data_root(Path(args.data_root))

    # 启动 MITM 桥（与 WS 并行）
    bridge = MitmBridge(MANAGER.get("backend.mitm_port", 10999))
    bridge.set_hooks(on_outbound=hooks.on_outbound, on_inbound=hooks.on_inbound)
    mitm_task = asyncio.create_task(bridge.start())

    try:
        # 常驻 WS 服务
        await run_ws_server(args.host, args.port)
    finally:
        # 优雅收尾
        mitm_task.cancel()
        try:
            await mitm_task
        except asyncio.CancelledError:
            pass


if __name__ == "__main__":
    asyncio.run(main())
