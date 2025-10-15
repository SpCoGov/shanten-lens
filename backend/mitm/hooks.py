from typing import Tuple, Any, Dict, List
import threading
from mitmproxy import ctx
from loguru import logger

from backend.bot.chiitoi_recommender import chiitoi_recommendation_json
from backend.app import MANAGER, GAME_STATE
import backend.mitm.addon as _addon


def on_outbound(view: Dict) -> Tuple[str, Any]:
    # 发送前不过滤
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
        events = data.get("events", [])
        matched = next((e for e in events if e.get("type") == 23), None)
        if matched:
            value_changes = matched.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            hands = round_info.get("hands", {}).get("value", None)
            pool = round_info.get("pool", {}).get("value", None)
            locked_tiles = round_info.get("lockedTile", {}).get("value", None)
            if hands and pool:
                GAME_STATE.update_pool(pool, hand_tiles=hands, locked_tiles=locked_tiles, push_gamestate=False)
                desktop_remain = round_info.get("desktopRemain", {}).get("value", 0)
                # 进入换牌阶段
                switch_stage_event = next((e for e in events if e.get("type") == 19), None)
                if switch_stage_event:
                    value_changes_19 = switch_stage_event.get("valueChanges", {})
                    stage = value_changes_19.get("stage", -1)
                    ended = value_changes_19.get("ended", False)
                    GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended, reason=".lq.Lobby.amuletActivityUpgrade:19")
                else:
                    GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=GAME_STATE.stage, ended=GAME_STATE.ended, reason=".lq.Lobby.amuletActivityUpgrade:23")
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
            GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=ended, reason=".lq.Lobby.amuletActivityOperate:100")
            return "pass", None
        # type = 4: 换牌
        switch_event = next((e for e in events if e.get("type") == 4), None)
        if switch_event:
            value_changes = switch_event.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            used = round_info.get("used", {}).get("value", [])
            GAME_STATE.update_switch_used_tiles(used=used, push_gamestate=False, reason=".lq.Lobby.amuletActivityOperate:4")
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=GAME_STATE.ended)

        # type = 6: 摸牌
        draw_event = next((e for e in events if e.get("type") == 6), None)
        if draw_event:
            value_changes = draw_event.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            desktop_remain = round_info.get("desktopRemain", {}).get("value", 0)
            stage = value_changes.get("stage", -1)
            ended = value_changes.get("ended", False)

            after_draw_hands = draw_event.get("valueChanges", {}).get("round", {}).get("hands", {}).get("value", None)
            if after_draw_hands:
                GAME_STATE.on_draw_tile(after_draw_hands, after_draw_hands[len(after_draw_hands) - 1], push_gamestate=False)

            GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended, reason=".lq.Lobby.amuletActivityOperate:6")

            json = chiitoi_recommendation_json(deck_map=GAME_STATE.deck_map,
                                               hand_ids=GAME_STATE.hand_tiles,  # 此时 14 张
                                               wall_ids=GAME_STATE.wall_tiles,
                                               policy="count")
            logger.info(json)

            if not json.get("data", {}).get("win_now", False):
                if MANAGER.get("game.auto_discard"):
                    discard_id = int(json["data"]["discard_id"])

                    # 固定 peer_key，尽量发到同一条 WS
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

                    ctx.master.event_loop.call_later(1.0, _do_inject)
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

                    ctx.master.event_loop.call_later(1.0, _do_inject)

    # 进入青云之志界面时获取已经开始的游戏数据
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.fetchAmuletActivityData":
        data = view.get("data", {}).get("data", {})
        game = data.get("game", None)
        if game:
            round_info = game.get("round", {})
            hands = round_info.get("hands", [])
            pool = round_info.get("pool", [])
            locked_tiles = round_info.get("lockedTile", [])
            GAME_STATE.update_pool(pool, hand_tiles=hands, locked_tiles=locked_tiles, push_gamestate=False, reason=".lq.Lobby.fetchAmuletActivityData")
            desktop_remain = round_info.get("desktopRemain", 0)
            stage = game.get("stage", -1)
            ended = game.get("ended", False)

            if desktop_remain < 36:
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended, push_gamestate=False)
                GAME_STATE.refresh_wall_by_remaning()
            else:
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended)
    # 只是用来更新一下状态
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityGiveup":
        GAME_STATE.on_giveup()
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivitySelectFreeEffect":
        data = view.get("data", {})
        events = data.get("events", [])
        start_event = next((e for e in events if e.get("type") == 2), None)
        value_changes = start_event.get("valueChanges", {})
        stage = value_changes.get("stage", -1)
        ended = value_changes.get("ended", False)
        GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=ended, reason=".lq.Lobby.amuletActivitySelectFreeEffect:2")
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityStartGame":
        data = view.get("data", {})
        events = data.get("events", [])
        start_event = next((e for e in events if e.get("type") == 1), None)
        value_changes = start_event.get("valueChanges", {})
        stage = value_changes.get("stage", -1)
        ended = value_changes.get("ended", False)
        GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=ended, reason=".lq.Lobby.amuletActivityStartGame:1")
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityBuy":
        data = dict(view["data"])
        events = data.get("events", [])
        buy_amulet_event = next((e for e in events if e.get("type") == 13), None)
        if buy_amulet_event:
            value_changes = buy_amulet_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=GAME_STATE.ended, reason=".lq.Lobby.amuletActivityBuy:13")
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivitySelectPack":
        data = view.get("data", {})
        events = data.get("events", [])
        select_amulet_event = next((e for e in events if e.get("type") == 14), None)
        if select_amulet_event:
            value_changes = select_amulet_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=GAME_STATE.ended, reason=".lq.Lobby.amuletActivitySelectPack:14")
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityEndShopping":
        data = view.get("data", {})
        events = data.get("events", [])
        end_shopping_event = next((e for e in events if e.get("type") == 22), None)
        if end_shopping_event:
            value_changes = end_shopping_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=GAME_STATE.ended, reason=".lq.Lobby.amuletActivityEndShopping:22")
    return "pass", None
