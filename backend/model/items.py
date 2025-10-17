from __future__ import annotations
from dataclasses import dataclass, asdict
from enum import Enum
from typing import Dict, List, Iterable, Optional, Any
import json


class AmuletRarity(Enum):
    GREEN = 1
    BLUE = 2
    ORANGE = 3
    PURPLE = 4


class BadgeRarity(Enum):
    BROWN = 1
    BLUE = 2
    RED = 3


@dataclass(frozen=True)
class Amulet:
    id: int  # 物品ID
    icon_id: int  # 图片ID（或资源ID）
    name: str  # 名称
    rarity: AmuletRarity


@dataclass(frozen=True)
class Badge:
    id: int
    icon_id: int
    name: str
    rarity: BadgeRarity


class _BaseRegistry:
    def __init__(self):
        self._by_id: Dict[int, Any] = {}
        self._by_name: Dict[str, Any] = {}

    def _check_unique(self, item_id: int, name: str):
        if item_id in self._by_id:
            raise ValueError(f"重复的 id: {item_id}")
        key = name.strip().lower()
        if key in self._by_name:
            raise ValueError(f"重复的名称: {name}")

    def __len__(self):
        return len(self._by_id)

    def all(self) -> List[Any]:
        return list(self._by_id.values())

    def get(self, item_id: int) -> Optional[Any]:
        return self._by_id.get(item_id)

    def get_by_name(self, name: str) -> Optional[Any]:
        return self._by_name.get(name.strip().lower())

    def exists(self, item_id: int) -> bool:
        return item_id in self._by_id


class AmuletRegistry(_BaseRegistry):
    def add(self, item: Amulet):
        self._check_unique(item.id, item.name)
        self._by_id[item.id] = item
        self._by_name[item.name.strip().lower()] = item

    def add_many(self, items: Iterable[Amulet]):
        for it in items:
            self.add(it)

    def list_by_rarity(self, rarity: AmuletRarity) -> List[Amulet]:
        return [a for a in self._by_id.values() if a.rarity == rarity]

    def to_json_obj(self) -> List[Dict[str, Any]]:
        data: List[Dict[str, Any]] = []
        for a in self._by_id.values():
            d = asdict(a)
            d["rarity"] = a.rarity.name
            data.append(d)
        data.sort(key=lambda x: x["id"])
        return data

    def to_json_str(self, ensure_ascii=False, indent=2) -> str:
        return json.dumps(self.to_json_obj(), ensure_ascii=ensure_ascii, indent=indent)

    @staticmethod
    def from_json_obj(obj: Any) -> "AmuletRegistry":
        reg = AmuletRegistry()
        if not isinstance(obj, list):
            raise ValueError("护身符 JSON 顶层必须是数组")
        for row in obj:
            rarity = row["rarity"]
            if isinstance(rarity, str):
                rarity_enum = AmuletRarity[rarity]
            else:
                rarity_enum = AmuletRarity(rarity)
            reg.add(Amulet(
                id=int(row["id"]),
                icon_id=int(row["icon_id"]),
                name=str(row["name"]),
                rarity=rarity_enum,
            ))
        return reg

    @staticmethod
    def from_json_str(s: str) -> "AmuletRegistry":
        return AmuletRegistry.from_json_obj(json.loads(s))


class BadgeRegistry(_BaseRegistry):
    def add(self, item: Badge):
        self._check_unique(item.id, item.name)
        self._by_id[item.id] = item
        self._by_name[item.name.strip().lower()] = item

    def add_many(self, items: Iterable[Badge]):
        for it in items:
            self.add(it)

    def list_by_rarity(self, rarity: BadgeRarity) -> List[Badge]:
        return [s for s in self._by_id.values() if s.rarity == rarity]

    def to_json_obj(self) -> List[Dict[str, Any]]:
        data: List[Dict[str, Any]] = []
        for s_ in self._by_id.values():
            d = asdict(s_)
            d["rarity"] = s_.rarity.name
            data.append(d)
        data.sort(key=lambda x: x["id"])
        return data

    def to_json_str(self, ensure_ascii=False, indent=2) -> str:
        return json.dumps(self.to_json_obj(), ensure_ascii=ensure_ascii, indent=indent)

    @staticmethod
    def from_json_obj(obj: Any) -> "BadgeRegistry":
        reg = BadgeRegistry()
        if not isinstance(obj, list):
            raise ValueError("印章 JSON 顶层必须是数组")
        for row in obj:
            rarity = row["rarity"]
            if isinstance(rarity, str):
                rarity_enum = BadgeRarity[rarity]
            else:
                rarity_enum = BadgeRarity(rarity)
            reg.add(Badge(
                id=int(row["id"]),
                icon_id=int(row["icon_id"]),
                name=str(row["name"]),
                rarity=rarity_enum,
            ))
        return reg

    @staticmethod
    def from_json_str(s: str) -> "BadgeRegistry":
        return BadgeRegistry.from_json_obj(json.loads(s))
