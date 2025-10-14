from typing import Tuple, Any, Dict, List

from loguru import logger
from backend.app import MANAGER, GAME_STATE


def on_outbound(view: Dict) -> Tuple[str, Any]:
    # 发送前不过滤
    return "pass", None


def on_inbound(view: Dict) -> Tuple[str, Any]:
    """
    .lq.Lobby.fetchAmuletActivityData           进入青云之志界面
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
                stage = value_changes.get("stage", -1)
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=GAME_STATE.ended)
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
            GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=ended)
            return "pass", None
        # type = 4: 换牌
        switch_event = next((e for e in events if e.get("type") == 4), None)
        if switch_event:
            value_changes = switch_event.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            used = round_info.get("used", {}).get("value", [])
            GAME_STATE.update_switch_used_tiles(used=used, push_gamestate=False)
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
                GAME_STATE.on_draw_tile(after_draw_hands[len(after_draw_hands) - 1], push_gamestate=False)

            GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended)
    # 进入青云之志界面时获取已经开始的游戏数据
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.fetchAmuletActivityData":
        data = view.get("data", {}).get("data", {})
        game = data.get("game", None)
        if game:
            round_info = game.get("round", {})
            hands = round_info.get("hands", [])
            pool = round_info.get("pool", [])
            locked_tiles = round_info.get("lockedTile", [])
            GAME_STATE.update_pool(pool, hand_tiles=hands, locked_tiles=locked_tiles, push_gamestate=False)
            desktop_remain = round_info.get("desktopRemain", 0)
            stage = game.get("stage", -1)
            ended = game.get("ended", False)

            if desktop_remain < 36:
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended, push_gamestate=False)
                GAME_STATE.refresh_wall_by_remaning()
            else:
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended)
    # 只是用来更新一下状态
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityStartGame":
        data = view.get("data", {})
        events = data.get("events", [])
        start_event = next((e for e in events if e.get("type") == 1), None)
        value_changes = start_event.get("valueChanges", {})
        stage = value_changes.get("stage", -1)
        ended = value_changes.get("ended", False)
        GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=ended)
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityBuy":
        data = dict(view["data"])
        events = data.get("events", [])
        buy_amulet_event = next((e for e in events if e.get("type") == 13), None)
        if buy_amulet_event:
            value_changes = buy_amulet_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=GAME_STATE.ended)
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivitySelectPack":
        data = view.get("data", {})
        events = data.get("events", [])
        select_amulet_event = next((e for e in events if e.get("type") == 14), None)
        if select_amulet_event:
            value_changes = select_amulet_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=GAME_STATE.ended)
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityEndShopping":
        data = view.get("data", {})
        events = data.get("events", [])
        end_shopping_event = next((e for e in events if e.get("type") == 22), None)
        if end_shopping_event:
            value_changes = end_shopping_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=GAME_STATE.ended)
    return "pass", None
