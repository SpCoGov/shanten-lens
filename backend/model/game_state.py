from __future__ import annotations

import asyncio
import json
from collections import OrderedDict
from dataclasses import dataclass, field
from typing import List, Dict


@dataclass
class GameState:
    """
    表示游戏状态（可序列化为 JSON）
    """
    stage: int = 0  # 1=选择免费卡包、2=换牌阶段、3=打牌阶段、4=卡包购买、5=卡包选择、6=关卡确认阶段、7=选择关卡奖励卡包
    deck_map: OrderedDict[int, str] = field(default_factory=OrderedDict)  # 牌山：id→牌面
    hand_tiles: List[int] = field(default_factory=list)  # 手牌
    dora_tiles: List[int] = field(default_factory=list)  # 宝牌指示牌（包含未翻开的）
    replacement_tiles: List[int] = field(default_factory=list)  # 替换牌（换牌阶段）
    wall_tiles: List[int] = field(default_factory=list)  # 牌山顺序（打牌阶段能摸到的）
    switch_used_tiles: List[int] = field(default_factory=list)  # 交换阶段交换到的牌
    ended: bool = field(default_factory=bool)  # 游戏是否结束
    desktop_remain: int = field(default_factory=int)  # 剩余可摸的牌
    locked_tiles: List[int] = field(default_factory=list)  # 被锁住的牌
    coin: int = field(default_factory=int)
    level: int = field(default_factory=int)
    effect_list: List[Dict] = field(default_factory=list)
    candidate_effect_list: List[Dict] = field(default_factory=list)
    record: Dict = field(default_factory=dict)
    ting_list: List[Dict] = field(default_factory=dict)
    # nextOperationType: 1=打牌、4=杠、8=自摸、100=跳过换牌、101=换牌（杠的时候会显示被杠的牌"gang": [{"tiles": [22,49,76,103]}]）
    next_operation: List[Dict] = field(default_factory=dict)
    goods: List[Dict] = field(default_factory=dict)
    refresh_price: int = field(default_factory=int)
    change_tile_count: int = field(default_factory=int)
    total_change_tile_count: int = field(default_factory=int)
    max_effect_volume: int = field(default_factory=int)
    boss_buff: List[int] = field(default_factory=list)

    update_reason: List[str] = field(default_factory=list)

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
            "switch_used_tiles": self.switch_used_tiles,
            "ended": self.ended,
            "desktop_remain": self.desktop_remain,  
            "locked_tiles": self.locked_tiles,
            "coin": self.coin,
            "level": self.level,
            "effect_list": self.effect_list,
            "candidate_effect_list": self.candidate_effect_list,
            "record": self.record,
            "ting_list": self.ting_list,
            "next_operation": self.next_operation,
            "goods": self.goods,
            "refresh_price": self.refresh_price,
            "change_tile_count": self.change_tile_count,
            "total_change_tile_count": self.total_change_tile_count,
            "max_effect_volume": self.max_effect_volume,
            "boss_buff": self.boss_buff,

            "update_reason": self.update_reason,
        }

    def to_json(self, *, indent: int | None = 2, ensure_ascii: bool = False) -> str:
        """
        序列化为 JSON 字符串（供前端使用）
        """
        return json.dumps(self.to_dict(), indent=indent, ensure_ascii=ensure_ascii)

    async def on_gamestage_change(self):
        from backend.app import broadcast
        await broadcast({"type": "update_gamestate", "data": self.to_dict()})
        self.update_reason.clear()

    def update_pool(self, pool: list[dict], hand_tiles: list[int], locked_tiles: list[int], push_gamestate: bool = True, reason: str = ""):
        self.deck_map.clear()
        self.hand_tiles.clear()
        self.dora_tiles.clear()
        self.replacement_tiles.clear()
        self.wall_tiles.clear()
        self.locked_tiles.clear()
        self.switch_used_tiles.clear()
        self.candidate_effect_list.clear()
        self.ended = True
        self.stage = -1
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

        self.update_reason.append(reason)
        if push_gamestate:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def update_wall(self, wall_tiles: List[int]):
        self.wall_tiles = wall_tiles.copy()

    def refresh_wall_by_remaning(self, push_gamestate: bool = True, reason: str = ""):
        temp = self.deck_map.copy()
        hand_tiles = self.hand_tiles.copy()
        for hand_tile_id in hand_tiles: 
            temp.pop(hand_tile_id)
        ids = list(temp.keys())
        # 跳过 dora 和 已经摸牌的数量
        cursor = 10 + 36 - self.desktop_remain - 1
        # 取后 剩余多少张 → wall
        self.wall_tiles = ids[cursor:cursor + self.desktop_remain]

        self.update_reason.append(reason)
        if push_gamestate:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def on_draw_tile(self, hand_tiles: list[int], tile_id: int, push_gamestate: bool = True, reason: str = ""):
        self.wall_tiles.remove(tile_id)
        self.hand_tiles = hand_tiles.copy()
        self.update_reason.append(reason)
        if push_gamestate:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def update_hand_tiles(self, hand_tiles: list[int], push_gamestate: bool = True, reason: str = ""):
        self.hand_tiles = hand_tiles.copy()
        self.update_reason.append(reason)
        if push_gamestate:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def update_switch_used_tiles(self, used: list[int], push_gamestate: bool = True, reason: str = ""):
        if self.stage == 2:
            self.switch_used_tiles = used.copy()

        self.update_reason.append(reason)
        if push_gamestate:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def update_other_info(self, desktop_remain: int = None, stage: int = None, ended: bool = None, coin: int = None, level: int = None,
                          effect_list: List[Dict] = None, candidate_effect_list: List[Dict] = None, ting_list: List[Dict] = None, next_operation: List[Dict] = None,
                          goods: List[Dict] = None, refresh_price: int = None, change_tile_count: int = None, total_change_tile_count: int = None, max_effect_volume: int = None,
                          boss_buff: List[int] = None,
                          push_gamestate: bool = True, reason: str = ""):
        if desktop_remain is not None:
            self.desktop_remain = desktop_remain
        if stage is not None:
            self.stage = stage
        if ended is not None:
            self.ended = ended
        if coin is not None:
            self.coin = coin
        if level is not None:
            self.level = level
        if effect_list is not None:
            self.effect_list = effect_list.copy()
        if candidate_effect_list is not None:
            self.candidate_effect_list = candidate_effect_list.copy()
        if ting_list is not None:
            self.ting_list = ting_list
        if next_operation is not None:
            self.next_operation = next_operation
        if goods is not None:
            self.goods = goods.copy()
        if refresh_price is not None:
            self.refresh_price = refresh_price
        if total_change_tile_count is not None:
            self.total_change_tile_count = total_change_tile_count
        if change_tile_count is not None:
            self.change_tile_count = change_tile_count
        if max_effect_volume is not None:
            self.max_effect_volume = max_effect_volume
        if boss_buff is not None:
            self.boss_buff = boss_buff
        self.update_reason.append(reason)
        if push_gamestate:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def on_giveup(self):
        self.stage = -1
        self.deck_map.clear()
        self.hand_tiles.clear()
        self.dora_tiles.clear()
        self.replacement_tiles.clear()
        self.wall_tiles.clear()
        self.switch_used_tiles.clear()
        self.ended = True
        self.desktop_remain = 0
        self.locked_tiles.clear()
        self.coin = 0
        self.level = 0
        self.effect_list.clear()
        self.candidate_effect_list.clear()
        self.record = {}
        self.refresh_price = 0
        self.total_change_tile_count = 0
        self.change_tile_count = 0
        self.goods.clear()
        self.next_operation.clear()
        self.ting_list.clear()
        self.boss_buff.clear()

        self.update_reason.clear()
        self.update_reason.append(".lq.Lobby.amuletActivityGiveup")
        loop = asyncio.get_running_loop()
        loop.create_task(self.on_gamestage_change())

    def update_record(self, record: dict):
        if not record or not isinstance(record, dict):
            return
        is_patch = any(
            isinstance(v, dict) and ("dirty" in v) and ("value" in v)
            for v in record.values()
        )
        if is_patch:
            for k, v in record.items():
                if isinstance(v, dict) and v.get("dirty") is True:
                    self.record[k] = v.get("value")
            return
        self.record = record
