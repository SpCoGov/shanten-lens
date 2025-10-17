import asyncio
import ctypes
import platform
from typing import Tuple, Any, Dict, List, Set, Iterable, Optional

from loguru import logger
from mitmproxy import ctx

import backend.mitm.addon as _addon
from backend.app import AMULET_REG, BADGE_REG
from backend.app import MANAGER, GAME_STATE, broadcast
from backend.bot.chiitoi_recommender import chiitoi_recommendation_json

ID_KAVI = 230
BADGE_CONDUCTION = 600170


def _base(x: Any) -> int:
    try:
        return int(x) // 10
    except:
        return 0


def _plus(x: Any) -> bool:
    try:
        return abs(int(x)) % 10 == 1
    except:
        return False


def _bid(row: Optional[dict]) -> int:
    if not isinstance(row, dict): return 0
    b = row.get("badge")
    if isinstance(b, dict) and "id" in b:
        try:
            return int(b["id"])
        except:
            return 0
    try:
        return int(row.get("badgeId", 0))
    except:
        return 0


def _effects() -> List[dict]:
    return list(getattr(GAME_STATE, "effect_list", []) or [])


def _name(row: Optional[dict]) -> str:
    if not isinstance(row, dict): return "(无)"
    a = AMULET_REG.get(_base(row.get("id"))) if AMULET_REG else None
    return a.name if a else f"护身符#{_base(row.get('id'))}"


def _badge_label(row: Optional[dict]) -> str:
    bid = _bid(row)
    if bid <= 0: return "无"
    b = BADGE_REG.get(bid) if BADGE_REG else None
    return f"{bid}（{b.name}）" if b else f"{bid}"


def _neighbors_of_kavi() -> dict:
    ef = _effects()
    n = len(ef)
    for i, row in enumerate(ef):
        rid = row.get("id")
        if _base(rid) == ID_KAVI:
            return {
                "left": ef[i - 1] if i - 1 >= 0 else None,
                "right": ef[i + 1] if i + 1 < n else None,
                "kavi_raw_id": int(rid) if rid is not None else 0,
                "kavi_index": i,
            }
    return {"left": None, "right": None, "kavi_raw_id": 0, "kavi_index": -1}


def _effects_with_badge(bid: int) -> List[dict]:
    out: List[dict] = []
    for i, row in enumerate(_effects()):
        if _bid(row) == bid:
            rid = row.get("id", 0)
            out.append({"index": i, "raw_id": rid, "base_id": _base(rid), "row": row})
    return out


def _effects_without_badge() -> List[dict]:
    out: List[dict] = []
    for i, row in enumerate(_effects()):
        if _bid(row) <= 0:
            rid = row.get("id", 0)
            out.append({"index": i, "raw_id": rid, "base_id": _base(rid), "row": row})
    return out


def _fmt_amulets(ids: Iterable[int]) -> List[str]:
    s: List[str] = []
    for aid in sorted(set(ids)):
        a = AMULET_REG.get(aid) if AMULET_REG else None
        s.append(f"  • {(a.name if a else f'护符#{aid}')}（ID:{aid}）")
    return s


def _fmt_badges(ids: Iterable[int]) -> List[str]:
    s: List[str] = []
    for bid in sorted(set(ids)):
        b = BADGE_REG.get(bid) if BADGE_REG else None
        s.append(f"  • {(b.name if b else f'印章#{bid}')}（ID:{bid}）")
    return s


def _collect_candidate_sets() -> tuple[Set[int], Set[int], List[dict]]:
    lst: List[dict] = list(getattr(GAME_STATE, "candidate_effect_list", []) or [])
    a_set, b_set = set(), set()
    for r in lst:
        try:
            aid = _base(r.get("id", 0))
            bid = int(r.get("badgeId", 0))
        except:
            continue
        if aid > 0: a_set.add(aid)
        if bid > 0: b_set.add(bid)
    return a_set, b_set, lst


def _fuse_hits() -> tuple[Set[int], Set[int], str]:
    cfg = MANAGER.to_table_payload("fuse")
    guard = (cfg or {}).get("guard_skip_contains", {}) or {}
    watch_a = set(map(int, guard.get("amulets", [])))
    watch_b = set(map(int, guard.get("badges", [])))
    cand_a, cand_b, _ = _collect_candidate_sets()
    hit_a, hit_b = cand_a & watch_a, cand_b & watch_b
    if not (hit_a or hit_b): return hit_a, hit_b, ""
    lines = ["检测到：卡包包含监控的护身符/印章", ""]
    if hit_a: lines += ["护身符：", *_fmt_amulets(hit_a), ""]
    if hit_b: lines += ["印章：", *_fmt_badges(hit_b), ""]
    lines.append("是否仍然跳过卡包？")
    return hit_a, hit_b, "\n".join(lines)


