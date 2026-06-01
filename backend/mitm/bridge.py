import asyncio
import contextlib
import os
import socket
from typing import Dict, Any, Callable, List, Optional

from loguru import logger
from mitmproxy.options import Options
from mitmproxy.tools.dump import DumpMaster

from backend.mitm.addon import WsAddon
from backend.mitm.codec import LiqiCodec


class MitmBridge:
    def __init__(self, port, liqi_json_path=None, upstream_proxy: str | None = None):
        self.host = "0.0.0.0"
        self.port = port
        self.upstream_proxy = self._normalize_upstream_proxy(upstream_proxy)
        self.codec = LiqiCodec(liqi_json_path)
        self.addon = WsAddon(self.codec)
        self._master: Optional[DumpMaster] = None
        self._listeners: List[Callable[[Dict[str, Any]], None]] = []

    @staticmethod
    def _normalize_upstream_proxy(upstream_proxy: str | None) -> str | None:
        proxy = (upstream_proxy or "").strip()
        if not proxy:
            return None
        if "://" not in proxy:
            proxy = f"http://{proxy}"
        return proxy

    async def start(self):
        try:
            mode = [f"upstream:{self.upstream_proxy}"] if self.upstream_proxy else ["regular"]
            opts = Options(listen_host=self.host, listen_port=self.port, ssl_insecure=True, mode=mode)
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

            upstream_suffix = f" upstream={self.upstream_proxy}" if self.upstream_proxy else ""
            logger.info(f"MitmBridge starting on {self.host}:{self.port}{upstream_suffix}")

            run_task = asyncio.create_task(self._run_master())
            try:
                await self._wait_until_listening(timeout=20.0)
                logger.info(f"SL_BACKEND_READY mitm={self.host}:{self.port}")
                await run_task
            except Exception:
                run_task.cancel()
                with contextlib.suppress(asyncio.CancelledError):
                    await run_task
                raise

            logger.info("MitmBridge run() finished normally")

        except asyncio.CancelledError:
            logger.info("MitmBridge.start cancelled")
            raise
        except Exception as e:
            logger.exception(f"MitmBridge.start() failed: {e}")
            raise

    async def _run_master(self):
        with open(os.devnull, "w") as devnull:
            with contextlib.redirect_stdout(devnull), contextlib.redirect_stderr(devnull):
                await self._master.run()

    async def _wait_until_listening(self, timeout: float = 20.0):
        deadline = asyncio.get_running_loop().time() + timeout
        probe_host = "127.0.0.1" if self.host in ("0.0.0.0", "::") else self.host

        while True:
            try:
                with socket.create_connection((probe_host, self.port), timeout=0.5):
                    return
            except OSError:
                if asyncio.get_running_loop().time() >= deadline:
                    raise TimeoutError(f"MITM listen timeout on {probe_host}:{self.port}")
                await asyncio.sleep(0.2)

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
