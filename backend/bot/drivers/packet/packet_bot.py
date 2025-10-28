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
            default_timeout: float = 5.0,
            state_getter: Optional[Callable[[], Any]] = None,
    ):
        self.get_addon = addon_getter
        self.activity_id = activity_id
        self.default_timeout = default_timeout
        self._get_state = state_getter

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
        addon: WsAddon = self.get_addon()
        if not addon:
            return None
        f = getattr(addon, "preferred_flow", None)
        if not f or not getattr(f, "websocket", None):
            return None
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
        addon = self.get_addon()
        if not addon:
            return False, "addon-or-flow-not-ready", None

        peer_key = self._get_peer_key()
        if not peer_key:
            return False, "no-preferred-flow", None

        ok, reason, msg_id = addon.inject_now(
            method=method,
            data=data,
            t="Req",
            peer_key=peer_key,
        )
        if not ok or msg_id < 0:
            return False, f"inject-failed:{reason}", None

        ev = addon.register_waiter_sync(msg_id)
        to = float(delay_sec) if timeout is None else float(timeout)
        try:
            signaled = ev.wait(to)
            if not signaled:
                addon.discard_waiter_sync(msg_id)
                return False, "timeout", None
            resp = addon.pop_waiter_sync_resp(msg_id)
            if resp.get('data', {}).get('error', None) is not None:
                logger.error(f"error occurred: {resp.get('data', {}).get('error')}")
                return False, f"error code: {resp.get('data', {}).get('error', {}).get('code', 0)}", None
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

    def heartbeat(self, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        ok, reason, resp = self._inject_and_wait(
            method=".lq.Route.heartbeat",
            data={"delay": 100, "platform": 5, "networkQuality": 100, "noOperationCounter": 0},
            delay_sec=delay_sec
        )
        return ok, reason, resp

    def giveup(self, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        ok, reason, resp = self._inject_and_wait(
            method=".lq.Lobby.amuletActivityGiveup",
            data={"activityId": self.activity_id},
            delay_sec=delay_sec
        )
        return ok, reason, resp

    def start_game(self, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        ok, reason, resp = self._inject_and_wait(
            method=".lq.Lobby.amuletActivityStartGame",
            data={"activityId": self.activity_id},
            delay_sec=delay_sec
        )
        return ok, reason, resp

    def op_tsumo(self, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        t = self.op_code.get("tsumo")
        if t is None:
            raise NotImplementedError("PacketBot.op_tsumo: no op_code 'tsumo'")
        if not self._ops_allow(t):
            logger.error("gamestate disallow tsumo")
            return False, "gamestate disallow discard", None
        ok, reason, resp = self._operate(pkt_type=t, tile_list=[], delay_sec=delay_sec)
        return ok, reason, resp

    def op_skip_change(self, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        t = self.op_code.get("skip_replace")
        if t is None:
            raise NotImplementedError("PacketBot.op_skip_replace: no op_code 'skip_replace'")
        if not self._ops_allow(t):
            logger.error("gamestate disallow skip-replace")
            return False, "gamestate disallow discard", None
        ok, reason, resp = self._operate(pkt_type=t, tile_list=[], delay_sec=delay_sec)
        return ok, reason, resp

    def op_change(self, tile_ids: List[int], delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        t = self.op_code.get("replace")
        if t is None:
            raise NotImplementedError("PacketBot.op_replace: no op_code 'replace'")
        if not self._ops_allow(t):
            logger.error("gamestate disallow replace")
            return False, "gamestate disallow discard", None
        ok, reason, resp = self._operate(pkt_type=t, tile_list=tile_ids, delay_sec=delay_sec)
        return ok, reason, resp

    def discard_by_tile_id(
            self,
            tile_id: int,
            allow_tsumogiri: bool = True,
            delay_sec: float = 3,
    ) -> Tuple[bool, str, Optional[dict]]:
        t = self.op_code.get("discard", 1)
        if not self._ops_allow(t):
            logger.error("gamestate disallow discard")
            return False, "gamestate disallow discard", None
        ok, reason, resp = self._operate(pkt_type=t, tile_list=[tile_id], delay_sec=delay_sec)
        return ok, reason, resp

    def select_free_effect(self, selected_id: int, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        st = self._state()
        if not self._check_stage(1):
            return False, "in the illegal stage", None
        if any(effect.get("id") == selected_id for effect in st.candidate_effect_list):
            ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivitySelectFreeEffect", data={"activityId": self.activity_id, "selectedId": selected_id}, delay_sec=delay_sec)
            return ok, reason, resp
        return False, "unknown id", None

    def select_reward_effect(self, selected_id: int, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        st = self._state()
        if not self._check_stage(7):
            return False, "in the illegal stage", None
        if int(selected_id) == 0 or any(effect.get("id") == selected_id for effect in st.candidate_effect_list):
            ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivitySelectRewardPack", data={"activityId": self.activity_id, "id": selected_id}, delay_sec=delay_sec)
            return ok, reason, resp
        return False, "unknown id", None

    def select_effect(self, selected_id: int, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        st = self._state()
        if not self._check_stage(5):
            return False, "in the illegal stage", None
        if int(selected_id) == 0 or any(effect.get("id") == selected_id for effect in st.candidate_effect_list):
            ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivitySelectPack", data={"activityId": self.activity_id, "id": selected_id}, delay_sec=delay_sec)
            return ok, reason, resp
        return False, "unknown id", None

    def buy_pack(self, good_id: int, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        st = self._state()
        if not self._check_stage(4):
            return False, "in the illegal stage", None
        good = next((g for g in st.goods if g.get("id") == good_id and g.get("sold") is False), None)
        if good:
            if good.get("price", 0) <= st.coin:
                ok, reason, resp = self._inject_and_wait(
                    method=".lq.Lobby.amuletActivityBuy",
                    data={"activityId": self.activity_id, "id": good_id},
                    delay_sec=delay_sec
                )
                return ok, reason, resp
            return False, "coin not enough", None
        return False, "unknown id", None

    def refresh_shop(self, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        st = self._state()
        if not self._check_stage(4):
            return False, "in the illegal stage", None
        if st.coin >= st.refresh_price:
            ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivityRefreshShop", data={"activityId": self.activity_id}, delay_sec=delay_sec)
            return ok, reason, resp
        return False, "coin not enough", None

    def sell_effect(self, uid: int, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        st = self._state()
        if any(effect.get("uid") == uid for effect in st.effect_list):
            ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivitySellEffect", data={"activityId": self.activity_id, "id": uid}, delay_sec=delay_sec)
            return ok, reason, resp
        return False, "unknown id", None

    def sort_effect(self, sorted_uid: List[int], delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        st = self._state()
        try:
            cur_uids = [int(e.get("uid")) for e in (st.effect_list or []) if e.get("uid") is not None]
        except Exception:
            return False, "bad-effect-list", None
        if not cur_uids:
            return False, "no-effects", None
        try:
            in_uids = [int(x) for x in (sorted_uid or [])]
        except Exception:
            return False, "sorted_uid-not-integers", None
        if len(in_uids) != len(set(in_uids)):
            return False, "sorted_uid-has-duplicates", None
        if set(in_uids) != set(cur_uids):
            return False, "sorted_uid-mismatch-current-effects", None
        if in_uids == cur_uids:
            return True, "already sorted", None
        ok, reason, resp = self._inject_and_wait(
            method=".lq.Lobby.amuletActivitySortEffect",
            data={"activityId": self.activity_id, "sortedUid": in_uids},
            delay_sec=delay_sec
        )
        return ok, reason, resp

    def end_shopping(self, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        if not self._check_stage(4):
            return False, "in the illegal stage", None
        ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivityEndShopping", data={"activityId": self.activity_id}, delay_sec=delay_sec)
        return ok, reason, resp

    def next_level(self, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        if not self._check_stage(6):
            return False, "in the illegal stage", None
        ok, reason, resp = self._inject_and_wait(method=".lq.Lobby.amuletActivityUpgrade", data={"activityId": self.activity_id}, delay_sec=delay_sec)
        return ok, reason, resp

    def fetch_amulet_activity_data(self, delay_sec: float = 3) -> Tuple[bool, str, Optional[dict]]:
        ok, reason, resp = self._inject_and_wait(
            method=".lq.Lobby.fetchAmuletActivityData",
            data={"activityId": self.activity_id},
            delay_sec=delay_sec
        )
        return ok, reason, resp