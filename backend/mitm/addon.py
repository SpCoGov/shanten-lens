from typing import Callable, Tuple, Any, Dict, List, Optional
from mitmproxy import http, ctx
from loguru import logger

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

class WsAddon:
    def __init__(self, codec: LiqiCodec):
        self.codec = codec
        self.on_outbound: Optional[HookFn] = None
        self.on_inbound: Optional[HookFn] = None
        self.subscribers: List[Callable[[Dict], None]] = []

    def subscribe(self, cb: Callable[[Dict], None]):
        self.subscribers.append(cb)

    def websocket_message(self, flow: http.HTTPFlow):
        # 只处理真正的 websocket 流量（mitmproxy 可能还会收到普通 HTTP）
        if not flow.websocket:
            return

        # 取最后一条消息（mitmproxy 把每条 ws 消息追加到 flow.websocket.messages）
        message = flow.websocket.messages[-1]

        # 可选：只处理来自某个路径/主机的 ws（例如只要 /gateway）
        # if flow.request.path != "/gateway":
        #     return

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

        # 记录：方法名 + 完整数据（可切换为 debug 级）
        #   - view['data'] 是已解析的 dict（我们 pretty print）
        #   - view['raw'] 是原始 bytes（我们也以 base64/hex 打印）
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