def _confirm(title: str, msg: str) -> bool:
    if platform.system() != "Windows": return False
    flags = 0x00000004 | 0x00000030 | 0x00001000 | 0x00010000 | 0x00040000
    return ctypes.windll.user32.MessageBoxW(0, str(msg), str(title), flags) == 6


def _build_kavi_msg(is_plus: bool, min_cnt: int, cnt: int, left: Optional[dict], right: Optional[dict]) -> str:
    lines: List[str] = []
    lines.append(f"检测到：传导卡维已装备（{'Plus' if is_plus else '普通'}）")
    lines.append(f"传导卡数量：{cnt}（阈值：{min_cnt}）")
    lines += ["", "邻位：",
              f"  左邻：{_name(left)}，印章：{_badge_label(left)}",
              f"  右邻：{_name(right)}，印章：{_badge_label(right)}", "",
              "规则：为避免误触发，请确保卡维相邻至少有一侧是「没有印章」的护身符。",
              "当前：两侧均带有印章。是否仍然继续开局？"]
    return "\n".join(lines)


def on_outbound(view: Dict) -> Tuple[str, Any]:
    try:
        if view.get("type") == "Req" and view.get("method") == ".lq.Lobby.amuletActivitySelectPack":
            is_skip = int((view.get("data") or {}).get("id", 0)) == 0
            if not is_skip: return "pass", None
            a, b, text = _fuse_hits()
            if a or b:
                return ("pass", None) if _confirm("熔断确认：跳过卡包？", text) else ("drop", None)
            return "pass", None

        if view.get("type") == "Req" and view.get("method") == ".lq.Lobby.amuletActivityUpgrade":
            cfg = MANAGER.to_table_payload("fuse") or {}
            if not bool(cfg.get("enable_prestart_kavi_guard", True)):
                return "pass", None

            ef = _effects()
            has_kavi = any(_base(e.get("id")) == ID_KAVI and _bid(e) == BADGE_CONDUCTION for e in ef)
            if not has_kavi: return "pass", None

            min_cnt = int(cfg.get("conduction_min_count", 3))
            cnt = sum(1 for e in ef if _bid(e) == BADGE_CONDUCTION)
            if cnt < min_cnt: return "pass", None

            nb = _neighbors_of_kavi()
            if nb["kavi_index"] < 0: return "pass", None
            left, right = nb["left"], nb["right"]
            if (left is not None and _bid(left) == 0) or (right is not None and _bid(right) == 0):
                return "pass", None

            msg = _build_kavi_msg(_plus(nb["kavi_raw_id"]), min_cnt, cnt, left, right)
            return ("pass", None) if _confirm("熔断确认：确认开局？", msg) else ("drop", None)
        # 黑客、不稳定存的第一个数据为复制或变身的护身符：{"id":2320,"store":[2290,1234]} 2290为盗印，不稳定228、黑客232、卡维230
        return "pass", None
    except Exception:
        return "pass", None


