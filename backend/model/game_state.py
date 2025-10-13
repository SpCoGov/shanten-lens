from __future__ import annotations

import asyncio
import json
from loguru import logger
from collections import OrderedDict
from dataclasses import dataclass, field
from typing import List


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
    ended: bool = field(default_factory=bool)  # 游戏是否结束
    desktop_remain: int = field(default_factory=int)  # 剩余可摸的牌
    locked_tiles: List[int] = field(default_factory=list)  # 被锁住的牌

    def to_dict(self) -> dict:
        """
        转为 Python 原生字典（保持顺序）
        """
        return {
            "stage": self.stage,
            "deck_map": self.deck_map,  # 转为列表保持顺序
            "hand_tiles": self.hand_tiles,
            "dora_tiles": self.dora_tiles,
            "replacement_tiles": self.replacement_tiles,
            "wall_tiles": self.wall_tiles,
            "ended": self.ended,
            "desktop_remain": self.desktop_remain,
            "locked_tiles": self.locked_tiles,
        }

    def to_json(self, *, indent: int | None = 2, ensure_ascii: bool = False) -> str:
        """
        序列化为 JSON 字符串（供前端使用）
        """
        return json.dumps(self.to_dict(), indent=indent, ensure_ascii=ensure_ascii)

    async def on_gamestage_change(self):
        from backend.app import broadcast
        await broadcast({"type": "update_gamestate", "data": self.to_dict()})

    def update_pool(self, pool: list[dict], hand_tiles: list[int], locked_tiles: list[int], push_gamestage: bool = True):
        self.deck_map.clear()
        self.hand_tiles.clear()
        self.dora_tiles.clear()
        self.replacement_tiles.clear()
        self.wall_tiles.clear()
        self.locked_tiles.clear()
        for item in pool:
            self.deck_map[item["id"]] = item["tile"]
        temp = self.deck_map.copy()
        self.hand_tiles = hand_tiles.copy()
        for hand_tile_id in hand_tiles:
            temp.pop(hand_tile_id)
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

        self.locked_tiles = locked_tiles.copy()
        for locked_id in self.locked_tiles:
            self.wall_tiles.remove(locked_id)
        if push_gamestage:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def on_draw_tile(self, tile_id: int, push_gamestage: bool = True):
        self.wall_tiles.remove(tile_id)
        if push_gamestage:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def update_other_info(self, desktop_remain: int, stage: int, ended: bool, push_gamestage: bool = True):
        self.desktop_remain = desktop_remain
        self.stage = stage
        self.ended = ended
        if push_gamestage:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())
