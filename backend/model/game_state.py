from __future__ import annotations
from dataclasses import dataclass, field, asdict
from collections import OrderedDict
from typing import List
from loguru import logger
import json


@dataclass
class GameState:
    """
    表示游戏状态（可序列化为 JSON）
    """
    stage: int = 0  # 1=开始阶段、2=换牌阶段、3=打牌阶段、4=卡包购买、5=卡包选择、6=关卡确认阶段
    deck_map: OrderedDict[int, str] = field(default_factory=OrderedDict)  # 牌山：id→牌面
    hand_tiles: List[int] = field(default_factory=list)  # 手牌
    dora_tiles: List[int] = field(default_factory=list)  # 宝牌指示牌（包含未翻开的）
    replacement_tiles: List[int] = field(default_factory=list)  # 替换牌（换牌阶段）
    wall_tiles: List[int] = field(default_factory=list)  # 牌山顺序（打牌阶段能摸到的）
    dropped_tiles: List[int] = field(default_factory=list)  # 打出的牌
    ended: bool = field(default_factory=bool)  # 游戏是否结束

    def to_dict(self) -> dict:
        """
        转为 Python 原生字典（保持顺序）
        """
        return {
            "stage": self.stage,
            "deck_map": list(self.deck_map.items()),  # 转为列表保持顺序
            "hand_tiles": self.hand_tiles,
            "dora_tiles": self.dora_tiles,
            "replacement_tiles": self.replacement_tiles,
            "wall_tiles": self.wall_tiles,
            "dropped_tiles": self.dropped_tiles,
            "ended": self.ended,
        }

    def to_json(self, *, indent: int | None = 2, ensure_ascii: bool = False) -> str:
        """
        序列化为 JSON 字符串（供前端使用）
        """
        return json.dumps(self.to_dict(), indent=indent, ensure_ascii=ensure_ascii)

    @classmethod
    def from_json(cls, data: str | dict) -> GameState:
        """
        从 JSON（字符串或字典）反序列化为 GameState 实例
        """
        if isinstance(data, str):
            data = json.loads(data)

        deck_items = data.get("deck_map", [])
        deck = OrderedDict((int(k), str(v)) for k, v in deck_items)

        return cls(
            stage=int(data.get("stage", 0)),
            deck_map=deck,
            hand_tiles=[int(x) for x in data.get("hand_tiles", [])],
            dora_tiles=data.get("dora_tiles", []),
            replacement_tiles=[int(x) for x in data.get("replacement_tiles", [])],
            wall_tiles=[int(x) for x in data.get("wall_tiles", [])],
            dropped_tiles=data.get("dropped_tiles", []),
            ended=data.get("ended", False),
        )

    def update_pool(self, pool: list[dict], tehai: list[int]):
        self.deck_map.clear()
        self.hand_tiles.clear()
        self.dora_tiles.clear()
        self.replacement_tiles.clear()
        self.wall_tiles.clear()
        self.dropped_tiles.clear()
        for item in pool:
            self.deck_map[item["id"]] = item["tile"]
        temp = self.deck_map.copy()
        for tehai_id in tehai:
            temp.pop(tehai_id)
        ids = list(temp.keys())
        cursor = 0

        # 取前 10 张 → dora
        self.dora_tiles = ids[cursor:cursor + 10]
        cursor += 10

        # 取后 36 张 → wall
        self.wall_tiles = ids[cursor:cursor + 36]
        cursor += 36

        # 剩下全部 → replacement
        self.replacement_tiles = ids[cursor:]

    def update_dropped_tiles(self, tiles: list[int]):
        self.dropped_tiles = list(tiles)