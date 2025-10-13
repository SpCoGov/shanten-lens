from typing import Tuple, Any, Dict, List

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
        matched = next((e["event"] for e in events if e.get("type") == 23), None)
        if matched:
            value_changes = matched.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            hands = round_info.get("hands", {}).get("value", None)
            pool = round_info.get("pool", {}).get("value", None)
            if hands and pool:
                GAME_STATE.update_pool(pool, tehai=hands)

    return "pass", None
