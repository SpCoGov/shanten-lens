from typing import List, Dict, Optional, Callable, Tuple, Any

from loguru import logger

from backend.mitm.addon import WsAddon
from backend.model.game_state import GameState
from ...core.interfaces import GameBot


class PacketBot(GameBot):
    def __init__(
            self,
            addon_getter: Callable[[], WsAddon],
            activity_id: int = 250811,
            op_code_map: Optional[Dict[str, int]] = None,
            verbose: bool = True,
            default_timeout: float = 5.0,
            state_getter: Optional[Callable[[], Any]] = None,
    ):
        self._get_addon = addon_getter
        self.activity_id = activity_id
        self.verbose = verbose
        self.default_timeout = default_timeout
        self._get_state = state_getter

        # 操作码（保留你的键）
        self.op_code = {
            "discard": 1,
            "tsumo": 8,
            "kan": 4,
            "skip_replace": 100,
            "replace": 101,
        }
        if op_code_map:
            self.op_code.update(op_code_map)

    def bind(self) -> bool:
        return True

    def refresh(self) -> bool:
        return True

    def _state(self) -> GameState:
        return self._get_state() if self._get_state else None

    def _get_peer_key(self) -> Optional[str]:
        addon: WsAddon = self._get_addon()
        if not addon or not getattr(addon, "last_flow", None):
            return None
        f = addon.last_flow
        try:
            return f"{f.client_conn.address[0]}|{f.server_conn.address[0]}"
        except Exception:
            return None

    def _ops_allow(self, t: int) -> bool:
        st = self._state()
        if not st:
            return True
        try:
            ops = st.next_operation or []
        except Exception:
            return True
        return any(op.get("type") == t for op in ops)

    def _check_stage(self, stage: int) -> bool:
        st = self._state()
        if not st:
            return False
        return stage == st.stage

    def _label(self, tile_id: int) -> str:
        st = self._state()
        try:
            return st.deck_map.get(tile_id, "??")
        except Exception:
            return "??"

    def _inject_and_wait(
            self, *, method: str, data: dict,
            delay_sec: float, timeout: Optional[float] = None
    ) -> Tuple[bool, str, Optional[Dict[str, Any]]]:
        addon = self._get_addon()
        if not addon or not getattr(addon, "last_flow", None):
            return False, "addon-or-flow-not-ready", None

        ok, reason, msg_id = addon.inject_now(
            method=method,
            data=data,
            t="Req",
            peer_key=self._get_peer_key(),
        )
        if not ok or msg_id < 0:
            return False, f"inject-failed:{reason}", None

        # 注册同步等待器并阻塞等待
        ev = addon.register_waiter_sync(msg_id)
        to = float(delay_sec) if timeout is None else float(timeout)
        try:
            signaled = ev.wait(to)
            if not signaled:
                addon.discard_waiter_sync(msg_id)
                return False, "timeout", None
            resp = addon.pop_waiter_sync_resp(msg_id)
            if resp.get("error", None) is not None:
                logger.error(f"error occurred: {resp.get('error')}")
                return False, f"error code: {resp.get('error', {}).get('code', 0)}", None
            return True, "ok", resp
        except Exception as e:
            addon.discard_waiter_sync(msg_id)
            return False, f"wait-error:{e}", None

    def _operate(
            self, *, pkt_type: int, tile_list: List[int],
            delay_sec: float, timeout: Optional[float] = None
    ) -> Tuple[bool, str, Optional[Dict[str, Any]]]:
        return self._inject_and_wait(
            method=".lq.Lobby.amuletActivityOperate",
            data={"activityId": self.activity_id, "type": pkt_type, "tileList": tile_list},
            delay_sec=delay_sec, timeout=timeout
        )

    def giveup(self, delay_sec: float = 0.1) -> bool:
        ok, reason, resp = self._inject_and_wait(
            method=".lq.Lobby.amuletActivityGiveup",
            data={"activityId": self.activity_id},
            delay_sec=delay_sec
        )
        return ok

    def start_game(self, delay_sec: float = 0.1):
        ok, reason, resp = self._inject_and_wait(
            method=".lq.Lobby.amuletActivityStartGame",
            data={"activityId": self.activity_id},
            delay_sec=delay_sec
        )
        return ok

    def op_tsumo(self, delay_sec: float = 0.3) -> bool:
        t = self.op_code.get("tsumo")
        if t is None:
            raise NotImplementedError("PacketBot.op_tsumo: no op_code 'tsumo'")
        if not self._ops_allow(t):
            logger.error("gamestate disallow tsumo")
            return False
        ok, reason, resp = self._operate(pkt_type=t, tile_list=[], delay_sec=delay_sec)
        if self.verbose:
            logger.info(f"tsumo -> ok={ok}, reason={reason}, resp={resp}")
        return ok

    def op_skip_replace(self, delay_sec: float = 0.3) -> bool:
        t = self.op_code.get("skip_replace")
        if t is None:
            raise NotImplementedError("PacketBot.op_skip_replace: no op_code 'skip_replace'")
        if not self._ops_allow(t):
            logger.error("gamestate disallow skip-replace")
            return False
        ok, reason, resp = self._operate(pkt_type=t, tile_list=[], delay_sec=delay_sec)
        if self.verbose:
            logger.info(f"skip_replace -> ok={ok}, reason={reason}, resp={resp}")
        return ok

    def op_replace(self, tile_id: int, delay_sec: float = 0.3) -> bool:
        t = self.op_code.get("replace")
        if t is None:
            raise NotImplementedError("PacketBot.op_replace: no op_code 'replace'")
        if not self._ops_allow(t):
            logger.error("gamestate disallow replace")
            return False
        ok, reason, resp = self._operate(pkt_type=t, tile_list=[tile_id], delay_sec=delay_sec)
        if self.verbose:
            logger.info(f"replace tile={tile_id} -> ok={ok}, reason={reason}, resp={resp}")
        return ok

    def discard_by_tile_id(
            self,
            tile_id: int,
            allow_tsumogiri: bool = True,
            delay_sec: float = 0.3,
    ) -> bool:
        t = self.op_code.get("discard", 1)
        if not self._ops_allow(t):
            logger.error("gamestate disallow discard")
            return False
        ok, reason, resp = self._operate(pkt_type=t, tile_list=[tile_id], delay_sec=delay_sec)
        if self.verbose:
            logger.info(f"discard id={tile_id}({self._label(tile_id)}) -> ok={ok}, reason={reason}, resp={resp}")
        return ok

    def select_free_effect(self, selected_id: int, delay_sec: float = 0.3) -> bool:
        st = self._state()
        if not self._check_stage(1):
            return False
        if any(effect.get("id") == selected_id for effect in st.candidate_effect_list):
            ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivitySelectFreeEffect", data={"activityId": self.activity_id, "selectedId": selected_id}, delay_sec=delay_sec)
            return ok
        return False

    def select_reward_effect(self, selected_id: int, delay_sec: float = 0.3) -> bool:
        st = self._state()
        if not self._check_stage(7):
            return False
        if any(effect.get("id") == selected_id for effect in st.candidate_effect_list):
            ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivitySelectRewardPack", data={"activityId": self.activity_id, "id": selected_id}, delay_sec=delay_sec)
            return ok
        return False

    def select_effect(self, selected_id: int, delay_sec: float = 0.3) -> bool:
        st = self._state()
        if not self._check_stage(5):
            return False
        if any(effect.get("id") == selected_id for effect in st.candidate_effect_list):
            ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivitySelectPack", data={"activityId": self.activity_id, "id": selected_id}, delay_sec=delay_sec)
            return ok
        return False

    def buy_pack(self, good_id: int, delay_sec: float = 0.3) -> bool:
        st = self._state()
        if not self._check_stage(4):
            return False
        if any(good.get("id") == good_id and good.get("sold") is False for good in st.goods):
            ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivityBuy", data={"activityId": self.activity_id, "id": good_id}, delay_sec=delay_sec)
            return ok
        return False

    def refresh_shop(self, delay_sec: float = 0.3):
        st = self._state()
        if not self._check_stage(4):
            return False
        if st.coin >= st.refresh_price:
            ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivityRefreshShop", data={"activityId": self.activity_id}, delay_sec=delay_sec)
            return ok
        return False

    def sell_effect(self, uid: int, delay_sec: float = 0.3):
        st = self._state()
        if any(effect.get("uid") == uid for effect in st.effect_list):
            ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivitySellEffect", data={"activityId": self.activity_id, "id": uid}, delay_sec=delay_sec)
            return ok
        return False

    def end_shopping(self, delay_sec: float = 0.3):
        if not self._check_stage(4):
            return False
        ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivityEndShopping", data={"activityId": self.activity_id}, delay_sec=delay_sec)
        return ok

    def next_level(self, delay_sec: float = 0.3):
        if not self._check_stage(6):
            return False
        ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivityUpgrade", data={"activityId": self.activity_id}, delay_sec=delay_sec)
        return ok

    def fetch_amulet_activity_data(self, delay_sec: float = 0.1):
        ok, reason, resp = self._inject_and_wait(
            method=".lq.Lobby.fetchAmuletActivityData",
            data={"activityId": self.activity_id},
            delay_sec=delay_sec
        )
        return ok
