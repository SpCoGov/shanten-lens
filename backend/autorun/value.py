from __future__ import annotations

from typing import Any, Dict, List, Optional, Tuple


RARITY_BASE_PRICE = {
    "GREEN": 3,
    "BLUE": 6,
    "ORANGE": 9,
    "PURPLE": 12,
}

GOLD_BADGE_ID = 600050
UNSTABLE_AMULET_REG_ID = 228


def extract_amulet_signature(effect_item: Dict[str, Any]) -> Tuple[int, bool, Optional[int]]:
    try:
        raw_id = int(effect_item.get("id", 0))
    except Exception:
        raw_id = 0
    reg_id = raw_id // 10
    is_plus = raw_id % 10 == 1

    badge = effect_item.get("badge")
    if isinstance(badge, dict) and "id" in badge:
        try:
            badge_id = int(badge["id"])
        except Exception:
            badge_id = None
    else:
        badge_id = None

    return reg_id, is_plus, badge_id


def reg_id_of_raw(raw_id: int) -> int:
    return int(raw_id) // 10


def candidate_badge_id(candidate: Dict[str, Any]) -> Optional[int]:
    try:
        badge_id = int(candidate.get("badgeId", 0))
        return badge_id if badge_id > 0 else None
    except Exception:
        return None


def target_value(target: Dict[str, Any]) -> int:
    try:
        return max(0, int(target.get("value", 1)))
    except Exception:
        return 1


def amulet_matches_target(effect_item: Dict[str, Any], target: Dict[str, Any]) -> bool:
    reg_id, is_plus, badge_id = extract_amulet_signature(effect_item)
    kind = target.get("kind")

    if kind == "badge":
        try:
            need_badge = int(target.get("id"))
        except Exception:
            return False
        return badge_id is not None and badge_id == need_badge

    if kind == "amulet":
        try:
            need_reg = int(target.get("id"))
        except Exception:
            return False
        if reg_id != need_reg:
            return False

        need_plus = bool(target.get("plus", False))
        target_badge = target.get("badge", None)
        need_badge = None
        if target_badge is not None and target_badge != "":
            try:
                need_badge = int(target_badge)
            except Exception:
                need_badge = None

        if need_badge is not None and badge_id != need_badge:
            return False
        return is_plus is True if need_plus else is_plus is False

    return False


def match_targets_for_amulet(effect_item: Dict[str, Any], targets: List[Dict[str, Any]]) -> List[int]:
    return [
        index
        for index, target in enumerate(targets or [])
        if amulet_matches_target(effect_item, target)
    ]


def calc_target_achievement_value(
        effect_list: List[Dict[str, Any]],
        targets: List[Dict[str, Any]],
) -> int:
    hit: set[int] = set()
    for item in effect_list or []:
        hit.update(match_targets_for_amulet(item, targets))

    total = 0
    for index in hit:
        try:
            total += target_value(targets[index])
        except Exception:
            continue
    return total


def is_needed_for_any_target(effect_item: Dict[str, Any], targets: List[Dict[str, Any]]) -> bool:
    reg_id, _is_plus, badge_id = extract_amulet_signature(effect_item)

    for target in targets or []:
        kind = target.get("kind")
        if kind == "badge":
            try:
                need_badge = int(target.get("id"))
            except Exception:
                continue
            if badge_id is not None and badge_id == need_badge:
                return True
            continue

        if kind == "amulet":
            try:
                need_reg = int(target.get("id"))
            except Exception:
                continue
            if reg_id == need_reg:
                return True
    return False


def _rarity_name_for_reg(amulet_registry: Any, reg_id: int) -> str:
    if amulet_registry is None:
        return ""
    try:
        item = amulet_registry.get(int(reg_id))
    except Exception:
        return ""
    rarity = getattr(item, "rarity", None)
    return str(getattr(rarity, "name", rarity) or "").upper()


def calc_amulet_price(
        *,
        raw_id: int,
        badge_id: Optional[int] = None,
        amulet_registry: Any = None,
) -> int:
    reg_id = reg_id_of_raw(raw_id)
    if reg_id == UNSTABLE_AMULET_REG_ID:
        return 0

    base_price = RARITY_BASE_PRICE.get(_rarity_name_for_reg(amulet_registry, reg_id), 0)
    if badge_id == GOLD_BADGE_ID:
        base_price *= 3
    return base_price


def calc_effect_price(effect_item: Dict[str, Any], *, amulet_registry: Any = None) -> int:
    try:
        raw_id = int(effect_item.get("id", 0) or 0)
    except Exception:
        raw_id = 0
    _reg_id, _is_plus, badge_id = extract_amulet_signature(effect_item)
    return calc_amulet_price(raw_id=raw_id, badge_id=badge_id, amulet_registry=amulet_registry)


def calc_candidate_price(candidate: Dict[str, Any], *, amulet_registry: Any = None) -> int:
    try:
        raw_id = int(candidate.get("id", 0) or 0)
    except Exception:
        raw_id = 0
    return calc_amulet_price(
        raw_id=raw_id,
        badge_id=candidate_badge_id(candidate),
        amulet_registry=amulet_registry,
    )


def total_volume(effect_list: List[Dict[str, Any]]) -> int:
    total = 0
    for item in effect_list or []:
        try:
            volume = int(item.get("volume", 0))
        except Exception:
            volume = 0
        total += max(0, volume)
    return total
