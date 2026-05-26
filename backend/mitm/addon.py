import json
import queue
import threading
from typing import Callable, Tuple, Any, Dict, List, Optional
import asyncio

from loguru import logger
from mitmproxy import http, ctx

import backend.app
from backend.mitm.codec import LiqiCodec
from backend.packet_monitor import PACKET_MONITOR

HookFn = Callable[[Dict], Tuple[str, Any]]

ignore_methods = [
    '.lq.Lobby.oauth2Login',
    '.lq.Route.heartbeat',
    '.lq.Lobby.prepareLogin',
    '.lq.Route.requestConnection',
    '.lq.Lobby.fetchServerTime',
    '.lq.Lobby.loginSuccess',
    '.lq.Lobby.loginBeat',
    '.lq.Lobby.fetchAccountStatisticInfo',
    '.lq.Lobby.fetchAccountInfo',
    '.lq.Lobby.fetchCommentList',
    '.lq.Lobby.fetchAccountChallengeRankInfo',
    '.lq.Lobby.fetchAccountInfoExtra',
    '.lq.Lobby.amuletActivityFetchBrief'
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
        self._flow_packet_stats: dict[int, dict[str, Any]] = {}
        self.last_flow: Optional[http.HTTPFlow] = None  # 最近一次触达的 flow
        self._client_last_req_id: dict[int, int] = {}  # flow_id -> last req id
        self._waiters: Dict[int, asyncio.Future] = {}
        self._waiters_sync: dict[int, dict] = {}
        self._waiters_lock = threading.Lock()
        self.master = None
        self.preferred_flow: Optional[http.HTTPFlow] = None
        self.preferred_peer_key: Optional[str] = None
        self._debug_log_queue: "queue.SimpleQueue[Optional[dict[str, Any]]]" = queue.SimpleQueue()
        self._debug_log_worker = threading.Thread(
            target=self._debug_log_pump,
            name="WsAddonDebugLog",
            daemon=True,
        )
        self._debug_log_worker.start()

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

    def _debug_log_pump(self):
        while True:
            item = self._debug_log_queue.get()
            if item is None:
                return
            try:
                pretty = json.dumps(item.get("data"), ensure_ascii=False)
                logger.debug(
                    "== FULL MESSAGE BEGIN ==\n"
                    f"method: {item.get('method')}\n"
                    f"from_client: {item.get('from_client')}\n"
                    f"msg_id: {item.get('msg_id')}\n"
                    f"parsed:\n{pretty}\n"
                    f"cur_f={item.get('flow_id')} cur_key={item.get('cur_key')} "
                    f"pref_f={item.get('pref_flow_id')} pref_key={item.get('pref_key')} "
                    f"on_pref={item.get('on_pref')}\n"
                    "== FULL MESSAGE END =="
                )
            except Exception as e:
                logger.error(f"logging full message failed: {e}")

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

        self._record_flow_packet(flow, view, message.from_client)

        try:
            if (not message.from_client) and view.get("method") in [".lq.Lobby.loginBeat"]:
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
            method = str(view.get("method") or "")
            if backend.app.should_emit_packet_monitor(method):
                packet = PACKET_MONITOR.append(view)
                backend.app.post_broadcast({"type": "packet_monitor_event", "data": packet})
        except Exception as e:
            logger.error(f"packet monitor append failed: {e}")

        try:
            if backend.app.MANAGER.get("general.debug"):
                if view.get('method') not in ignore_methods:
                    logger.debug(f"{'已发送' if message.from_client else '接收到'}：{view.get('method')} (id={view.get('id')})")
                    cur_key = _peer_key_ws(flow)
                    pf = self.preferred_flow
                    pf_key = _peer_key_ws(pf) if pf else None
                    on_pref = (pf is not None and pf is flow)
                    self._debug_log_queue.put({
                        "method": view.get("method"),
                        "from_client": view.get("from_client"),
                        "msg_id": view.get("id"),
                        "data": view.get("data"),
                        "flow_id": id(flow),
                        "cur_key": cur_key,
                        "pref_flow_id": id(pf) if pf else None,
                        "pref_key": pf_key or "None",
                        "on_pref": on_pref,
                    })
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

    def _record_flow_packet(self, flow: http.HTTPFlow, view: dict[str, Any], from_client: bool) -> None:
        flow_id = id(flow)
        packet_type = str(view.get("type") or "Unknown")
        method = str(view.get("method") or "")
        direction = "client_to_server" if from_client else "server_to_client"
        stats = self._flow_packet_stats.setdefault(
            flow_id,
            {
                "total": 0,
                "by_type": {},
                "by_direction": {},
                "by_method": {},
                "by_type_method": {},
            },
        )
        stats["total"] += 1
        stats["by_type"][packet_type] = int(stats["by_type"].get(packet_type, 0)) + 1
        stats["by_direction"][direction] = int(stats["by_direction"].get(direction, 0)) + 1
        if method:
            stats["by_method"][method] = int(stats["by_method"].get(method, 0)) + 1
            type_methods = stats["by_type_method"].setdefault(packet_type, {})
            type_methods[method] = int(type_methods.get(method, 0)) + 1

    def _flow_summary(self, flow: http.HTTPFlow, peer_key: Optional[str] = None) -> dict[str, Any]:
        ws = getattr(flow, "websocket", None)
        stats = self._flow_packet_stats.get(id(flow)) or {}
        try:
            client = f"{flow.client_conn.address[0]}:{flow.client_conn.address[1]}"
        except Exception:
            client = "n/a"
        try:
            server = f"{flow.server_conn.address[0]}:{flow.server_conn.address[1]}"
        except Exception:
            server = "n/a"
        try:
            request = flow.request
            request_line = f"{request.method} {request.pretty_url}"
        except Exception:
            request_line = "n/a"
        try:
            messages = len(ws.messages) if ws else 0
        except Exception:
            messages = 0
        return {
            "id": id(flow),
            "peer_key": peer_key or _peer_key_ws(flow),
            "client": client,
            "server": server,
            "request": request_line,
            "websocket": bool(ws),
            "messages": messages,
            "is_preferred": flow is self.preferred_flow,
            "is_last": flow is self.last_flow,
            "packet_stats": {
                "total": int(stats.get("total") or 0),
                "by_type": dict(stats.get("by_type") or {}),
                "by_direction": dict(stats.get("by_direction") or {}),
                "by_method": dict(stats.get("by_method") or {}),
                "by_type_method": {
                    str(packet_type): dict(methods or {})
                    for packet_type, methods in dict(stats.get("by_type_method") or {}).items()
                },
            },
            "exclusive_packets": [],
        }

    def _attach_exclusive_packets(self, rows: list[dict[str, Any]]) -> None:
        packet_owner_count: dict[tuple[str, str], int] = {}
        for row in rows:
            seen_in_flow: set[tuple[str, str]] = set()
            by_type_method = row.get("packet_stats", {}).get("by_type_method", {})
            for packet_type, methods in dict(by_type_method or {}).items():
                for method in dict(methods or {}).keys():
                    if method:
                        seen_in_flow.add((str(packet_type), str(method)))
            for key in seen_in_flow:
                packet_owner_count[key] = packet_owner_count.get(key, 0) + 1

        for row in rows:
            exclusive_packets: list[dict[str, Any]] = []
            by_type_method = row.get("packet_stats", {}).get("by_type_method", {})
            for packet_type, methods in dict(by_type_method or {}).items():
                for method, count in dict(methods or {}).items():
                    key = (str(packet_type), str(method))
                    if packet_owner_count.get(key) == 1:
                        exclusive_packets.append({
                            "packet_type": key[0],
                            "method": key[1],
                            "count": int(count),
                        })
            exclusive_packets.sort(key=lambda item: (-item["count"], item["packet_type"], item["method"]))
            row["exclusive_packets"] = exclusive_packets

    def dump_current_flows(self) -> list[dict[str, Any]]:
        seen: set[int] = set()
        rows: list[dict[str, Any]] = []

        for peer_key, flow in list(self._flows.items()):
            if not flow or id(flow) in seen:
                continue
            seen.add(id(flow))
            rows.append(self._flow_summary(flow, peer_key))

        for flow in (self.preferred_flow, self.last_flow):
            if not flow or id(flow) in seen:
                continue
            seen.add(id(flow))
            rows.append(self._flow_summary(flow))

        self._attach_exclusive_packets(rows)

        logger.info("== CURRENT MITM FLOWS BEGIN ==")
        if not rows:
            logger.info("no active websocket flows recorded")
        else:
            for idx, row in enumerate(rows, 1):
                flags = ",".join(
                    flag
                    for flag, active in (
                        ("preferred", row["is_preferred"]),
                        ("last", row["is_last"]),
                    )
                    if active
                ) or "-"
                logger.info(
                    "[flow {idx}] id={id} peer={peer} client={client} server={server} "
                    "ws={ws} messages={messages} packets={packets} by_type={by_type} "
                    "flags={flags} request={request}",
                    idx=idx,
                    id=row["id"],
                    peer=row["peer_key"],
                    client=row["client"],
                    server=row["server"],
                    ws=row["websocket"],
                    messages=row["messages"],
                    packets=row["packet_stats"]["total"],
                    by_type=json.dumps(row["packet_stats"]["by_type"], ensure_ascii=False),
                    flags=flags,
                    request=row["request"],
                )
                by_type_method = row["packet_stats"]["by_type_method"]
                if by_type_method:
                    logger.info(
                        "[flow {idx}] packet methods by type: {stats}",
                        idx=idx,
                        stats=json.dumps(by_type_method, ensure_ascii=False),
                    )
                exclusive_packets = row.get("exclusive_packets") or []
                logger.info(
                    "[flow {idx}] exclusive packets: {packets}",
                    idx=idx,
                    packets=json.dumps(exclusive_packets, ensure_ascii=False),
                )
        logger.info("== CURRENT MITM FLOWS END ==")
        return rows

    def websocket_end(self, flow: http.HTTPFlow):
        # 連線正常關閉
        try:
            peer_key = f"{flow.client_conn.address[0]}|{flow.server_conn.address[0]}"
        except Exception:
            peer_key = None

        if peer_key and peer_key in self._flows:
            self._flows.pop(peer_key, None)

        self._flow_packet_stats.pop(id(flow), None)

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

        def _ctx(extra: str = "") -> str:
            pf = self.preferred_flow
            try:
                cip = pf.client_conn.address[0] if pf else None
                sip = pf.server_conn.address[0] if pf else None
                pf_key = f"{cip}|{sip}" if (cip and sip) else None
            except Exception:
                pf_key = None
            return ((f"[inject ctx] method={method!r} t={t!r} "
                     f"force_id={force_id!r} pf_id={id(pf) if pf else None} "
                     f"pf_key={pf_key or 'None'} {extra} "
                     f"method={method!r} data={data!r}"
                     ).strip())

        flow = self.preferred_flow
        if not flow or not getattr(flow, "websocket", None):
            logger.error(f"inject_now: no preferred websocket flow. {_ctx()}")
            return False, "no-preferred-websocket-flow", -1

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
            logger.exception("build-frame-failed")
            logger.exception(f"inject_now: build-frame-failed. {_ctx()}")
            return False, f"build-frame-failed: {e}", -1

        try:
            # 对“请求”必须用 from_client=True 才会登记 _res_map
            self.codec.parse_frame(inj_bytes, from_client=(t == "Req"))
        except Exception:
            pass

        master = self.master
        if not master or not getattr(master, "event_loop", None):
            logger.error(f"inject_now: no master loop. {_ctx()}")
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
            logger.error(f"inject_now: call_soon_threadsafe failed: {e}. {_ctx()}")
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

    # def request(self, flow: http.HTTPFlow):
    #     # 记录最近的 flow，便于后续注入时拾取
    #     peer_key = f"{flow.client_conn.address[0]}|{flow.request.host}"
    #     self._flows[peer_key] = flow
    #     self.last_flow = flow
    #
    #     if not backend.app.MANAGER.get("general.debug"):
    #         return
    #
    #     try:
    #         req = flow.request
    #         headers = self._maybe_redact_headers(req.headers)
    #         body_text = self._pretty_body(req.raw_content or b"", req.headers.get("content-type"))
    #
    #         logger.debug(
    #             "== HTTP REQUEST BEGIN ==\n"
    #             f"{self._short_addr(flow)}\n"
    #             f"{req.method} {req.url}\n"
    #             f"HTTP/{req.http_version}\n"
    #             f"Headers: {json.dumps(headers, ensure_ascii=False)}\n"
    #             f"Body({len(req.raw_content or b'')} bytes, shown up to {self._MAX_LOG_BODY}):\n"
    #             f"{body_text}\n"
    #             "== HTTP REQUEST END =="
    #         )
    #     except Exception as e:
    #         logger.error(f"http request log failed: {e}")
    #
    # # 打印 HTTP 响应
    # def response(self, flow: http.HTTPFlow):
    #     if not backend.app.MANAGER.get("general.debug"):
    #         return
    #     try:
    #         resp = flow.response
    #         headers = self._maybe_redact_headers(resp.headers)
    #         body_text = self._pretty_body(resp.raw_content or b"", resp.headers.get("content-type"))
    #
    #         logger.debug(
    #             "== HTTP RESPONSE BEGIN ==\n"
    #             f"{self._short_addr(flow)}\n"
    #             f"HTTP/{resp.http_version} {resp.status_code} {resp.reason}\n"
    #             f"From: {flow.request.method} {flow.request.url}\n"
    #             f"Headers: {json.dumps(headers, ensure_ascii=False)}\n"
    #             f"Body({len(resp.raw_content or b'')} bytes, shown up to {self._MAX_LOG_BODY}):\n"
    #             f"{body_text}\n"
    #             "== HTTP RESPONSE END =="
    #         )
    #     except Exception as e:
    #         logger.error(f"http response log failed: {e}")


WS_ADDON_INSTANCE: Optional[WsAddon] = None
