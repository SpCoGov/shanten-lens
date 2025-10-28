import json
import threading
from typing import Callable, Tuple, Any, Dict, List, Optional
import asyncio

from loguru import logger
from mitmproxy import http, ctx

import backend.app
from backend.mitm.codec import LiqiCodec

HookFn = Callable[[Dict], Tuple[str, Any]]

ignore_methods = [
    '.lq.Lobby.oauth2Login',
    '.lq.Route.heartbeat',
    '.lq.Lobby.prepareLogin',
    '.lq.Route.requestConnection',
    '.lq.Lobby.fetchServerTime',
    '.lq.Lobby.loginSuccess',
    '.lq.Lobby.loginBeat',
]


def _peer_key_ws(flow: http.HTTPFlow) -> str:
    try:
        cip = flow.client_conn.address[0]
        sip = flow.server_conn.address[0]
        return f"{cip}|{sip}"
    except Exception:
        return "n/a"


class WsAddon:
    def __init__(self, codec: LiqiCodec):
        self.codec = codec
        self.on_outbound: Optional[HookFn] = None
        self.on_inbound: Optional[HookFn] = None
        self.subscribers: List[Callable[[Dict], None]] = []
        self._flows: Dict[str, http.HTTPFlow] = {}  # peer_key -> flow
        self.last_flow: Optional[http.HTTPFlow] = None  # 最近一次触达的 flow
        self._client_last_req_id: dict[int, int] = {}  # flow_id -> last req id
        self._waiters: Dict[int, asyncio.Future] = {}
        self._waiters_sync: dict[int, dict] = {}
        self._waiters_lock = threading.Lock()
        self.master = None
        self.preferred_flow: Optional[http.HTTPFlow] = None
        self.preferred_peer_key: Optional[str] = None

        global WS_ADDON_INSTANCE
        WS_ADDON_INSTANCE = self

    def set_master(self, master):
        self.master = master

    def register_waiter_sync(self, msg_id: int):
        ev = threading.Event()
        with self._waiters_lock:
            self._waiters_sync[msg_id] = {"ev": ev, "resp": None}
        return ev

    def resolve_waiter_sync(self, msg_id: int, resp: dict):
        with self._waiters_lock:
            item = self._waiters_sync.get(msg_id)
            if item is None:
                return
            item["resp"] = resp
            item["ev"].set()

    def pop_waiter_sync_resp(self, msg_id: int) -> dict:
        with self._waiters_lock:
            item = self._waiters_sync.pop(msg_id, None)
            return None if item is None else item.get("resp")

    def discard_waiter_sync(self, msg_id: int):
        with self._waiters_lock:
            self._waiters_sync.pop(msg_id, None)

    def subscribe(self, cb: Callable[[Dict], None]):
        self.subscribers.append(cb)

    @staticmethod
    def _apply(hook: Optional[HookFn], view: Dict):
        if not hook:
            return "pass", None
        try:
            return hook(view)
        except Exception as e:
            logger.exception(
                "hook 执行异常 (hook={}, view_keys={})",
                getattr(hook, "__name__", repr(hook)),
                view.keys(),
            )
            return "pass", None

    def websocket_message(self, flow: http.HTTPFlow):
        if not flow.websocket:
            return

        peer_key = f"{flow.client_conn.address[0]}|{flow.server_conn.address[0]}"
        self._flows[peer_key] = flow
        self.last_flow = flow

        message = flow.websocket.messages[-1]

        try:
            view = self.codec.parse_frame(message.content, message.from_client)
        except Exception as e:
            logger.error(
                f"parse error for ws message from {flow.client_conn.address} -> "
                f"{flow.server_conn.address}: {e}"
            )
            return

        try:
            if (not message.from_client) and view.get("method") in [".lq.Lobby.fetchAmuletActivityData", ".lq.Lobby.fetchActivityRank", ".lq.Lobby.fetchAccountStatisticInfo"]:
                self.preferred_flow = flow
                self.preferred_peer_key = f"{flow.client_conn.address[0]}|{flow.server_conn.address[0]}"
                logger.info(f"[PREFERRED-FLOW] set to game flow f={id(flow)} ({self.preferred_peer_key})")

        except Exception:
            pass

        # 记录客户端最近一次 Req 的 id，供后续注入生成 id 参考
        try:
            if message.from_client and view.get("type") == "Req" and isinstance(view.get("id"), int):
                self._client_last_req_id[id(flow)] = view["id"]
        except Exception:
            pass

        for cb in self.subscribers:
            try:
                cb(view)
            except Exception as e:
                logger.error(f"subscriber error: {e}")

        try:
            if backend.app.MANAGER.get("general.debug"):
                logger.debug(f"{'已发送' if message.from_client else '接收到'}：{view.get('method')} (id={view.get('id')})")
                import json
                pretty = json.dumps(view.get('data'), ensure_ascii=False)
                cur_key = _peer_key_ws(flow)
                pf = self.preferred_flow
                pf_key = _peer_key_ws(pf) if pf else None
                on_pref = (pf is not None and pf is flow)
                logger.debug(
                    "== FULL MESSAGE BEGIN ==\n"
                    f"method: {view.get('method')}\n"
                    f"from_client: {view.get('from_client')}\n"
                    f"msg_id: {view.get('id')}\n"
                    f"parsed:\n{pretty}\n"
                    f"cur_f={id(flow)} cur_key={cur_key} pref_f={id(pf) if pf else None} pref_key={pf_key or 'None'} on_pref={on_pref}\n"
                    "== FULL MESSAGE END =="
                )
                if view.get('method') not in ignore_methods:
                    ...
        except Exception as e:
            logger.error(f"logging full message failed: {e}")

        hook = self.on_outbound if message.from_client else self.on_inbound
        action, payload = self._apply(hook, view)

        # 如果要 drop，先尝试唤醒 waiter 再返回，避免调用侧超时
        if action == "drop":
            if (not message.from_client) and view.get("type") in ("Res", "Notify") and isinstance(view.get("id"), int):
                try:
                    self.resolve_waiter_sync(int(view["id"]), view)
                except Exception:
                    pass
            message.drop()
            logger.success(f"{'已发送' if message.from_client else '接收到'}(drop)：{view.get('method')}")
            return

        if action == "modify" and payload is not None:
            new_view = dict(view, data=payload)
            try:
                message.content = self.codec.build_frame(new_view)
                view = new_view
                logger.success(f"{'已发送' if message.from_client else '接收到'}(modify)：{new_view.get('method')}")
            except Exception as e:
                logger.error(f"修改后重建失败：{e}")

        if action == "inject" and payload:
            for inj in payload:
                try:
                    inj_bytes = self.codec.build_frame(inj)
                    to_client = (inj["type"] in ("Notify", "Res"))
                    ctx.master.commands.call("inject.websocket", flow, to_client, inj_bytes, False)
                    logger.success(f"已注入：{inj.get('method')} -> {'client' if to_client else 'server'}")
                except Exception as e:
                    logger.error(f"注入失败：{e}")
        try:
            if (not message.from_client) and view.get("type") in ("Res", "Notify") and isinstance(view.get("id"), int):
                self.resolve_waiter_sync(int(view["id"]), view)
        except Exception:
            pass

    def _pick_flow(self, peer_key: Optional[str]):
        if peer_key:
            return self._flows.get(peer_key)
        return self.last_flow

    def websocket_end(self, flow: http.HTTPFlow):
        # 連線正常關閉
        try:
            peer_key = f"{flow.client_conn.address[0]}|{flow.server_conn.address[0]}"
        except Exception:
            peer_key = None

        if peer_key and peer_key in self._flows:
            self._flows.pop(peer_key, None)

        if getattr(self, "preferred_flow", None) is flow:
            self.preferred_flow = None
            self.preferred_peer_key = None
            logger.info(f"[PREFERRED-FLOW] closed -> set to None (f={id(flow)})")

        if getattr(self, "last_flow", None) is flow:
            self.last_flow = None

    def websocket_error(self, flow: http.HTTPFlow):
        self.websocket_end(flow)

    def inject_now(
            self, *,
            method: str,
            data: dict,
            t: str = "Req",
            peer_key: Optional[str] = None,
            force_id: Optional[int] = None
    ) -> tuple[bool, str, int]:
        """
        立即注入一条帧到当前/指定 flow：
          - t: "Req" | "Res" | "Notify"
          - method: 例如 ".lq.Lobby.amuletActivitySelectPack"
          - data:   Protobuf 对应的 dict（由 codec.build_frame 负责序列化）
          - peer_key: "clientIP|serverHost"；不传则用最近活跃 flow
          - force_id: 可选，强制使用某个 msg_id（不传则用 last_req_id+偏移）
        返回: (ok: bool, detail: str, msg_id: int|-1)
        """
        flow = self._pick_flow(peer_key)
        if not flow or not getattr(flow, "websocket", None):
            return False, "no-active-websocket-flow", -1

        inj = {"type": t, "method": method, "data": data}
        msg_id = -1

        if t in ("Req", "Res"):
            if force_id is not None:
                msg_id = int(force_id) & 0xFFFF
            else:
                base = self._client_last_req_id.get(
                    id(flow),
                    getattr(self.codec, "_last_req_id", 0) or 0
                )
                candidate = (int(base) - 1) & 0xFFFF
                busy = getattr(self.codec, "_res_map", {})
                tries = 0
                # 尽量避开当前“忙碌”的 id
                while candidate in busy and tries < 16:
                    candidate = (candidate - 1) & 0xFFFF
                    tries += 1
                msg_id = candidate
            inj["id"] = msg_id

        try:
            inj_bytes = self.codec.build_frame(inj)
        except Exception as e:
            return False, f"build-frame-failed: {e}", -1

        try:
            # 对“请求”必须用 from_client=True 才会登记 _res_map
            self.codec.parse_frame(inj_bytes, from_client=(t == "Req"))
        except Exception:
            pass

        master = self.master
        if not master or not getattr(master, "event_loop", None):
            return False, "inject-failed:no-master-loop", -1

        loop = master.event_loop

        def _do_inject():
            try:
                to_client = (t in ("Notify", "Res"))
                master.commands.call("inject.websocket", flow, to_client, inj_bytes, False)
            except Exception as e:
                # 这里无法把异常直接抛回调用方，但至少能在日志里看到
                from loguru import logger
                logger.error(f"inject command failed inside mitm loop: {e}")

        try:
            loop.call_soon_threadsafe(_do_inject)
            return True, "ok", msg_id
        except Exception as e:
            return False, f"inject-failed:{e}", -1

    _MAX_LOG_BODY = 64 * 1024  # 64 KB

    _REDACT_HEADERS = {"authorization", "cookie", "set-cookie", "proxy-authorization"}

    def _short_addr(self, flow: http.HTTPFlow) -> str:
        try:
            cip = flow.client_conn.address[0]
            sip = flow.server_conn.address[0]
            return f"{cip} -> {sip}"
        except Exception:
            return "n/a"

    def _maybe_redact_headers(self, headers: http.Headers) -> dict:
        out = {}
        for k, v in headers.items(multi=True):
            key_lower = k.lower()
            if key_lower in self._REDACT_HEADERS:
                out[k] = "<redacted>"
            else:
                out[k] = v
        return out

    def _pretty_body(self, content: bytes, content_type: str | None) -> str:
        if not content:
            return ""
        body = content[: self._MAX_LOG_BODY]
        # 先看 content-type，再看首字符猜测
        ct = (content_type or "").lower()
        looks_json = ("json" in ct) or (body[:1] in (b"{", b"["))
        try:
            text = body.decode("utf-8", errors="replace")
        except Exception:
            return f"<{len(body)} bytes binary>"
        if looks_json:
            try:
                return json.dumps(json.loads(text), ensure_ascii=False, indent=2)
            except Exception:
                pass
        return text

    def request(self, flow: http.HTTPFlow):
        # 记录最近的 flow，便于后续注入时拾取
        peer_key = f"{flow.client_conn.address[0]}|{flow.request.host}"
        self._flows[peer_key] = flow
        self.last_flow = flow

        if not backend.app.MANAGER.get("general.debug"):
            return

        try:
            req = flow.request
            headers = self._maybe_redact_headers(req.headers)
            body_text = self._pretty_body(req.raw_content or b"", req.headers.get("content-type"))

            logger.debug(
                "== HTTP REQUEST BEGIN ==\n"
                f"{self._short_addr(flow)}\n"
                f"{req.method} {req.url}\n"
                f"HTTP/{req.http_version}\n"
                f"Headers: {json.dumps(headers, ensure_ascii=False)}\n"
                f"Body({len(req.raw_content or b'')} bytes, shown up to {self._MAX_LOG_BODY}):\n"
                f"{body_text}\n"
                "== HTTP REQUEST END =="
            )
        except Exception as e:
            logger.error(f"http request log failed: {e}")

    # 打印 HTTP 响应
    def response(self, flow: http.HTTPFlow):
        if not backend.app.MANAGER.get("general.debug"):
            return
        try:
            resp = flow.response
            headers = self._maybe_redact_headers(resp.headers)
            body_text = self._pretty_body(resp.raw_content or b"", resp.headers.get("content-type"))

            logger.debug(
                "== HTTP RESPONSE BEGIN ==\n"
                f"{self._short_addr(flow)}\n"
                f"HTTP/{resp.http_version} {resp.status_code} {resp.reason}\n"
                f"From: {flow.request.method} {flow.request.url}\n"
                f"Headers: {json.dumps(headers, ensure_ascii=False)}\n"
                f"Body({len(resp.raw_content or b'')} bytes, shown up to {self._MAX_LOG_BODY}):\n"
                f"{body_text}\n"
                "== HTTP RESPONSE END =="
            )
        except Exception as e:
            logger.error(f"http response log failed: {e}")


WS_ADDON_INSTANCE: Optional[WsAddon] = None