def on_inbound(view: Dict) -> Tuple[str, Any]:
    """
    .lq.Lobby.fetchAmuletActivityData           进入青云之志界面
    .lq.Lobby.amuletActivityGiveup              放弃
    .lq.Lobby.amuletActivityOperate             游戏中打牌等操作
    .lq.Lobby.amuletActivityStartGame           游戏开始，这回合获得的牌山数组似乎没有任何作用
    .lq.Lobby.amuletActivityUpgrade             回合开始

    .lq.Lobby.amuletActivitySelectFreeEffect    选择免费的卡包
    .lq.Lobby.amuletActivityBuy                 购买卡包
    .lq.Lobby.amuletActivitySelectPack          选择卡护身符、跳过
    .lq.Lobby.amuletActivitySellEffect          卖出护身符
    .lq.Lobby.amuletActivityEffectSort          对护身符排序
    .lq.Lobby.amuletActivityUpgradeShopBuff     升级增益
    .lq.Lobby.amuletActivityRefreshShop         刷新商店
    .lq.Lobby.amuletActivityEndShopping         购买结束
    """
    # 服务器下发公告
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.fetchAnnouncement" and MANAGER.get("game.modify_announcement"):
        newd = dict(view["data"])
        anns: List[Dict] = newd.get("announcements", [])
        anns.insert(0, {
            "id": 9999,
            "title": "欢迎使用向听镜",
            "content": "向听镜已启动，祝各位大大欧气满满！",
            "headerImage": "internal://2.jpg"
        })
        return "modify", newd
    # 开始新游戏
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityUpgrade":
        data = dict(view["data"])
        modify = False
        events = data.get("events", [])
        matched = next((e for e in events if e.get("type") == 23), None)
        if matched:
            value_changes = matched.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            hands = round_info.get("hands", {}).get("value", None)
            pool = round_info.get("pool", {}).get("value", None)
            locked_tiles = round_info.get("lockedTile", {}).get("value", None)
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            if hands and pool:
                GAME_STATE.update_pool(pool, hand_tiles=hands, locked_tiles=locked_tiles, push_gamestate=False)
                desktop_remain = round_info.get("desktopRemain", {}).get("value", 0)
                level = value_changes.get("game", {}).get("level", {}).get("value", 0)
                # 进入换牌阶段
                switch_stage_event = next((e for e in events if e.get("type") == 19), None)
                if switch_stage_event:
                    value_changes_19 = switch_stage_event.get("valueChanges", {})
                    stage = value_changes_19.get("stage", -1)
                    ended = value_changes_19.get("ended", False)
                    GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended, level=level, effect_list=effect_list, reason=".lq.Lobby.amuletActivityUpgrade:19")
                else:
                    GAME_STATE.update_other_info(desktop_remain=desktop_remain, level=level, effect_list=effect_list, reason=".lq.Lobby.amuletActivityUpgrade:23")
                if MANAGER.get("game.public_all"):
                    show_desktop_tiles = round_info.get("showDesktopTiles", {}).get("value", [])
                    show_desktop_tiles.clear()
                    pos = len(GAME_STATE.wall_tiles) + len(GAME_STATE.locked_tiles) - 1

                    for tile in GAME_STATE.wall_tiles:
                        show_desktop_tiles.append({"id": tile, "pos": pos})
                        pos -= 1
                    for tile in GAME_STATE.locked_tiles:
                        show_desktop_tiles.append({"id": tile, "pos": pos})
                        pos -= 1
                    modify = True
        matched = next((e for e in events if e.get("type") == 48), None)
        if matched:
            value_changes = matched.get("valueChanges", {})
            coin = int(value_changes.get("game", {}).get("coin", {}).get("value", None))
            GAME_STATE.update_other_info(coin=coin, reason=".lq.Lobby.amuletActivityUpgrade:48")
        matched = next((e for e in events if e.get("type") == 49), None)
        if matched:
            value_changes = matched.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(stage=stage, reason=".lq.Lobby.amuletActivityUpgrade:49")
        if modify:
            return "modify", data
    # 游戏中打牌等操作
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityOperate":
        data = view.get("data", {})
        events = data.get("events", [])
        # type = 100: 游戏结束
        end_event = next((e for e in events if e.get("type") == 100), None)
        if end_event:
            value_changes = end_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            ended = value_changes.get("ended", True)
            GAME_STATE.update_other_info(stage=stage, ended=ended, reason=".lq.Lobby.amuletActivityOperate:100")
            return "pass", None
        # type = 4: 换牌
        switch_event = next((e for e in events if e.get("type") == 4), None)
        if switch_event:
            value_changes = switch_event.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            used = round_info.get("used", {}).get("value", [])
            GAME_STATE.update_switch_used_tiles(used=used, push_gamestate=False, reason=".lq.Lobby.amuletActivityOperate:4")
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(stage=stage)

        # type = 6: 摸牌
        draw_event = next((e for e in events if e.get("type") == 6), None)
        if draw_event:
            value_changes = draw_event.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            desktop_remain = round_info.get("desktopRemain", {}).get("value", 0)
            stage = value_changes.get("stage", -1)
            ended = value_changes.get("ended", False)
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            after_draw_hands = draw_event.get("valueChanges", {}).get("round", {}).get("hands", {}).get("value", None)
            if after_draw_hands:
                GAME_STATE.on_draw_tile(after_draw_hands, after_draw_hands[len(after_draw_hands) - 1], push_gamestate=False)

            GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended, effect_list=effect_list, reason=".lq.Lobby.amuletActivityOperate:6")

            json = chiitoi_recommendation_json(deck_map=GAME_STATE.deck_map,
                                               hand_ids=GAME_STATE.hand_tiles,
                                               wall_ids=GAME_STATE.wall_tiles,
                                               policy="speed")
            loop = asyncio.get_running_loop()
            loop.create_task(broadcast(json))

            json = chiitoi_recommendation_json(deck_map=GAME_STATE.deck_map,
                                               hand_ids=GAME_STATE.hand_tiles,
                                               wall_ids=GAME_STATE.wall_tiles,
                                               policy="count")
            loop = asyncio.get_running_loop()
            loop.create_task(broadcast(json))

            if not json.get("data", {}).get("win_now", False):
                if MANAGER.get("game.auto_discard"):
                    discard_id = int(json["data"]["discard_id"])

                    peer_key = None
                    addon_now = _addon.WS_ADDON_INSTANCE
                    if addon_now and addon_now._last_flow:
                        f = addon_now._last_flow
                        peer_key = f"{f.client_conn.address[0]}|{f.server_conn.address[0]}"

                    def _do_inject():
                        addon = _addon.WS_ADDON_INSTANCE
                        if not addon:
                            logger.warning("WS_ADDON_INSTANCE not ready; skip inject")
                            return
                        ok, reason = addon.inject_now(
                            method=".lq.Lobby.amuletActivityOperate",
                            data={"activityId": 250811, "type": 1, "tileList": [discard_id]},
                            t="Req",
                            peer_key=peer_key,
                        )
                        logger.info(f"success: {ok}, reason: {reason}")

                    ctx.master.event_loop.call_later(0.3, _do_inject)
            else:
                if MANAGER.get("game.auto_tsumo"):
                    peer_key = None
                    addon_now = _addon.WS_ADDON_INSTANCE
                    if addon_now and addon_now._last_flow:
                        f = addon_now._last_flow
                        peer_key = f"{f.client_conn.address[0]}|{f.server_conn.address[0]}"

                    def _do_inject():
                        addon = _addon.WS_ADDON_INSTANCE
                        if not addon:
                            logger.warning("WS_ADDON_INSTANCE not ready; skip inject")
                            return
                        ok, reason = addon.inject_now(
                            method=".lq.Lobby.amuletActivityOperate",
                            data={"activityId": 250811, "type": 8, "tileList": []},
                            t="Req",
                            peer_key=peer_key,
                        )
                        logger.info(f"success: {ok}, reason: {reason}")

                    ctx.master.event_loop.call_later(0.3, _do_inject)
        finish_event = next((e for e in events if e.get("type") == 11), None)
        if finish_event:
            value_changes = finish_event.get("valueChanges", {})
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            coin = int(value_changes.get("game", {}).get("coin", {}).get("value", None))
            GAME_STATE.update_other_info(coin=coin, effect_list=effect_list, reason=".lq.Lobby.amuletActivityOperate:11")
    # 进入青云之志界面时获取已经开始的游戏数据
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.fetchAmuletActivityData":
        data = view.get("data", {}).get("data", {})
        game = data.get("game", None)
        if game:
            round_info = game.get("round", {})
            hands = round_info.get("hands", [])
            pool = round_info.get("pool", [])
            locked_tiles = round_info.get("lockedTile", [])
            effect_list = game.get("effect", {}).get("effectList", None)
            GAME_STATE.update_pool(pool, hand_tiles=hands, locked_tiles=locked_tiles, push_gamestate=False, reason=".lq.Lobby.fetchAmuletActivityData")
            desktop_remain = round_info.get("desktopRemain", 0)
            stage = game.get("stage", -1)
            ended = game.get("ended", False)
            coin = int(game.get("game", {}).get("coin", ""))
            level = game.get("level", None)
            shop = game.get("shop", -1)
            candidate_effect_list = shop.get("candidateEffectList", [])
            if desktop_remain < 36:
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended, level=level, effect_list=effect_list, candidate_effect_list=candidate_effect_list, coin=coin, push_gamestate=False)
                GAME_STATE.refresh_wall_by_remaning()
            else:
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended, level=level, effect_list=effect_list, candidate_effect_list=candidate_effect_list, coin=coin)
    # 只是用来更新一下状态
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityGiveup":
        GAME_STATE.on_giveup()
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivitySelectFreeEffect":
        data = view.get("data", {})
        events = data.get("events", [])
        start_event = next((e for e in events if e.get("type") == 2), None)
        value_changes = start_event.get("valueChanges", {})
        stage = value_changes.get("stage", -1)
        effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
        ended = value_changes.get("ended", False)
        GAME_STATE.update_other_info(stage=stage, ended=ended, effect_list=effect_list, reason=".lq.Lobby.amuletActivitySelectFreeEffect:2")
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityStartGame":
        data = view.get("data", {})
        events = data.get("events", [])
        start_event = next((e for e in events if e.get("type") == 1), None)
        value_changes = start_event.get("valueChanges", {})
        stage = value_changes.get("stage", -1)
        ended = value_changes.get("ended", False)
        GAME_STATE.update_other_info(stage=stage, ended=ended, reason=".lq.Lobby.amuletActivityStartGame:1")
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityBuy":
        data = dict(view["data"])
        events = data.get("events", [])
        buy_amulet_event = next((e for e in events if e.get("type") == 13), None)
        if buy_amulet_event:
            value_changes = buy_amulet_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            game = value_changes.get("game", {})
            ended = value_changes.get("ended", False)
            coin = int(game.get("coin", {}).get("value", None))
            shop = value_changes.get("shop", {})
            candidate_effect_list = shop.get("candidateEffectList", {}).get("value", None)
            GAME_STATE.update_other_info(stage=stage, coin=coin, ended=ended, candidate_effect_list=candidate_effect_list, reason=".lq.Lobby.amuletActivityBuy:13")
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivitySelectPack":
        data = view.get("data", {})
        events = data.get("events", [])
        select_amulet_event = next((e for e in events if e.get("type") == 14), None)
        if select_amulet_event:
            value_changes = select_amulet_event.get("valueChanges", {})
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(stage=stage, effect_list=effect_list, reason=".lq.Lobby.amuletActivitySelectPack:14")
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivitySellEffect":
        data = dict(view["data"])
        events = data.get("events", [])
        sell_amulet_event = next((e for e in events if e.get("type") == 17), None)
        if sell_amulet_event:
            value_changes = sell_amulet_event.get("valueChanges", {})
            game = value_changes.get("game", {})
            stage = value_changes.get("stage", -1)
            coin = int(game.get("coin", {}).get("value", None))
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            ended = value_changes.get("ended", False)
            GAME_STATE.update_other_info(stage=stage, coin=coin, ended=ended, effect_list=effect_list, reason=".lq.Lobby.amuletActivitySellEffect:17")
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityRefreshShop":
        data = dict(view["data"])
        events = data.get("events", [])
        refresh_shop_event = next((e for e in events if e.get("type") == 18), None)
        if refresh_shop_event:
            value_changes = refresh_shop_event.get("valueChanges", {})
            game = value_changes.get("game", {})
            stage = value_changes.get("stage", -1)
            coin = int(game.get("coin", {}).get("value", None))
            GAME_STATE.update_other_info(stage=stage, coin=coin, reason=".lq.Lobby.amuletActivitySellEffect:18")
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityEndShopping":
        data = view.get("data", {})
        events = data.get("events", [])
        end_shopping_event = next((e for e in events if e.get("type") == 22), None)
        if end_shopping_event:
            value_changes = end_shopping_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(stage=stage, reason=".lq.Lobby.amuletActivityEndShopping:22")
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityEffectSort":
        data = view.get("data", {})
        events = data.get("events", [])
        amulet_sort_event = next((e for e in events if e.get("type") == 20), None)
        if amulet_sort_event:
            value_changes = amulet_sort_event.get("valueChanges", {})
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            GAME_STATE.update_other_info(effect_list=effect_list, reason=".lq.Lobby.amuletActivityEffectSort:20")
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivitySelectRewardPack":
        data = view.get("data", {})
        events = data.get("events", [])
        select_reward_event = next((e for e in events if e.get("type") == 16), None)
        if select_reward_event:
            value_changes = select_reward_event.get("valueChanges", {})
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            GAME_STATE.update_other_info(effect_list=effect_list, reason=".lq.Lobby.amuletActivitySelectRewardPack:16")
    return "pass", None
