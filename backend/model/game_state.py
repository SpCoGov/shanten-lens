from __future__ import annotations

import asyncio
import json
from collections import OrderedDict
from dataclasses import dataclass, field
from typing import Dict, List


@dataclass
class GameState:
    """
    表示游戏状态（可序列化为 JSON）
    """
    stage: int = -1  # 1=选择免费卡包、2=换牌阶段、3=打牌阶段、4=卡包购买、5=卡包选择、6=关卡确认阶段、7=选择关卡奖励卡包
    deck_map: OrderedDict[int, str] = field(default_factory=OrderedDict)  # 牌山：id→牌面
    hand_tiles: List[int] = field(default_factory=list)  # 手牌
    dora_tiles: List[int] = field(default_factory=list)  # 宝牌指示牌（包含未翻开的）
    tian_dora_tiles: List[str] = field(default_factory=list)  # 魂牌
    replacement_tiles: List[int] = field(default_factory=list)  # 替换牌（换牌阶段）
    wall_tiles: List[int] = field(default_factory=list)  # 牌山顺序（打牌阶段能摸到的）
    switch_used_tiles: List[int] = field(default_factory=list)  # 交换阶段交换到的牌
    ended: bool = field(default_factory=bool)  # 游戏是否结束
    desktop_remain: int = field(default_factory=int)  # 剩余可摸的牌
    locked_tiles: List[int] = field(default_factory=list)  # 被锁住的牌
    coin: int = field(default_factory=int)
    point: int = field(default_factory=int)
    target_point: int = field(default_factory=int)
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
    tile_score_map: Dict[str, str] = field(default_factory=dict)
    opening_hand_tiles: List[int] = field(default_factory=list)
    used_desktop_tiles: List[int] = field(default_factory=list)

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
            "tian_dora_tiles": self.tian_dora_tiles,
            "replacement_tiles": self.replacement_tiles,
            "wall_tiles": self.wall_tiles,
            "switch_used_tiles": self.switch_used_tiles,
            "ended": self.ended,
            "desktop_remain": self.desktop_remain,
            "locked_tiles": self.locked_tiles,
            "coin": str(self.coin),
            "point": str(self.point),
            "target_point": str(self.target_point),
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
            "tile_score_map": self.tile_score_map,

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

    def _infer_opening_hand_tiles(
            self, current_hand_tiles: list[int], dora_tiles: list[int] | None = None
    ) -> List[int]:
        ids = list(self.deck_map.keys())
        if not ids:
            return []

        dora_hint = list(dora_tiles or [])
        if dora_hint:
            first_dora_id = dora_hint[0]
            try:
                dora_idx = ids.index(first_dora_id)
            except ValueError:
                dora_idx = -1
            if dora_idx >= 0:
                opening_hand_tiles = ids[:dora_idx]
                last_id = ids[-1]
                if (
                        self.deck_map.get(last_id) == "bd"
                        and last_id in (current_hand_tiles or [])
                        and last_id not in opening_hand_tiles
                ):
                    opening_hand_tiles.append(last_id)
                return opening_hand_tiles

        return list(current_hand_tiles or [])

    def _candidate_pool_ids(self) -> List[int]:
        # 从 pool 中剔除开局手牌的 id，剩余顺序与协议保持一致
        hand_set = set(self.opening_hand_tiles)
        return [tile_id for tile_id in self.deck_map.keys() if tile_id not in hand_set]

    def _rebuild_sections_from_pool(self):
        # 协议定义：从 pool 排除 hand 后，前 10 张是 dora，再后 36 张是 wall，剩下的是 replacement
        ids = self._candidate_pool_ids()
        self.dora_tiles = ids[:10]
        self.wall_tiles = ids[10:46]
        self.replacement_tiles = ids[46:]

    def update_pool(
            self,
            pool: list[dict],
            hand_tiles: list[int],
            locked_tiles: list[int],
            used: list[int],
            dora_tiles: list[int] | None = None,
            used_desktop: list[int] | None = None,
            push_gamestate: bool = True,
            reason: str = "",
    ):
        self.deck_map.clear()
        self.hand_tiles.clear()
        self.dora_tiles.clear()
        self.tian_dora_tiles.clear()
        self.replacement_tiles.clear()
        self.wall_tiles.clear()
        self.locked_tiles.clear()
        self.opening_hand_tiles.clear()
        self.used_desktop_tiles.clear()
        self.switch_used_tiles.clear()
        self.candidate_effect_list.clear()
        self.ended = True
        self.stage = -1
        # 根据池子信息构建完整的牌堆（id → 牌面）
        for item in pool:
            self.deck_map[item["id"]] = item["tile"]

        self.hand_tiles = hand_tiles.copy()
        self.opening_hand_tiles = self._infer_opening_hand_tiles(self.hand_tiles, dora_tiles)
        self.used_desktop_tiles = used_desktop.copy() if used_desktop else []

        # 按协议从 pool 中排除手牌后重新切 dora / wall / replacement
        self._rebuild_sections_from_pool()

        # lockedTile 一定是牌山的子集，但这里仍做容错，避免异常中断整个 hook
        self.locked_tiles = locked_tiles.copy() if locked_tiles else []
        if self.locked_tiles:
            locked_set = set(self.locked_tiles)
            self.wall_tiles = [tile_id for tile_id in self.wall_tiles if tile_id not in locked_set]
        if self.used_desktop_tiles:
            used_desktop_set = set(self.used_desktop_tiles)
            self.wall_tiles = [
                tile_id for tile_id in self.wall_tiles if tile_id not in used_desktop_set
            ]

        self.update_reason.append(reason)
        if push_gamestate:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def update_wall(self, wall_tiles: List[int]):
        self.wall_tiles = wall_tiles.copy()

    def refresh_wall_by_remaning(self, push_gamestate: bool = True, reason: str = ""):
        if self.locked_tiles:
            _locked = len(self.locked_tiles)
        # wall tiles 里保存的是“当前还可能摸到的未锁牌”，因此直接按 desktop_remain 对齐即可
        remain = self.desktop_remain
        # 优先裁剪当前 wall（可能已被外部重排）
        if isinstance(self.wall_tiles, list) and self.desktop_remain is not None and len(self.wall_tiles) >= int(remain):
            self.wall_tiles = self.wall_tiles[-int(remain):]
        else:
            # fallback：从 deck_map 按与 update_pool 一致的规则重新推导
            self._rebuild_sections_from_pool()
            if self.locked_tiles:
                locked_set = set(self.locked_tiles)
                self.wall_tiles = [
                    tile_id for tile_id in self.wall_tiles if tile_id not in locked_set
                ]
            if self.used_desktop_tiles:
                used_desktop_set = set(self.used_desktop_tiles)
                self.wall_tiles = [
                    tile_id for tile_id in self.wall_tiles if tile_id not in used_desktop_set
                ]
            self.wall_tiles = self.wall_tiles[-int(remain):]

        if remain <= 0:
            self.wall_tiles = []

        self.update_reason.append(reason)
        if push_gamestate:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def on_draw_tile(self, hand_tiles: list[int], tile_id: int, push_gamestate: bool = True, reason: str = ""):
        # 正常情况下摸到的牌一定在 wall 里；这里保留容错避免状态不同步时直接抛异常
        if tile_id in self.wall_tiles:
            self.wall_tiles.remove(tile_id)
        self.hand_tiles = hand_tiles.copy()
        self.update_reason.append(reason)
        if push_gamestate:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def update_hand_tiles(
            self, hand_tiles: list[int], push_gamestate: bool = True, reason: str = ""
    ):
        self.hand_tiles = hand_tiles.copy()
        self.update_reason.append(reason)
        if push_gamestate:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def update_switch_used_tiles(
            self, used: list[int], push_gamestate: bool = True, reason: str = ""
    ):
        if self.stage == 2:
            self.switch_used_tiles = used.copy()

        self.update_reason.append(reason)
        if push_gamestate:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def update_other_info(
            self,
            desktop_remain: int = None,
            stage: int = None,
            ended: bool = None,
            coin: int = None,
            point: int = None,
            target_point: int = None,
            level: int = None,
            effect_list: List[Dict] = None,
            candidate_effect_list: List[Dict] = None,
            ting_list: List[Dict] = None,
            next_operation: List[Dict] = None,
            goods: List[Dict] = None,
            refresh_price: int = None,
            change_tile_count: int = None,
            total_change_tile_count: int = None,
            max_effect_volume: int = None,
            boss_buff: List[int] = None,
            tile_score_map: Dict[str, str] = None,
            tian_dora_tiles: List[str] = None,
            push_gamestate: bool = True,
            reason: str = "",
    ):
        if desktop_remain is not None:
            self.desktop_remain = desktop_remain
        if stage is not None:
            self.stage = stage
        if ended is not None:
            self.ended = ended
        if coin is not None:
            self.coin = coin
        if point is not None:
            self.point = point
        if target_point is not None:
            self.target_point = target_point
        if level is not None:
            self.level = level
        if effect_list is not None:
            self.effect_list = effect_list.copy()
            # for e in self.effect_list:
            #     if isinstance(e, dict):
            #         badge = e.get("badge")
            #         if not isinstance(badge, dict):
            #             badge = {}
            #             e["badge"] = badge
            #         badge["id"] = 600170
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
        if tile_score_map is not None:
            self.tile_score_map = tile_score_map.copy()
        if tian_dora_tiles is not None:
            self.tian_dora_tiles = tian_dora_tiles.copy()
        self.update_reason.append(reason)
        if push_gamestate:
            loop = asyncio.get_running_loop()
            loop.create_task(self.on_gamestage_change())

    def on_giveup(self):
        self.stage = -1
        self.deck_map.clear()
        self.hand_tiles.clear()
        self.dora_tiles.clear()
        self.tian_dora_tiles.clear()
        self.replacement_tiles.clear()
        self.wall_tiles.clear()
        self.switch_used_tiles.clear()
        self.ended = True
        self.desktop_remain = 0
        self.locked_tiles.clear()
        self.opening_hand_tiles.clear()
        self.used_desktop_tiles.clear()
        self.coin = 0
        self.point = 0
        self.target_point = 0
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
        self.tile_score_map.clear()

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
