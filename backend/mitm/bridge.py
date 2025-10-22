import asyncio
from typing import Dict, Any, Callable, List, Optional

from loguru import logger
from mitmproxy.options import Options
from mitmproxy.tools.dump import DumpMaster

from backend.mitm.addon import WsAddon
from backend.mitm.codec import LiqiCodec


class MitmBridge:
    def __init__(self, port, liqi_json_path=None):
        self.host = "0.0.0.0"
        self.port = port
        self.codec = LiqiCodec(liqi_json_path)
        self.addon = WsAddon(self.codec)
        self._master: Optional[DumpMaster] = None
        self._listeners: List[Callable[[Dict[str, Any]], None]] = []

    async def start(self):
        try:
            opts = Options(listen_host=self.host, listen_port=self.port, ssl_insecure=True)
            self._master = DumpMaster(opts)
            try:
                self.addon.set_master(self._master)
            except Exception:
                pass

            try:
                self._master.options.termlog_verbosity = "error"
            except Exception:
                pass
            try:
                self._master.options.flow_detail = 0
            except Exception:
                pass

            self._master.addons.add(self.addon)
            self.addon.subscribe(self._emit)

            try:
                for a in list(getattr(self._master.addons, "addons", [])):
                    n = a.__class__.__name__.lower()
                    if n in ("termlog", "eventlog"):
                        self._master.addons.remove(a)
            except Exception:
                pass

            import logging as _pylog
            for name in ("mitmproxy", "mitmproxy.proxy", "mitmproxy.tools.dump",
                         "mitmproxy.addons.proxyserver", "mitmproxy.net.http", "mitmproxy.net.tcp"):
                try:
                    _pylog.getLogger(name).setLevel(_pylog.ERROR)
                except Exception:
                    pass

            logger.info(f"MitmBridge starting on {self.host}:{self.port}")

            import contextlib, os
            with open(os.devnull, "w") as devnull:
                with contextlib.redirect_stdout(devnull), contextlib.redirect_stderr(devnull):
                    await self._master.run()

            logger.info("MitmBridge run() finished normally")

        except asyncio.CancelledError:
            logger.info("MitmBridge.start cancelled")
            raise
        except Exception as e:
            logger.exception(f"MitmBridge.start() failed: {e}")
            raise

    def _emit(self, event: Dict[str, Any]):
        for fn in list(self._listeners):
            try:
                fn(event)
            except Exception as e:
                logger.error(f"listener error: {e}")

    def on_event(self, fn: Callable[[Dict[str, Any]], None]):
        self._listeners.append(fn)

    def set_hooks(self, on_outbound=None, on_inbound=None):
        self.addon.on_outbound = on_outbound
        self.addon.on_inbound = on_inbound

    def build(self, view_like: Dict[str, Any]) -> bytes:
        return self.codec.build_frame(view_like)
