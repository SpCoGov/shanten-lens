from typing import Callable, Tuple, Any, Dict, List, Optional
from mitmproxy import http, ctx
from loguru import logger

import backend.app
from backend.mitm.codec import LiqiCodec

HookFn = Callable[[Dict], Tuple[str, Any]]

WS_ADDON_INSTANCE = None


ignore_methods = [
    '.lq.Lobby.oauth2Login',
    '.lq.Route.heartbeat',
    '.lq.Lobby.prepareLogin',
    '.lq.Route.requestConnection',
    '.lq.Lobby.fetchServerTime',
    '.lq.Lobby.loginSuccess',
    '.lq.Lobby.loginBeat',

]


class WsAddon:
    def __init__(self, codec: LiqiCodec):
        self.codec = codec
        self.on_outbound: Optional[HookFn] = None
        self.on_inbound: Optional[HookFn] = None
        self.subscribers: List[Callable[[Dict], None]] = []
        self._flows = {}  # peer_key -> flow
        self._last_flow = None  # 最近一次触达的 flow

        global WS_ADDON_INSTANCE
        WS_ADDON_INSTANCE = self

    def subscribe(self, cb: Callable[[Dict], None]):
        self.subscribers.append(cb)

    def websocket_message(self, flow: http.HTTPFlow):
        # 只处理真正的 websocket 流量（mitmproxy 可能还会收到普通 HTTP）
        if not flow.websocket:
            return

        peer_key = f"{flow.client_conn.address[0]}|{flow.server_conn.address[0]}"
        self._flows[peer_key] = flow
        self._last_flow = flow

        # 取最后一条消息（mitmproxy 把每条 ws 消息追加到 flow.websocket.messages）
        message = flow.websocket.messages[-1]

        # 解析帧
        try:
            view = self.codec.parse_frame(message.content, message.from_client)
        except Exception as e:
            # 如果解析失败，按需记录错误（不要把太多原始数据直接打印到控制台，以免太长）
            logger.error(f"parse error for ws message from {flow.client_conn.address} -> {flow.server_conn.address}: {e}")
            return

        # 将解析后的事件广播给订阅者
        for cb in self.subscribers:
            try:
                cb(view)
            except Exception as e:
                logger.error(f"subscriber error: {e}")

        try:
            if view['method'] not in ignore_methods:
                if backend.app.MANAGER.get("general.debug"):
                    # 简洁信息（info）
                    logger.info(f"{'已发送' if message.from_client else '接收到'}：{view['method']} (id={view['id']})")

                    # 详细内容（debug）——包括序列化后的 JSON 和原始二进制（base64）
                    import json, base64
                    pretty = json.dumps(view['data'], ensure_ascii=False, indent=2)
                    raw_b64 = base64.b64encode(view['raw']).decode()

                    logger.debug(f"== FULL MESSAGE BEGIN ==\nmethod: {view['method']}\nfrom_client: {view['from_client']}\nmsg_id: {view['id']}\nparsed:\n{pretty}\nraw_base64:\n{raw_b64}\n== FULL MESSAGE END ==")
        except Exception as e:
            logger.error(f"logging full message failed: {e}")

        # 调用 hook 执行修改 / 丢弃 / 注入 等（保持原逻辑）
        hook = self.on_outbound if message.from_client else self.on_inbound
        action, payload = self._apply(hook, view)

        if action == "drop":
            message.drop()
            logger.success(f"{'已发送' if message.from_client else '接收到'}(drop)：{view['method']}")
            return

        if action == "modify" and payload is not None:
            new_view = dict(view, data=payload)
            try:
                message.content = self.codec.build_frame(new_view)
                logger.success(f"{'已发送' if message.from_client else '接收到'}(modify)：{new_view['method']}")
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

    @staticmethod
    def _apply(hook: Optional[HookFn], view: Dict):
        if not hook: return "pass", None
        try:
            return hook(view)
        except Exception as e:
            logger.exception(
                "hook 执行异常 (hook={}, view_keys={})",
                getattr(hook, "__name__", repr(hook)),
                view.keys(),
            )
            return "pass", None

    def _pick_flow(self, peer_key: Optional[str]):
        if peer_key:
            return self._flows.get(peer_key)
        return self._last_flow

    def inject_now(self, *, method: str, data: dict, t: str = "Req",
                   peer_key: Optional[str] = None, force_id: Optional[int] = None) -> tuple[bool, str]:
        """
        立即注入一条帧到当前/指定 flow：
          - t: "Req" | "Res" | "Notify"
          - method: 例如 ".lq.Lobby.amuletActivitySelectPack"
          - data:   Protobuf 对应的 dict（由 codec.build_frame 负责序列化）
          - peer_key: "clientIP|serverHost"；不传则用最近活跃 flow
          - force_id: 可选，强制使用某个 msg_id（不传则用 last_req_id+1）
        返回: (ok: bool, detail: str)
        """
        flow = self._pick_flow(peer_key)
        if not flow or not getattr(flow, "websocket", None):
            return False, "no-active-websocket-flow"

        inj = {"type": t, "method": method, "data": data}

        # 对 Req/Res 显式指定“下一号”更稳（避免 build_frame 默认取值不合预期）
        if t in ("Req", "Res"):
            try:
                next_id = (int(self.codec._last_req_id) + 1) & 0xFFFF
            except Exception:
                next_id = 1
            inj["id"] = force_id if force_id is not None else next_id
        try:
            inj_bytes = self.codec.build_frame(inj)
        except Exception as e:
            return False, f"build-frame-failed: {e}"

        try:
            # 对“请求”必须用 from_client=True 才会登记：_res_map[msg_id] = (method, resp_cls)
            from_client_flag = True if t == "Req" else False
            self.codec.parse_frame(inj_bytes, from_client_flag)
        except Exception:
            # 登记失败不致命，继续尝试注入
            pass

        # 3) 注入（Req 发给服务器；Res/Notify 发给客户端）
        try:
            to_client = (t in ("Notify", "Res"))
            ctx.master.commands.call("inject.websocket", flow, to_client, inj_bytes, False)
            return True, "ok"
        except Exception as e:
            return False, f"inject-failed: {e}"