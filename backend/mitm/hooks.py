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
    """
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
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityUpgrade":
        data = view.get("data", {})
        events = data.get("events", [])
        matched = next((e for e in events if e.get("type") == 23), None)
        if matched:
            value_changes = matched.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            hands = round_info.get("hands", {}).get("value", None)
            pool = round_info.get("pool", {}).get("value", None)
            locked_tiles = round_info.get("lockedTile", {}).get("value", None)
            if hands and pool:
                GAME_STATE.update_pool(pool, hand_tiles=hands, locked_tiles=locked_tiles, push_gamestage=False)
                desktop_remain = round_info.get("desktopRemain", {}).get("value", 0)
                stage = value_changes.get("stage", -1)
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=GAME_STATE.ended)
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityOperate":
        data = view.get("data", {})
        events = data.get("events", [])
        end_event = next((e for e in events if e.get("type") == 100), None)
        if end_event:
            value_changes = end_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            ended = value_changes.get("ended", True)
            GAME_STATE.update_other_info(desktop_remain=GAME_STATE.desktop_remain, stage=stage, ended=ended)
            return "pass", None
        draw_event = next((e for e in events if e.get("type") == 6), None)
        if draw_event:
            value_changes = draw_event.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            desktop_remain = round_info.get("desktopRemain", {}).get("value", 0)
            stage = value_changes.get("stage", -1)
            ended = value_changes.get("ended", False)

            after_draw_hands = draw_event.get("valueChanges", {}).get("round", {}).get("hands", {}).get("value", None)
            if after_draw_hands:
                GAME_STATE.on_draw_tile(after_draw_hands[len(after_draw_hands) - 1], push_gamestage=False)

            GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended)

    return "pass", None
