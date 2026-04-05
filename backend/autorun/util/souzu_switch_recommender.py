import time
import os
import signal
import threading
import traceback
from concurrent.futures import ProcessPoolExecutor, wait, FIRST_COMPLETED
from concurrent.futures.process import BrokenProcessPool
from collections import Counter, defaultdict
from dataclasses import dataclass
from functools import lru_cache
from itertools import combinations
from typing import Callable, Dict, Iterable, List, Optional, Sequence, Set, Tuple
from loguru import logger

RED_MAP = {"0m": "5m", "0p": "5p", "0s": "5s"}
ALL_TILES = [f"{n}{s}" for s in "mps" for n in range(1, 10)] + [f"{n}z" for n in range(1, 8)]
TILE_INDEX = {tile: idx for idx, tile in enumerate(ALL_TILES)}
_ACTIVE_EXECUTORS: set[ProcessPoolExecutor] = set()
_ACTIVE_EXECUTORS_LOCK = threading.Lock()


@dataclass(frozen=True)
class PoolEntry:
    order: int
    tile_id: int
    face: str
    source: str
    source_index: int


def _norm(tile: str) -> str:
    return RED_MAP.get(tile, tile)


def _parse_face(face: str) -> tuple[int, str]:
    return int(face[0]), face[1]


def _candidate_ids(ids: Iterable[int], deck_map: Dict[int, str]) -> List[int]:
    def key(tile_id: int) -> tuple[int, int, int]:
        raw = deck_map[tile_id]
        norm = _norm(raw)
        is_red = 1 if raw in RED_MAP else 0
        return is_red, TILE_INDEX.get(norm, 99), tile_id

    return sorted(ids, key=key)


def _entry_label(entry: PoolEntry) -> str:
    source_map = {"hand": "手牌", "replacement": "换牌堆", "wall": "牌山"}
    if entry.source == "hand":
        obtain_text = f"取得=起手#{entry.source_index + 1}"
    elif entry.source == "replacement":
        obtain_text = f"取得=换入#{entry.source_index + 1}"
    else:
        obtain_text = f"取得=摸到#{entry.source_index + 1}"
    return (
        f"{entry.face}(id={entry.tile_id},来源={source_map.get(entry.source, entry.source)},"
        f"来源序号={entry.source_index + 1},{obtain_text})"
    )


def _quad_label(ids: Sequence[int], by_id: Dict[int, PoolEntry]) -> str:
    if not ids:
        return "-"
    face = by_id[ids[0]].face
    return f"{face} <- " + " / ".join(_entry_label(by_id[tile_id]) for tile_id in ids)


def _quad_tile_positions(ids: Sequence[int], by_id: Dict[int, PoolEntry]) -> List[dict]:
    result: List[dict] = []
    for tile_id in ids:
        entry = by_id[tile_id]
        result.append({
            "tile_id": entry.tile_id,
            "source": entry.source,
            "source_index": entry.source_index + 1,
        })
    return result


def _quad_score_text(time_key: tuple[int, int, int]) -> str:
    return f"总序号={time_key[0]} / 换牌深度={time_key[1]} / 牌山深度={time_key[2]}"


def _component_text(parts: Sequence[str]) -> str:
    return " | ".join(parts) if parts else "尚未选定组合"


def _chiitoi_win(tiles: List[str]) -> bool:
    if len(tiles) != 14:
        return False
    cnt = Counter(t for t in tiles if t != "bd")
    jokers = tiles.count("bd")
    pair_kinds = sum(1 for v in cnt.values() if v >= 2)
    single_kinds = sum(1 for v in cnt.values() if v == 1)
    need_pairs = max(0, 7 - pair_kinds)
    use_on_singles = min(need_pairs, single_kinds)
    need_jokers = use_on_singles + (need_pairs - use_on_singles) * 2
    return jokers >= need_jokers


@lru_cache(maxsize=None)
def _can_form_melds(counts: tuple[int, ...], jokers: int) -> bool:
    total = sum(counts)
    if total == 0:
        return jokers % 3 == 0

    first = next(i for i, v in enumerate(counts) if v > 0)

    triplet_take = min(3, counts[first])
    triplet_need = 3 - triplet_take
    if triplet_need <= jokers:
        nxt = list(counts)
        nxt[first] -= triplet_take
        if _can_form_melds(tuple(nxt), jokers - triplet_need):
            return True

    if first < 27 and first % 9 <= 6:
        nxt = list(counts)
        nxt[first] -= 1
        need = 0
        for off in (1, 2):
            idx = first + off
            if nxt[idx] > 0:
                nxt[idx] -= 1
            else:
                need += 1
        if need <= jokers and _can_form_melds(tuple(nxt), jokers - need):
            return True

    return False


def _standard_win(tiles: List[str]) -> bool:
    counts = [0] * len(ALL_TILES)
    jokers = 0
    for tile in tiles:
        if tile == "bd":
            jokers += 1
        else:
            counts[TILE_INDEX[tile]] += 1

    base = tuple(counts)
    if jokers >= 2 and _can_form_melds(base, jokers - 2):
        return True

    for idx, have in enumerate(base):
        if have >= 2:
            nxt = list(base)
            nxt[idx] -= 2
            if _can_form_melds(tuple(nxt), jokers):
                return True
        if have >= 1 and jokers >= 1:
            nxt = list(base)
            nxt[idx] -= 1
            if _can_form_melds(tuple(nxt), jokers - 1):
                return True
    return False


def _is_win_14(tiles: List[str]) -> bool:
    return _standard_win(tiles) or _chiitoi_win(tiles)


def _waits_for_hand13_faces(hand13: List[str]) -> List[str]:
    cnt = Counter(t for t in hand13 if t != "bd")
    waits: List[str] = []
    for face in ALL_TILES:
        if cnt[face] >= 4:
            continue
        if _is_win_14(hand13 + [face]):
            waits.append(face)
    return waits


def _is_open_two_melds_pair_win(tiles8: List[str]) -> bool:
    if len(tiles8) != 8:
        return False
    counts = [0] * len(ALL_TILES)
    jokers = 0
    for tile in tiles8:
        if tile == "bd":
            jokers += 1
        else:
            counts[TILE_INDEX[tile]] += 1

    base = tuple(counts)
    if jokers >= 2 and _can_form_melds(base, jokers - 2):
        return True

    for idx, have in enumerate(base):
        if have >= 2:
            nxt = list(base)
            nxt[idx] -= 2
            if _can_form_melds(tuple(nxt), jokers):
                return True
        if have >= 1 and jokers >= 1:
            nxt = list(base)
            nxt[idx] -= 1
            if _can_form_melds(tuple(nxt), jokers - 1):
                return True
    return False


def _waits_for_open_two_melds_faces(hand7: List[str], used_face_cnt: Optional[Counter[str]] = None) -> List[str]:
    if len(hand7) != 7:
        return []
    used_face_cnt = used_face_cnt or Counter()
    hand_cnt = Counter(t for t in hand7 if t != "bd")
    waits: List[str] = []
    for face in ALL_TILES:
        if hand_cnt[face] + used_face_cnt[face] >= 4:
            continue
        if _is_open_two_melds_pair_win(hand7 + [face]):
            waits.append(face)
    return waits


def _is_exact_meld3(faces3: Sequence[str]) -> bool:
    if len(faces3) != 3:
        return False
    if faces3[0] == faces3[1] == faces3[2]:
        return True
    ordered = sorted(faces3, key=lambda tile: TILE_INDEX[tile])
    a, b, c = ordered
    ra, sa = _parse_face(a)
    rb, sb = _parse_face(b)
    rc, sc = _parse_face(c)
    return sa in "mps" and sa == sb == sc and rb == ra + 1 and rc == rb + 1


def _non_souzu_pair_wait_shape_reason(hand7: Sequence[str]) -> Optional[str]:
    cnt = Counter(hand7)
    pair_faces = sorted([face for face, n in cnt.items() if n >= 2], key=lambda tile: TILE_INDEX[tile])
    for i, face_b in enumerate(pair_faces):
        for face_c in pair_faces[i + 1:]:
            tmp = cnt.copy()
            tmp[face_b] -= 2
            tmp[face_c] -= 2
            leftover = []
            for face, n in tmp.items():
                leftover.extend([face] * n)
            if len(leftover) != 3:
                continue
            if not _is_exact_meld3(leftover):
                continue
            if not face_b.endswith("s") or not face_c.endswith("s"):
                shape = "".join(sorted(leftover, key=lambda tile: TILE_INDEX[tile]))
                return (
                    f"鍘绘潬鍚?寮犲憟鐜?1 闈㈠瓙 + 2 瀵瑰瓙褰紝涓斿瀛愬惈闈炴潯瀛? "
                    f"闈㈠瓙={shape}, 瀵瑰瓙={face_b}/{face_c}"
                )
    return None


def _build_pool(
        deck_map: Dict[int, str],
        hand_ids: Sequence[int],
        replacement_ids: Sequence[int],
        wall_ids: Sequence[int],
) -> tuple[List[PoolEntry], Dict[int, PoolEntry]]:
    entries: List[PoolEntry] = []
    by_id: Dict[int, PoolEntry] = {}
    for source, ids in (("hand", hand_ids), ("replacement", replacement_ids), ("wall", wall_ids)):
        for source_index, tile_id in enumerate(ids):
            entry = PoolEntry(
                order=len(entries),
                tile_id=int(tile_id),
                face=_norm(deck_map[int(tile_id)]),
                source=source,
                source_index=source_index,
            )
            entries.append(entry)
            by_id[entry.tile_id] = entry
    return entries, by_id


def _limit_replacement_search_window(
        replacement_ids: Sequence[int],
        remaining_changes: int,
        per_change_limit: int,
) -> List[int]:
    if remaining_changes <= 0 or per_change_limit <= 0:
        return []
    max_read = min(len(replacement_ids), remaining_changes * per_change_limit)
    return [int(tile_id) for tile_id in replacement_ids[:max_read]]


def _quad_time(entry_ids: Sequence[int], by_id: Dict[int, PoolEntry]) -> tuple[int, int, int]:
    entries = [by_id[tile_id] for tile_id in entry_ids]
    return (
        max(entry.order for entry in entries),
        max((entry.source_index + 1 for entry in entries if entry.source == "replacement"), default=0),
        max((entry.source_index + 1 for entry in entries if entry.source == "wall"), default=0),
    )


def _enumerate_quads(pool: Sequence[PoolEntry], by_id: Dict[int, PoolEntry]) -> List[dict]:
    face_map: Dict[str, List[int]] = defaultdict(list)
    for entry in pool:
        face_map[entry.face].append(entry.tile_id)

    quads: List[dict] = []
    for face, ids in face_map.items():
        if len(ids) < 4:
            continue
        for combo in combinations(ids, 4):
            ids4 = tuple(sorted(combo))
            quads.append({
                "face": face,
                "ids": ids4,
                "time_key": _quad_time(ids4, by_id),
            })
    quads.sort(key=lambda item: (item["time_key"], TILE_INDEX.get(item["face"], 99), item["ids"]))
    return quads


def _enumerate_quad_pairs(quads: Sequence[dict]) -> List[dict]:
    pairs: List[dict] = []
    for i in range(len(quads)):
        for j in range(i + 1, len(quads)):
            if quads[i]["face"] == quads[j]["face"]:
                continue
            first = quads[i]
            second = quads[j]
            if second["time_key"] < first["time_key"]:
                first, second = second, first
            score = (
                first["time_key"][0],
                second["time_key"][0],
                first["time_key"][1],
                second["time_key"][1],
                first["time_key"][2],
                second["time_key"][2],
                TILE_INDEX.get(first["face"], 99),
                TILE_INDEX.get(second["face"], 99),
            )
            pairs.append({
                "faces": (first["face"], second["face"]),
                "quad_ids": tuple(first["ids"] + second["ids"]),
                "pair_score": score,
            })
    pairs.sort(key=lambda item: (item["pair_score"], item["quad_ids"]))
    return pairs


def _build_debug_pool_info(
        deck_map: Dict[int, str],
        hand_ids: Sequence[int],
        remaining_replacements: Sequence[int],
        wall_ids: Sequence[int],
        quads: Sequence[dict],
        by_id: Dict[int, PoolEntry],
        *,
        replacement_total_remaining: int,
        replacement_window_count: int,
        replacement_used_count: int,
        focus_face: str = "5s",
) -> dict:
    pool_ids = list(hand_ids) + list(remaining_replacements) + list(wall_ids)
    raw_counts = Counter(deck_map[int(tile_id)] for tile_id in pool_ids)
    norm_counts = Counter(_norm(deck_map[int(tile_id)]) for tile_id in pool_ids)

    focus_entries: List[dict] = []
    for tile_id in pool_ids:
        raw_face = deck_map[int(tile_id)]
        norm_face = _norm(raw_face)
        if norm_face != focus_face:
            continue
        entry = by_id[int(tile_id)]
        focus_entries.append({
            "tile_id": int(tile_id),
            "raw_face": raw_face,
            "norm_face": norm_face,
            "source": entry.source,
            "source_index": entry.source_index + 1,
        })

    available_quads: List[dict] = []
    for quad in quads:
        available_quads.append({
            "face": quad["face"],
            "tile_positions": _quad_tile_positions(quad["ids"], by_id),
        })

    return {
        "focus_face": focus_face,
        "focus_count": norm_counts.get(focus_face, 0),
        "raw_focus_counts": {
            "0s": raw_counts.get("0s", 0),
            "5s": raw_counts.get("5s", 0),
        },
        "pool_counts": {
            "hand": len(hand_ids),
            "replacement_window": len(remaining_replacements),
            "wall": len(wall_ids),
            "total": len(pool_ids),
        },
        "replacement_window": {
            "used_count": replacement_used_count,
            "total_remaining": replacement_total_remaining,
            "window_count": replacement_window_count,
        },
        "focus_entries": focus_entries,
        "norm_face_counts": dict(sorted(norm_counts.items(), key=lambda item: TILE_INDEX.get(item[0], 99))),
        "available_quads": available_quads,
    }


def _build_switch_batches(
        hand_ids: Sequence[int],
        consumed_ids: Sequence[int],
        target_face_counts: Counter[str],
        deck_map: Dict[int, str],
        remaining_changes: int,
        per_change_limit: int,
) -> Optional[List[int]]:
    c = len(consumed_ids)
    if c == 0:
        return []

    faces = tuple(sorted(target_face_counts.keys(), key=lambda tile: TILE_INDEX.get(tile, 99)))
    suffix_counts: List[Counter[str]] = [Counter() for _ in range(c + 1)]
    running = Counter()
    for idx in range(c - 1, -1, -1):
        face = _norm(deck_map[consumed_ids[idx]])
        if face in target_face_counts:
            running[face] += 1
        suffix_counts[idx] = running.copy()

    def required_keep_count(pos: int) -> int:
        future = suffix_counts[pos]
        return sum(max(0, target_face_counts[face] - future.get(face, 0)) for face in faces)

    @lru_cache(maxsize=None)
    def solve(pos: int, ops_left: int) -> Optional[Tuple[int, ...]]:
        if pos == c:
            return ()
        if ops_left <= 0:
            return None
        available = max(0, 13 - required_keep_count(pos))
        if available <= 0:
            return None
        max_batch = min(per_change_limit, c - pos, available)
        for batch in range(1, max_batch + 1):
            rest = solve(pos + batch, ops_left - 1)
            if rest is not None:
                return (batch,) + rest
        return None

    result = solve(0, remaining_changes)
    return list(result) if result is not None else None


def _simulate_switch_reachability(
        hand_ids: Sequence[int],
        occupied_ids: Sequence[int],
        replacement_ids: Sequence[int],
        current_change_count: int,
        max_change_count: int,
        per_change_limit: int,
        deck_map: Dict[int, str],
) -> tuple[bool, str, List[int], List[int], List[int]]:
    cur_hand = list(hand_ids)
    occupied_set = {int(tile_id) for tile_id in occupied_ids}
    replacement_list = [int(tile_id) for tile_id in replacement_ids]
    cursor = 0
    change_count = max(0, int(current_change_count or 0))
    max_changes = max(0, int(max_change_count or 0))
    per_change = max(0, int(per_change_limit or 0))
    switch_discards: List[int] = []
    switch_in: List[int] = []
    batch_sizes: List[int] = []

    def missing_replacement_occupied() -> List[int]:
        hand_set = set(cur_hand)
        return [tile_id for tile_id in replacement_list if tile_id in occupied_set and tile_id not in hand_set]

    while change_count < max_changes:
        keep_count = sum(1 for tile_id in cur_hand if tile_id in occupied_set)
        missing = missing_replacement_occupied()
        if keep_count == 13:
            if missing:
                return False, f"keep-count-full-but-missing-needed-replacements: {len(missing)}", switch_discards, switch_in, batch_sizes
            return True, "ok", switch_discards, switch_in, batch_sizes

        batch = min(13 - keep_count, per_change)
        if batch <= 0:
            break

        discard_candidates = [tile_id for tile_id in _candidate_ids(cur_hand, deck_map) if tile_id not in occupied_set]
        if len(discard_candidates) < batch:
            return False, (
                f"insufficient-discardable-tiles: step={change_count + 1}, "
                f"have={len(discard_candidates)}, need={batch}"
            ), switch_discards, switch_in, batch_sizes

        incoming_batch = replacement_list[cursor:cursor + batch]
        if len(incoming_batch) < batch:
            return False, (
                f"replacement-stack-exhausted: step={change_count + 1}, "
                f"have={len(incoming_batch)}, need={batch}"
            ), switch_discards, switch_in, batch_sizes

        discard_batch = discard_candidates[:batch]
        for tile_id in discard_batch:
            cur_hand.remove(tile_id)
        cur_hand.extend(incoming_batch)
        cursor += batch
        change_count += 1
        switch_discards.extend(discard_batch)
        switch_in.extend(incoming_batch)
        batch_sizes.append(batch)

        if not missing_replacement_occupied():
            return True, "ok", switch_discards, switch_in, batch_sizes

    missing = missing_replacement_occupied()
    if missing:
        return False, f"changes-exhausted-but-missing-needed-replacements: {len(missing)}", switch_discards, switch_in, batch_sizes
    return True, "ok", switch_discards, switch_in, batch_sizes


def _explain_switch_batch_failure(
        hand_ids: Sequence[int],
        consumed_ids: Sequence[int],
        target_face_counts: Counter[str],
        deck_map: Dict[int, str],
        remaining_changes: int,
        per_change_limit: int,
) -> str:
    c = len(consumed_ids)
    if c == 0:
        return "无需从换牌堆继续吃牌"

    faces = tuple(sorted(target_face_counts.keys(), key=lambda tile: TILE_INDEX.get(tile, 99)))
    suffix_counts: List[Counter[str]] = [Counter() for _ in range(c + 1)]
    running = Counter()
    for idx in range(c - 1, -1, -1):
        face = _norm(deck_map[consumed_ids[idx]])
        if face in target_face_counts:
            running[face] += 1
        suffix_counts[idx] = running.copy()

    def required_keep_count(pos: int) -> int:
        future = suffix_counts[pos]
        return sum(max(0, target_face_counts[face] - future.get(face, 0)) for face in faces)

    @lru_cache(maxsize=None)
    def solve(pos: int, ops_left: int) -> bool:
        if pos == c:
            return True
        if ops_left <= 0:
            return False
        available = max(0, 13 - required_keep_count(pos))
        if available <= 0:
            return False
        max_batch = min(per_change_limit, c - pos, available)
        for batch in range(1, max_batch + 1):
            if solve(pos + batch, ops_left - 1):
                return True
        return False

    pos = 0
    ops_left = remaining_changes
    step = 1
    while pos < c:
        if ops_left <= 0:
            return f"到第 {step} 次换牌前已无剩余次数，但还剩 {c - pos} 张必须按顺序吃入的换牌牌"
        available = max(0, 13 - required_keep_count(pos))
        if available <= 0:
            return f"到第 {step} 次换牌前，按当前最终目标至少要保留 13 张牌，已没有可继续换出的槽位，但还剩 {c - pos} 张必须按顺序吃入的换牌牌"
        max_batch = min(per_change_limit, c - pos, available)
        feasible_batch = None
        for batch in range(1, max_batch + 1):
            if solve(pos + batch, ops_left - 1):
                feasible_batch = batch
                break
        if feasible_batch is None:
            return (
                f"第 {step} 次换牌无法继续：当前最多只能换 {max_batch} 张，"
                f"按当前最终目标至少要保留 {13 - available} 张牌，"
                f"但后续还剩 {c - pos} 张必须按顺序吃入的换牌牌"
            )
        pos += feasible_batch
        ops_left -= 1
        step += 1

    return "换牌可达性验证失败，但暂时没有定位到更具体的阻塞原因"


def _materialize_switch_plan(
        hand_ids: Sequence[int],
        consumed_ids: Sequence[int],
        target_face_counts: Counter[str],
        deck_map: Dict[int, str],
        batches: Sequence[int],
) -> tuple[List[int], List[int]]:
    cur_hand = list(hand_ids)
    switch_discards: List[int] = []
    switch_in: List[int] = []
    pos = 0
    c = len(consumed_ids)
    faces = tuple(sorted(target_face_counts.keys(), key=lambda tile: TILE_INDEX.get(tile, 99)))
    suffix_counts: List[Counter[str]] = [Counter() for _ in range(c + 1)]
    running = Counter()
    for idx in range(c - 1, -1, -1):
        face = _norm(deck_map[consumed_ids[idx]])
        if face in target_face_counts:
            running[face] += 1
        suffix_counts[idx] = running.copy()

    for batch in batches:
        future = suffix_counts[pos]
        keep_need = {face: max(0, target_face_counts[face] - future.get(face, 0)) for face in faces}
        by_face_ids: Dict[str, List[int]] = defaultdict(list)
        for tile_id in _candidate_ids(cur_hand, deck_map):
            by_face_ids[_norm(deck_map[tile_id])].append(tile_id)
        keep_ids: Set[int] = set()
        for face in faces:
            keep_ids.update(by_face_ids.get(face, [])[:keep_need[face]])
        discard_candidates = [tile_id for tile_id in _candidate_ids(cur_hand, deck_map) if tile_id not in keep_ids]
        discard_batch = discard_candidates[:batch]
        switch_discards.extend(discard_batch)
        for tile_id in discard_batch:
            cur_hand.remove(tile_id)

        incoming_batch = list(consumed_ids[pos:pos + batch])
        pos += batch
        switch_in.extend(incoming_batch)
        cur_hand.extend(incoming_batch)

    return switch_discards, switch_in


def _max_consumable_replacements(
        consumed_ids: Sequence[int],
        target_face_counts: Counter[str],
        deck_map: Dict[int, str],
        remaining_changes: int,
        per_change_limit: int,
) -> int:
    c = len(consumed_ids)
    if c == 0 or remaining_changes <= 0:
        return 0

    faces = tuple(sorted(target_face_counts.keys(), key=lambda tile: TILE_INDEX.get(tile, 99)))
    suffix_counts: List[Counter[str]] = [Counter() for _ in range(c + 1)]
    running = Counter()
    for idx in range(c - 1, -1, -1):
        face = _norm(deck_map[consumed_ids[idx]])
        if face in target_face_counts:
            running[face] += 1
        suffix_counts[idx] = running.copy()

    def required_keep_count(pos: int) -> int:
        future = suffix_counts[pos]
        return sum(max(0, target_face_counts[face] - future.get(face, 0)) for face in faces)

    pos = 0
    for _ in range(remaining_changes):
        if pos >= c:
            break
        available = max(0, 13 - required_keep_count(pos))
        if available <= 0:
            break
        batch = min(per_change_limit, c - pos, available)
        if batch <= 0:
            break
        pos += batch
    return pos


def _select_initial_fillers(
        hand_ids: Sequence[int],
        essential_ids: Set[int],
        need: int,
        deck_map: Dict[int, str],
) -> Optional[List[int]]:
    if need == 0:
        return []
    fillers = [tile_id for tile_id in _candidate_ids(hand_ids, deck_map) if tile_id not in essential_ids]
    if len(fillers) < need:
        return None
    return fillers[:need]


def _pair_components(
        pool: Sequence[PoolEntry],
        blocked_orders: Set[int],
        *,
        allow_replacement: bool = True,
        min_order: int = 0,
) -> List[dict]:
    by_face: Dict[str, List[PoolEntry]] = defaultdict(list)
    for entry in pool:
        if entry.order in blocked_orders:
            continue
        if entry.order < min_order:
            continue
        if not allow_replacement and entry.source == "replacement":
            continue
        by_face[entry.face].append(entry)
    result: List[dict] = []
    for face, entries in by_face.items():
        if len(entries) < 2:
            continue
        for a, b in combinations(entries, 2):
            result.append({
                "ids": (a.tile_id, b.tile_id),
                "orders": (a.order, b.order),
                "face": face,
                "kind": "pair",
                "desc": f"雀头[{face}] <- {_entry_label(a)} / {_entry_label(b)}",
                "sort_key": (max(a.order, b.order), TILE_INDEX.get(face, 99), a.tile_id, b.tile_id),
            })
    result.sort(key=lambda item: item["sort_key"])
    return result


def _meld_components(
        pool: Sequence[PoolEntry],
        blocked_orders: Set[int],
        *,
        allow_replacement: bool = True,
        min_order: int = 0,
) -> List[dict]:
    by_face: Dict[str, List[PoolEntry]] = defaultdict(list)
    for entry in pool:
        if entry.order in blocked_orders:
            continue
        if entry.order < min_order:
            continue
        if not allow_replacement and entry.source == "replacement":
            continue
        by_face[entry.face].append(entry)

    result: List[dict] = []
    seen: Set[tuple] = set()

    for face, entries in by_face.items():
        if len(entries) >= 3:
            for combo in combinations(entries, 3):
                ids = tuple(sorted(entry.tile_id for entry in combo))
                sig = ("triplet", ids)
                if sig in seen:
                    continue
                seen.add(sig)
                result.append({
                    "ids": ids,
                    "orders": tuple(sorted(entry.order for entry in combo)),
                    "face": face,
                    "kind": "triplet",
                    "desc": f"面子[刻子 {face}] <- " + " / ".join(_entry_label(entry) for entry in combo),
                    "sort_key": (max(entry.order for entry in combo), 0, TILE_INDEX.get(face, 99), ids),
                })

    for face, entries in by_face.items():
        rank, suit = _parse_face(face)
        if suit not in "mps" or rank > 7:
            continue
        face2 = f"{rank + 1}{suit}"
        face3 = f"{rank + 2}{suit}"
        if face2 not in by_face or face3 not in by_face:
            continue
        for a in entries:
            for b in by_face[face2]:
                for c in by_face[face3]:
                    ids = tuple(sorted((a.tile_id, b.tile_id, c.tile_id)))
                    sig = ("sequence", ids)
                    if sig in seen:
                        continue
                    seen.add(sig)
                    seq = f"{face}{face2}{face3}"
                    result.append({
                        "ids": ids,
                        "orders": tuple(sorted((a.order, b.order, c.order))),
                        "face": face,
                        "kind": "sequence",
                        "desc": f"面子[顺子 {seq}] <- {_entry_label(a)} / {_entry_label(b)} / {_entry_label(c)}",
                        "sort_key": (max(a.order, b.order, c.order), 1, TILE_INDEX.get(face, 99), ids),
                    })

    result.sort(key=lambda item: item["sort_key"])
    return result


def _win_components_for_wall_tile(
        win_entry: PoolEntry,
        pool: Sequence[PoolEntry],
        blocked_orders: Set[int],
        *,
        allow_replacement: bool = True,
        min_order: int = 0,
) -> List[dict]:
    by_face: Dict[str, List[PoolEntry]] = defaultdict(list)
    for entry in pool:
        if entry.order in blocked_orders or entry.tile_id == win_entry.tile_id:
            continue
        if entry.order < min_order:
            continue
        if not allow_replacement and entry.source == "replacement":
            continue
        by_face[entry.face].append(entry)

    result: List[dict] = []
    same_face_entries = by_face.get(win_entry.face, [])

    for entry in same_face_entries:
        result.append({
            "role": "pair",
            "ids": (entry.tile_id,),
            "orders": (entry.order,),
            "face": win_entry.face,
            "kind": "pair",
            "desc": f"雀头[{win_entry.face}] <- {_entry_label(entry)} + 胡牌 {_entry_label(win_entry)}",
            "sort_key": (entry.order, 0, TILE_INDEX.get(win_entry.face, 99), entry.tile_id),
        })

    if len(same_face_entries) >= 2:
        for a, b in combinations(same_face_entries, 2):
            result.append({
                "role": "meld",
                "ids": (a.tile_id, b.tile_id),
                "orders": tuple(sorted((a.order, b.order))),
                "face": win_entry.face,
                "kind": "triplet",
                "desc": f"面子[刻子 {win_entry.face}] <- {_entry_label(a)} / {_entry_label(b)} / 胡牌 {_entry_label(win_entry)}",
                "sort_key": (max(a.order, b.order), 1, TILE_INDEX.get(win_entry.face, 99), a.tile_id, b.tile_id),
            })

    rank, suit = _parse_face(win_entry.face)
    if suit in "mps":
        patterns = []
        if rank >= 3:
            patterns.append((f"{rank - 2}{suit}", f"{rank - 1}{suit}"))
        if 2 <= rank <= 8:
            patterns.append((f"{rank - 1}{suit}", f"{rank + 1}{suit}"))
        if rank <= 7:
            patterns.append((f"{rank + 1}{suit}", f"{rank + 2}{suit}"))
        for fa, fb in patterns:
            for a in by_face.get(fa, []):
                for b in by_face.get(fb, []):
                    ids = tuple(sorted((a.tile_id, b.tile_id)))
                    seq = ''.join(sorted((fa, fb, win_entry.face), key=lambda tile: TILE_INDEX.get(tile, 99)))
                    result.append({
                        "role": "meld",
                        "ids": ids,
                        "orders": tuple(sorted((a.order, b.order))),
                        "face": win_entry.face,
                        "kind": "sequence",
                        "desc": f"面子[顺子 {seq}] <- {_entry_label(a)} / {_entry_label(b)} / 胡牌 {_entry_label(win_entry)}",
                        "sort_key": (max(a.order, b.order), 2, TILE_INDEX.get(win_entry.face, 99), ids),
                    })

    result.sort(key=lambda item: item["sort_key"])
    return result


def _plan_summary(plan: dict, deck_map: Dict[int, str]) -> str:
    return "\n".join([
        f"换出: {', '.join(f'{_norm(deck_map[t])}(id={t})' for t in plan.get('switch_discards', [])) or '-'}",
        f"换入: {', '.join(f'{_norm(deck_map[t])}(id={t})' for t in plan.get('switch_in', [])) or '-'}",
        f"摸牌序列: {', '.join(f'{_norm(deck_map[t])}(id={t})' for t in plan.get('wall_draws', [])) or '-'}",
        f"最终需打掉的占位牌: {', '.join(f'{_norm(deck_map[t])}(id={t})' for t in plan.get('post_draw_discards', [])) or '-'}",
        f"听牌: {', '.join(plan.get('waits', [])) or '-'}",
        f"组合: {' | '.join(plan.get('component_descs', [])) or '-'}",
    ])


def _equivalent_hand13_faces(
        quad_ids: Sequence[int],
        prewin_ids: Sequence[int],
        deck_map: Dict[int, str],
) -> List[str]:
    faces: List[str] = []
    quad_face_cnt = Counter(_norm(deck_map[tile_id]) for tile_id in quad_ids)
    for face, cnt in quad_face_cnt.items():
        # 双杠在和牌结构里按刻子计入，只占 3 张等价值。
        faces.extend([face] * min(cnt, 3))
    faces.extend(_norm(deck_map[tile_id]) for tile_id in prewin_ids)
    return faces


def _candidate_signature(*args) -> tuple:
    if len(args) == 1:
        prewin_ids = args[0]
        return tuple(sorted(prewin_ids))
    if len(args) == 2:
        win_tile_id, prewin_ids = args
        return win_tile_id, tuple(sorted(prewin_ids))
    raise TypeError("_candidate_signature expects 1 or 2 arguments")


def _blocked_key(blocked_orders: Set[int]) -> frozenset[int]:
    return frozenset(blocked_orders)


def _latest_nonquad_wall_win_id(
        prewin_ids: Sequence[int],
        win_tile_id: int,
        by_id: Dict[int, PoolEntry],
) -> int:
    candidate_ids = [tile_id for tile_id in list(prewin_ids) + [win_tile_id] if by_id[tile_id].source == "wall"]
    if not candidate_ids:
        return win_tile_id
    return max(candidate_ids, key=lambda tile_id: by_id[tile_id].source_index)


def _target13_lower_bound(
        quad_ids: Sequence[int],
        selected_ids: Sequence[int],
        win_tile_id: Optional[int],
        by_id: Dict[int, PoolEntry],
) -> int:
    all_ids = list(quad_ids) + list(selected_ids)
    if win_tile_id is not None:
        all_ids.append(win_tile_id)
    wall_count = sum(1 for tile_id in all_ids if by_id[tile_id].source == "wall")
    nonwall_count = len(all_ids) - wall_count
    kong_slot_gain = len(quad_ids) // 4
    future_wall_need = max(0, wall_count - (1 if win_tile_id is not None else 0))
    filler_need = max(0, future_wall_need - kong_slot_gain)
    return nonwall_count + filler_need


def _nonwall_selected_count(
        quad_ids: Sequence[int],
        selected_ids: Sequence[int],
        win_tile_id: Optional[int],
        by_id: Dict[int, PoolEntry],
) -> int:
    all_ids = list(quad_ids) + list(selected_ids)
    if win_tile_id is not None:
        all_ids.append(win_tile_id)
    return sum(1 for tile_id in all_ids if by_id[tile_id].source != "wall")


def _occupied_prefix_count(
        quad_ids: Sequence[int],
        selected_ids: Sequence[int],
        win_tile_id: Optional[int],
        by_id: Dict[int, PoolEntry],
        wall_start_order: int,
) -> int:
    all_ids = list(quad_ids) + list(selected_ids)
    if win_tile_id is not None:
        all_ids.append(win_tile_id)
    return sum(1 for tile_id in all_ids if by_id[tile_id].order < wall_start_order)


def _replacement_used_count(tile_ids: Sequence[int], by_id: Dict[int, PoolEntry]) -> int:
    return sum(1 for tile_id in tile_ids if by_id[tile_id].source == "replacement")


def _hand13_debug_text(faces: Sequence[str]) -> str:
    return ", ".join(faces) if faces else "-"


def _single_components(
        pool: Sequence[PoolEntry],
        blocked_orders: Set[int],
        *,
        allow_replacement: bool = True,
        min_order: int = 0,
) -> List[dict]:
    result: List[dict] = []
    for entry in pool:
        if entry.order in blocked_orders:
            continue
        if entry.order < min_order:
            continue
        if not allow_replacement and entry.source == "replacement":
            continue
        result.append({
            "ids": (entry.tile_id,),
            "orders": (entry.order,),
            "face": entry.face,
            "kind": "single",
            "desc": f"单骑[{entry.face}] <- {_entry_label(entry)}",
            "sort_key": (entry.order, TILE_INDEX.get(entry.face, 99), entry.tile_id),
        })
    result.sort(key=lambda item: item["sort_key"])
    return result


def _wait_components_for_face(
        wait_face: str,
        pool: Sequence[PoolEntry],
        blocked_orders: Set[int],
        *,
        allow_replacement: bool = True,
        min_order: int = 0,
) -> List[dict]:
    by_face: Dict[str, List[PoolEntry]] = defaultdict(list)
    for entry in pool:
        if entry.order in blocked_orders:
            continue
        if entry.order < min_order:
            continue
        if not allow_replacement and entry.source == "replacement":
            continue
        by_face[entry.face].append(entry)

    result: List[dict] = []
    same_face_entries = by_face.get(wait_face, [])

    for entry in same_face_entries:
        result.append({
            "role": "pair",
            "ids": (entry.tile_id,),
            "orders": (entry.order,),
            "face": wait_face,
            "kind": "pair",
            "desc": f"雀头[{wait_face}] <- {_entry_label(entry)} + 听牌 {wait_face}",
            "sort_key": (entry.order, 0, TILE_INDEX.get(wait_face, 99), entry.tile_id),
        })

    if len(same_face_entries) >= 2:
        for a, b in combinations(same_face_entries, 2):
            result.append({
                "role": "meld",
                "ids": (a.tile_id, b.tile_id),
                "orders": tuple(sorted((a.order, b.order))),
                "face": wait_face,
                "kind": "triplet",
                "desc": f"面子[刻子 {wait_face}] <- {_entry_label(a)} / {_entry_label(b)} / 听牌 {wait_face}",
                "sort_key": (max(a.order, b.order), 1, TILE_INDEX.get(wait_face, 99), a.tile_id, b.tile_id),
            })

    rank, suit = _parse_face(wait_face)
    if suit in "mps":
        patterns = []
        if rank >= 3:
            patterns.append((f"{rank - 2}{suit}", f"{rank - 1}{suit}"))
        if 2 <= rank <= 8:
            patterns.append((f"{rank - 1}{suit}", f"{rank + 1}{suit}"))
        if rank <= 7:
            patterns.append((f"{rank + 1}{suit}", f"{rank + 2}{suit}"))
        for fa, fb in patterns:
            for a in by_face.get(fa, []):
                for b in by_face.get(fb, []):
                    ids = tuple(sorted((a.tile_id, b.tile_id)))
                    seq = ''.join(sorted((fa, fb, wait_face), key=lambda tile: TILE_INDEX.get(tile, 99)))
                    result.append({
                        "role": "meld",
                        "ids": ids,
                        "orders": tuple(sorted((a.order, b.order))),
                        "face": wait_face,
                        "kind": "sequence",
                        "desc": f"面子[顺子 {seq}] <- {_entry_label(a)} / {_entry_label(b)} / 听牌 {wait_face}",
                        "sort_key": (max(a.order, b.order), 2, TILE_INDEX.get(wait_face, 99), ids),
                    })

    result.sort(key=lambda item: item["sort_key"])
    return result


def _taatsu_components(
        pool: Sequence[PoolEntry],
        blocked_orders: Set[int],
        *,
        allow_replacement: bool = True,
        min_order: int = 0,
) -> List[dict]:
    by_face: Dict[str, List[PoolEntry]] = defaultdict(list)
    for entry in pool:
        if entry.order in blocked_orders:
            continue
        if entry.order < min_order:
            continue
        if not allow_replacement and entry.source == "replacement":
            continue
        by_face[entry.face].append(entry)

    result: List[dict] = []
    seen: Set[tuple] = set()

    for face, entries in by_face.items():
        if len(entries) >= 2:
            for a, b in combinations(entries, 2):
                ids = tuple(sorted((a.tile_id, b.tile_id)))
                sig = ("pair-wait", ids)
                if sig in seen:
                    continue
                seen.add(sig)
                result.append({
                    "ids": ids,
                    "orders": tuple(sorted((a.order, b.order))),
                    "face": face,
                    "kind": "pair_wait",
                    "desc": f"搭子[双碰 {face}] <- {_entry_label(a)} / {_entry_label(b)}",
                    "sort_key": (max(a.order, b.order), 0, TILE_INDEX.get(face, 99), ids),
                })

    for face, entries in by_face.items():
        rank, suit = _parse_face(face)
        if suit not in "mps":
            continue
        for delta, kind, label in ((1, "ryanmen", "两面"), (2, "kanchan", "嵌张")):
            face2 = f"{rank + delta}{suit}"
            if face2 not in by_face:
                continue
            for a in entries:
                for b in by_face[face2]:
                    ids = tuple(sorted((a.tile_id, b.tile_id)))
                    sig = (kind, ids)
                    if sig in seen:
                        continue
                    seen.add(sig)
                    seq = ''.join(sorted((face, face2), key=lambda tile: TILE_INDEX.get(tile, 99)))
                    result.append({
                        "ids": ids,
                        "orders": tuple(sorted((a.order, b.order))),
                        "face": face,
                        "kind": kind,
                        "desc": f"搭子[{label} {seq}] <- {_entry_label(a)} / {_entry_label(b)}",
                        "sort_key": (max(a.order, b.order), 1 if delta == 1 else 2, TILE_INDEX.get(face, 99), ids),
                    })

    result.sort(key=lambda item: item["sort_key"])
    return result


def list_reachable_quads_for_switch(
        deck_map: Dict[int, str],
        hand_ids: List[int],
        replacement_ids: List[int],
        wall_ids: List[int],
        switch_used_tiles: List[int],
        total_change_tile_count: int,
        change_tile_count: int,
        boss_buff: Optional[Sequence[int]] = None,
) -> dict:
    remaining_changes = max(0, int(total_change_tile_count or 0) - int(change_tile_count or 0))
    per_change_limit = 3 if 901 in (boss_buff or []) else 13
    used_count = len(switch_used_tiles or [])
    remaining_replacements = _limit_replacement_search_window(
        replacement_ids[used_count:],
        remaining_changes,
        per_change_limit,
    )
    pool, by_id = _build_pool(deck_map, hand_ids, remaining_replacements, wall_ids)
    quads = _enumerate_quads(pool, by_id)
    debug_pool = _build_debug_pool_info(
        deck_map,
        hand_ids,
        remaining_replacements,
        wall_ids,
        quads,
        by_id,
        replacement_total_remaining=max(0, len(replacement_ids) - used_count),
        replacement_window_count=len(remaining_replacements),
        replacement_used_count=used_count,
    )

    catalog: List[dict] = []
    for quad in quads:
        ok, reason, switch_discards, switch_in, batches = _simulate_switch_reachability(
            hand_ids,
            quad["ids"],
            remaining_replacements,
            0,
            remaining_changes,
            per_change_limit,
            deck_map,
        )
        if not ok:
            catalog.append({
                "face": quad["face"],
                "label": _quad_label(quad["ids"], by_id),
                "tile_positions": _quad_tile_positions(quad["ids"], by_id),
                "score": _quad_score_text(quad["time_key"]),
                "switch_batch_sizes": [],
                "switch_discards": [],
                "switch_in": [],
                "reachable": False,
                "reason": reason,
            })
            continue

        catalog.append({
            "face": quad["face"],
            "label": _quad_label(quad["ids"], by_id),
            "tile_positions": _quad_tile_positions(quad["ids"], by_id),
            "score": _quad_score_text(quad["time_key"]),
            "switch_batch_sizes": batches,
            "switch_discards": switch_discards,
            "switch_in": switch_in,
            "reachable": True,
            "reason": "",
        })

    return {
        "status": "catalog",
        "reason": "quad-catalog",
        "quad_catalog": catalog,
        "remaining_changes": remaining_changes,
        "debug_pool": debug_pool,
    }


def _is_exact_pair2(faces2: Sequence[str]) -> bool:
    return len(faces2) == 2 and faces2[0] == faces2[1]


def _is_valid_taatsu2(faces2: Sequence[str]) -> bool:
    if len(faces2) != 2:
        return False
    a, b = sorted(faces2, key=lambda tile: TILE_INDEX.get(tile, 99))
    if a == b:
        return True
    ra, sa = _parse_face(a)
    rb, sb = _parse_face(b)
    if sa not in "mps" or sa != sb:
        return False
    return rb - ra in (1, 2)


def _manual_bucket_faces(ids: Sequence[int], deck_map: Dict[int, str]) -> List[str]:
    return [_norm(deck_map[int(tile_id)]) for tile_id in ids]


def _manual_meld_searchable(faces: Sequence[str]) -> tuple[bool, str]:
    if len(faces) != 3:
        return False, "面子必须正好是 3 张"
    if not _is_exact_meld3(faces):
        return False, "完整面子不是合法的顺子或刻子"
    return True, ""


def _analyze_manual_searchability(
        deck_map: Dict[int, str],
        structure_groups: Dict[str, Sequence[int]],
) -> tuple[bool, str]:
    meld1_faces = _manual_bucket_faces(structure_groups.get("meld1") or [], deck_map)
    meld2_faces = _manual_bucket_faces(structure_groups.get("meld2") or [], deck_map)
    pair_faces = _manual_bucket_faces(structure_groups.get("pair") or [], deck_map)

    deficits = {
        "meld1": 3 - len(meld1_faces),
        "meld2": 3 - len(meld2_faces),
        "pair": 2 - len(pair_faces),
    }
    short_keys = [key for key, miss in deficits.items() if miss == 1]
    if len(short_keys) != 1 or any(miss < 0 or miss > 1 for miss in deficits.values()):
        return False, "搜索器只支持恰好有一个分组少 1 张牌的听牌形"

    short_key = short_keys[0]
    if short_key == "pair":
        ok1, reason1 = _manual_meld_searchable(meld1_faces)
        if not ok1:
            return False, f"面子 A 无法被搜索器按完整面子枚举: {reason1}"
        ok2, reason2 = _manual_meld_searchable(meld2_faces)
        if not ok2:
            return False, f"面子 B 无法被搜索器按完整面子枚举: {reason2}"
        return True, "该形状符合搜索器的“单骑雀头加两个完整面子”枚举规则"

    short_faces = meld1_faces if short_key == "meld1" else meld2_faces
    other_faces = meld2_faces if short_key == "meld1" else meld1_faces
    if len(pair_faces) != 2 or not _is_exact_pair2(pair_faces):
        return False, "当缺的是面子时，雀头必须是完整对子"
    if len(short_faces) != 2 or not _is_valid_taatsu2(short_faces):
        return False, "缺一张的面子必须是可补成和牌的两张搭子"
    ok_other, reason_other = _manual_meld_searchable(other_faces)
    if not ok_other:
        which = "面子 B" if short_key == "meld1" else "面子 A"
        return False, f"{which} 无法被搜索器按完整面子枚举: {reason_other}"
    return True, "该形状符合搜索器的“一个完整面子加一个缺一张的面子”枚举规则"


def validate_manual_souzu_switch_plan(
        deck_map: Dict[int, str],
        hand_ids: List[int],
        replacement_ids: List[int],
        wall_ids: List[int],
        switch_used_tiles: List[int],
        total_change_tile_count: int,
        change_tile_count: int,
        boss_buff: Optional[Sequence[int]],
        quad_groups: Sequence[Sequence[int]],
        structure_groups: Dict[str, Sequence[int]],
) -> dict:
    remaining_changes = max(0, int(total_change_tile_count or 0) - int(change_tile_count or 0))
    per_change_limit = 3 if 901 in (boss_buff or []) else 13
    used_count = len(switch_used_tiles or [])
    remaining_replacements = _limit_replacement_search_window(
        replacement_ids[used_count:],
        remaining_changes,
        per_change_limit,
    )
    pool, by_id = _build_pool(deck_map, hand_ids, remaining_replacements, wall_ids)
    pool_ids = {entry.tile_id for entry in pool}
    quads = _enumerate_quads(pool, by_id)
    debug_pool = _build_debug_pool_info(
        deck_map,
        hand_ids,
        remaining_replacements,
        wall_ids,
        quads,
        by_id,
        replacement_total_remaining=max(0, len(replacement_ids) - used_count),
        replacement_window_count=len(remaining_replacements),
        replacement_used_count=used_count,
    )

    if len(hand_ids) != 13:
        return {"status": "impossible", "reason": "switch-hand-must-be-13", "remaining_changes": remaining_changes, "debug_pool": debug_pool}

    if len(quad_groups) != 2:
        return {"status": "impossible", "reason": "manual-quad-count-invalid", "debug_pool": debug_pool}

    normalized_quads: List[tuple[int, ...]] = []
    quad_faces: List[str] = []
    used_ids: Set[int] = set()
    available_quads = {tuple(sorted(quad["ids"])): quad for quad in quads}

    for index, group in enumerate(quad_groups, start=1):
        ids = tuple(sorted(int(tile_id) for tile_id in group))
        if len(ids) != 4:
            return {"status": "impossible", "reason": f"manual-quad-{index}-size-invalid", "debug_pool": debug_pool}
        if len(set(ids)) != 4:
            return {"status": "impossible", "reason": f"manual-quad-{index}-duplicate-tile", "debug_pool": debug_pool}
        if any(tile_id not in pool_ids for tile_id in ids):
            return {"status": "impossible", "reason": f"manual-quad-{index}-tile-not-in-pool", "debug_pool": debug_pool}
        if any(tile_id in used_ids for tile_id in ids):
            return {"status": "impossible", "reason": f"manual-quad-{index}-overlap", "debug_pool": debug_pool}
        quad = available_quads.get(ids)
        if quad is None:
            return {"status": "impossible", "reason": f"manual-quad-{index}-not-a-quad", "debug_pool": debug_pool}
        normalized_quads.append(ids)
        quad_faces.append(str(quad["face"]))
        used_ids.update(ids)

    meld1_ids = tuple(sorted(int(tile_id) for tile_id in (structure_groups.get("meld1") or [])))
    meld2_ids = tuple(sorted(int(tile_id) for tile_id in (structure_groups.get("meld2") or [])))
    pair_ids = tuple(sorted(int(tile_id) for tile_id in (structure_groups.get("pair") or [])))

    if len(meld1_ids) > 3 or len(meld2_ids) > 3 or len(pair_ids) > 2:
        return {"status": "impossible", "reason": "manual-structure-size-invalid", "debug_pool": debug_pool}

    prewin_ids = list(meld1_ids + meld2_ids + pair_ids)
    if len(prewin_ids) != 7:
        return {"status": "impossible", "reason": "manual-structure-total-invalid", "debug_pool": debug_pool}
    if len(set(prewin_ids)) != 7:
        return {"status": "impossible", "reason": "manual-structure-duplicate-tile", "debug_pool": debug_pool}
    if any(tile_id not in pool_ids for tile_id in prewin_ids):
        return {"status": "impossible", "reason": "manual-structure-tile-not-in-pool", "debug_pool": debug_pool}
    if any(tile_id in used_ids for tile_id in prewin_ids):
        return {"status": "impossible", "reason": "manual-structure-overlap-with-quad", "debug_pool": debug_pool}

    def group_desc(name: str, ids: Sequence[int]) -> str:
        faces = [_norm(deck_map[int(tile_id)]) for tile_id in ids]
        return f"{name}: {' '.join(faces)}"

    component_descs = [
        group_desc("meld1", meld1_ids),
        group_desc("meld2", meld2_ids),
        group_desc("pair", pair_ids),
    ]
    manual_searchable, manual_search_reason = _analyze_manual_searchability(
        deck_map,
        {
            "meld1": meld1_ids,
            "meld2": meld2_ids,
            "pair": pair_ids,
        },
    )
    quad_ids = list(normalized_quads[0] + normalized_quads[1])
    plan, reason = _evaluate_candidate(
        deck_map,
        hand_ids,
        remaining_replacements,
        by_id,
        quad_ids,
        prewin_ids,
        component_descs,
        remaining_changes,
        per_change_limit,
    )
    if plan is None:
        return {
            "status": "impossible",
            "reason": reason,
            "remaining_changes": remaining_changes,
            "quad_faces": quad_faces,
            "target13": [_norm(deck_map[int(tile_id)]) for tile_id in prewin_ids],
            "component_descs": component_descs,
            "manual_searchable": manual_searchable,
            "manual_search_reason": manual_search_reason,
            "debug_pool": debug_pool,
        }

    plan["quad_faces"] = quad_faces
    plan["component_descs"] = component_descs
    plan["manual_searchable"] = manual_searchable
    plan["manual_search_reason"] = manual_search_reason
    plan["debug_pool"] = debug_pool
    return plan


def _signature_to_text(signature: tuple) -> str:
    if len(signature) == 2 and isinstance(signature[1], tuple):
        return f"{signature[0]}|" + ",".join(str(tile_id) for tile_id in signature[1])
    return ",".join(str(tile_id) for tile_id in signature)


def _evaluate_candidate(
        deck_map: Dict[int, str],
        hand_ids: Sequence[int],
        replacement_ids: Sequence[int],
        by_id: Dict[int, PoolEntry],
        quad_ids: Sequence[int],
        prewin_ids: Sequence[int],
        component_descs: Sequence[str],
        remaining_changes: int,
        per_change_limit: int,
) -> tuple[Optional[dict], str]:
    prewin_physical_ids = list(quad_ids) + list(prewin_ids)
    concealed_hand7 = [_norm(deck_map[tile_id]) for tile_id in prewin_ids]
    if len(concealed_hand7) != 7:
        return None, (
            f"去掉双杠后的待听手牌必须是 7 张，但当前为 {len(concealed_hand7)} 张。"
            f"当前候选总物理牌数为 {len(prewin_physical_ids)} 张。"
        )

    pair_wait_shape_reason = _non_souzu_pair_wait_shape_reason(concealed_hand7)
    if pair_wait_shape_reason is not None:
        return None, f"{pair_wait_shape_reason}。当前 7 张待听手牌：{_hand13_debug_text(concealed_hand7)}"

    opened_face_cnt = Counter(_norm(deck_map[tile_id]) for tile_id in list(quad_ids))
    waits = _waits_for_open_two_melds_faces(concealed_hand7, opened_face_cnt)
    waits = sorted(waits, key=lambda tile: TILE_INDEX[tile])
    if not waits:
        return None, f"当前 7 张待听手牌无法形成有效听牌：{_hand13_debug_text(concealed_hand7)}"
    if any(not face.endswith("s") for face in waits):
        return None, (
            f"存在非索子听牌：{','.join(waits)}。"
            f"当前 7 张待听手牌：{_hand13_debug_text(concealed_hand7)}"
        )

    prewin_wall_entries = sorted(
        [by_id[tile_id] for tile_id in prewin_physical_ids if by_id[tile_id].source == "wall"],
        key=lambda entry: entry.source_index,
    )

    essential_nonwall_ids = {tile_id for tile_id in prewin_physical_ids if by_id[tile_id].source != "wall"}
    if len(essential_nonwall_ids) > 13:
        return None, (
            f"去掉双杠后，必须预先占位的非牌山牌已有 {len(essential_nonwall_ids)} 张，"
            "超过了 13 张的可行上限。"
        )
    kong_slot_gain = len(quad_ids) // 4
    filler_need = max(0, len(prewin_wall_entries) - kong_slot_gain)
    filler_ids = _select_initial_fillers(hand_ids, essential_nonwall_ids, filler_need, deck_map)
    if filler_ids is None:
        return None, (
            "无法补齐起手目标牌。"
            f"当前需要为 {len(prewin_wall_entries)} 张牌山牌预留占位，但双杠只腾出了 {kong_slot_gain} 个位置。"
        )

    switch_target_ids = list(essential_nonwall_ids) + filler_ids
    if len(switch_target_ids) != 13:
        return None, (
            f"最终目标牌数量异常：{len(switch_target_ids)} 张。"
            f"非牌山牌 {len(essential_nonwall_ids)} 张，补位牌 {len(filler_ids)} 张，双杠腾出 {kong_slot_gain} 个位置。"
        )
    ok, reason, switch_discards, switch_in, batches = _simulate_switch_reachability(
        hand_ids,
        prewin_physical_ids,
        replacement_ids,
        0,
        remaining_changes,
        per_change_limit,
        deck_map,
    )
    if not ok:
        return None, f"switch-reachability-failed: {reason}"
    ordered_wall_draws = [
        entry.tile_id
        for entry in sorted(
            {
                by_id[tile_id]
                for tile_id in list(quad_ids) + list(prewin_ids)
                if by_id[tile_id].source == "wall"
            },
            key=lambda entry: entry.source_index,
        )
    ]

    return {
        "status": "plan",
        "mode": "quad-first-dfs",
        "draws_needed": max((by_id[tile_id].source_index + 1 for tile_id in ordered_wall_draws), default=0),
        "switch_discards": switch_discards,
        "switch_in": switch_in,
        "switch_batch_sizes": batches,
        "wall_draws": ordered_wall_draws,
        "post_draw_discards": filler_ids,
        "waits": waits,
        "quad_faces": [],
        "target13": concealed_hand7,
        "remaining_changes": remaining_changes,
        "win_tile_faces": [],
        "win_tile_draw_index": None,
        "component_descs": list(component_descs),
        "retargeted_win_tile_id": None,
    }, "ok"



def _search_remaining_plan(
        pool: Sequence[PoolEntry],
        by_id: Dict[int, PoolEntry],
        quad_pair: dict,
        deck_map: Dict[int, str],
        hand_ids: Sequence[int],
        replacement_ids: Sequence[int],
        remaining_changes: int,
        per_change_limit: int,
        stats: dict,
        emit_progress: Callable[[str], None],
        emit_candidate: Optional[Callable[[dict], None]],
        timed_out: Callable[[], bool],
        best_draws_limit: Optional[int] = None,
        stop_after_first: bool = False,
        skip_signatures: Optional[Set[str]] = None,
) -> Optional[dict]:
    quad_orders = {by_id[tile_id].order for tile_id in quad_pair["quad_ids"]}
    seen_candidate_signatures: Set[tuple[int, tuple[int, ...]]] = set()
    pair_cache: Dict[tuple[frozenset[int], bool], List[dict]] = {}
    meld_cache: Dict[tuple[frozenset[int], bool], List[dict]] = {}
    single_cache: Dict[tuple[frozenset[int], bool], List[dict]] = {}
    taatsu_cache: Dict[tuple[frozenset[int], bool], List[dict]] = {}
    win_component_cache: Dict[tuple[int, frozenset[int], bool], List[dict]] = {}
    best_plan: Optional[dict] = None
    local_best_draws = best_draws_limit
    wall_start_order = len(hand_ids) + len(replacement_ids)

    def get_pairs(blocked_orders: Set[int], *, allow_replacement: bool, min_order: int) -> List[dict]:
        key = (_blocked_key(blocked_orders), allow_replacement, min_order)
        cached = pair_cache.get(key)
        if cached is not None:
            stats["state_cache_hits"] += 1
            return cached
        built = _pair_components(pool, blocked_orders, allow_replacement=allow_replacement, min_order=min_order)
        pair_cache[key] = built
        return built

    def get_melds(blocked_orders: Set[int], *, allow_replacement: bool, min_order: int) -> List[dict]:
        key = (_blocked_key(blocked_orders), allow_replacement, min_order)
        cached = meld_cache.get(key)
        if cached is not None:
            stats["state_cache_hits"] += 1
            return cached
        built = _meld_components(pool, blocked_orders, allow_replacement=allow_replacement, min_order=min_order)
        meld_cache[key] = built
        return built

    def get_singles(blocked_orders: Set[int], *, allow_replacement: bool, min_order: int) -> List[dict]:
        key = (_blocked_key(blocked_orders), allow_replacement, min_order)
        cached = single_cache.get(key)
        if cached is not None:
            stats["state_cache_hits"] += 1
            return cached
        built = _single_components(pool, blocked_orders, allow_replacement=allow_replacement, min_order=min_order)
        single_cache[key] = built
        return built

    def get_taatsus(blocked_orders: Set[int], *, allow_replacement: bool, min_order: int) -> List[dict]:
        key = (_blocked_key(blocked_orders), allow_replacement, min_order)
        cached = taatsu_cache.get(key)
        if cached is not None:
            stats["state_cache_hits"] += 1
            return cached
        built = _taatsu_components(pool, blocked_orders, allow_replacement=allow_replacement, min_order=min_order)
        taatsu_cache[key] = built
        return built

    def get_wait_components(wait_face: str, blocked_orders: Set[int], *, allow_replacement: bool, min_order: int) -> List[dict]:
        key = (wait_face, _blocked_key(blocked_orders), allow_replacement, min_order)
        cached = win_component_cache.get(key)
        if cached is not None:
            stats["state_cache_hits"] += 1
            return cached
        built = _wait_components_for_face(
            wait_face,
            pool,
            blocked_orders,
            allow_replacement=allow_replacement,
            min_order=min_order,
        )
        win_component_cache[key] = built
        return built

    win_candidates = [f"{n}s" for n in range(1, 10)]

    emit_progress(
        "开始搜索听牌方案\n"
        "说明: 固定双杠后，搜索其余手牌组成可听牌的最终形状\n"
        f"当前双杠: {_quad_label(quad_pair['quad_ids'][:4], by_id)} | {_quad_label(quad_pair['quad_ids'][4:], by_id)}",
        force=True,
    )

    skip_signatures = skip_signatures or set()

    for win_entry_index, wait_face in enumerate(win_candidates, start=1):
        if timed_out():
            return best_plan
        stats["branch_attempts"] += 1
        stats["node_total"] = len(win_candidates)
        stats["node_index"] = win_entry_index
        stats["node_searchable"] = len(pool) - len(quad_orders)
        stats["latest_result"] = f"尝试听牌 {wait_face}"
        emit_progress(
            "正在尝试听牌候选\n"
            f"搜索次数: {stats['branch_attempts']}\n"
            f"当前节点类型: 听牌候选\n"
            f"目标听牌: {wait_face}\n"
            f"当前双杠1: {_quad_label(quad_pair['quad_ids'][:4], by_id)}\n"
            f"当前双杠2: {_quad_label(quad_pair['quad_ids'][4:], by_id)}",
        )

        allow_replacement_after_quad = _replacement_used_count(quad_pair["quad_ids"], by_id) <= 13
        min_order_after_quad = wall_start_order if _occupied_prefix_count(
            quad_pair["quad_ids"], [], None, by_id, wall_start_order
        ) > 13 else 0
        win_components = get_wait_components(
            wait_face,
            quad_orders,
            allow_replacement=allow_replacement_after_quad,
            min_order=min_order_after_quad,
        )
        for win_component_index, win_component in enumerate(win_components, start=1):
            if timed_out():
                return best_plan
            blocked = set(quad_orders)
            blocked.update(by_id[tile_id].order for tile_id in win_component["ids"])
            if _target13_lower_bound(quad_pair["quad_ids"], win_component["ids"], None, by_id) > 13:
                stats["target13_prunes"] += 1
                continue
            stats["dfs_nodes"] += 1
            stats["node_total"] = len(win_components)
            stats["node_index"] = win_component_index
            stats["node_searchable"] = len(pool) - len(blocked)

            emit_progress(
                "正在扩展听牌组合\n"
                f"搜索次数: {stats['branch_attempts']}\n"
                f"当前节点类型: 听牌组合候选\n"
                f"目标听牌: {wait_face}\n"
                f"已定组合: {win_component['desc']}",
            )

            if win_component["role"] == "pair":
                allow_replacement_after_win = _replacement_used_count(
                    list(quad_pair["quad_ids"]) + list(win_component["ids"]),
                    by_id,
                ) <= 13
                min_order_after_win = wall_start_order if _occupied_prefix_count(
                    quad_pair["quad_ids"], list(win_component["ids"]), None, by_id, wall_start_order
                ) > 13 else 0
                melds = get_melds(blocked, allow_replacement=allow_replacement_after_win, min_order=min_order_after_win)
                for meld1_index, meld1 in enumerate(melds, start=1):
                    blocked1 = blocked | {by_id[tile_id].order for tile_id in meld1["ids"]}
                    if _target13_lower_bound(
                            quad_pair["quad_ids"],
                            list(win_component["ids"]) + list(meld1["ids"]),
                            None,
                            by_id,
                    ) > 13:
                        stats["target13_prunes"] += 1
                        continue
                    allow_replacement_after_meld1 = _replacement_used_count(
                        list(quad_pair["quad_ids"]) + list(win_component["ids"]) + list(meld1["ids"]),
                        by_id,
                    ) <= 13
                    min_order_after_meld1 = wall_start_order if _occupied_prefix_count(
                        quad_pair["quad_ids"],
                        list(win_component["ids"]) + list(meld1["ids"]),
                        None,
                        by_id,
                        wall_start_order,
                    ) > 13 else 0
                    meld2_list = get_melds(
                        blocked1,
                        allow_replacement=allow_replacement_after_meld1,
                        min_order=min_order_after_meld1,
                    )
                    stats["node_total"] = len(melds)
                    stats["node_index"] = meld1_index
                    stats["node_searchable"] = len(pool) - len(blocked1)
                    for meld2_index, meld2 in enumerate(meld2_list, start=1):
                        if timed_out():
                            return best_plan
                        stats["node_total"] = len(meld2_list)
                        stats["node_index"] = meld2_index
                        stats["node_searchable"] = len(pool) - len(blocked1 | {by_id[tile_id].order for tile_id in meld2["ids"]})
                        component_descs = [win_component["desc"], meld1["desc"], meld2["desc"]]
                        prewin_ids = list(win_component["ids"]) + list(meld1["ids"]) + list(meld2["ids"])
                        if _occupied_prefix_count(
                                quad_pair["quad_ids"],
                                prewin_ids,
                                None,
                                by_id,
                                wall_start_order,
                        ) > 13:
                            stats["nonwall_prunes"] += 1
                            continue
                        signature = _candidate_signature(prewin_ids)
                        signature_text = _signature_to_text(signature)
                        if signature_text in skip_signatures:
                            stats["duplicate_prunes"] += 1
                            continue
                        if signature in seen_candidate_signatures:
                            stats["duplicate_prunes"] += 1
                            continue
                        seen_candidate_signatures.add(signature)
                        emit_progress(
                            "开始验证听牌方案\n"
                            f"搜索次数: {stats['branch_attempts']}\n"
                            f"当前节点类型: 双面子验证\n"
                            f"当前双杠1: {_quad_label(quad_pair['quad_ids'][:4], by_id)}\n"
                            f"当前双杠2: {_quad_label(quad_pair['quad_ids'][4:], by_id)}\n"
                            f"当前组合: {_component_text(component_descs)}",
                        )
                        plan, reason = _evaluate_candidate(
                            deck_map,
                            hand_ids,
                            replacement_ids,
                            by_id,
                            quad_pair["quad_ids"],
                            prewin_ids,
                            component_descs,
                            remaining_changes,
                            per_change_limit,
                        )
                        stats["reachability_checks"] += 1
                        if plan is not None:
                            plan["quad_faces"] = list(quad_pair["faces"])
                            plan["plan_signature"] = signature_text
                            stats["candidate_hands"] += 1
                            if stop_after_first:
                                if emit_candidate is not None:
                                    emit_candidate(plan)
                                emit_progress("方案验证成功\n" + _plan_summary(plan, deck_map), force=True)
                                return plan
                            if best_plan is None or plan["draws_needed"] < best_plan["draws_needed"]:
                                best_plan = plan
                                local_best_draws = plan["draws_needed"]
                                stats["latest_result"] = f"已找到更快方案: waits={','.join(plan['waits'])}"
                                if emit_candidate is not None:
                                    emit_candidate(plan)
                                emit_progress("方案验证成功\n" + _plan_summary(plan, deck_map), force=True)
                            continue
                        if reason.startswith("换牌可达性上界不足"):
                            stats["reachability_upper_prunes"] += 1
                        stats["latest_result"] = f"最近失败: {reason}"
                        emit_progress(
                            "方案验证失败\n"
                            f"当前双杠1: {_quad_label(quad_pair['quad_ids'][:4], by_id)}\n"
                            f"当前双杠2: {_quad_label(quad_pair['quad_ids'][4:], by_id)}\n"
                            f"当前组合: {_component_text(component_descs)}\n"
                            f"失败原因: {reason}",
                        )
            else:
                allow_replacement_after_win = _replacement_used_count(
                    list(quad_pair["quad_ids"]) + list(win_component["ids"]),
                    by_id,
                ) <= 13
                min_order_after_win = wall_start_order if _occupied_prefix_count(
                    quad_pair["quad_ids"], list(win_component["ids"]), None, by_id, wall_start_order
                ) > 13 else 0
                pairs = get_pairs(blocked, allow_replacement=allow_replacement_after_win, min_order=min_order_after_win)
                melds = get_melds(blocked, allow_replacement=allow_replacement_after_win, min_order=min_order_after_win)
                for pair_index, pair in enumerate(pairs, start=1):
                    blocked1 = blocked | {by_id[tile_id].order for tile_id in pair["ids"]}
                    if _target13_lower_bound(
                            quad_pair["quad_ids"],
                            list(pair["ids"]) + list(win_component["ids"]),
                            None,
                            by_id,
                    ) > 13:
                        stats["target13_prunes"] += 1
                        continue
                    stats["node_total"] = len(pairs)
                    stats["node_index"] = pair_index
                    stats["node_searchable"] = len(pool) - len(blocked1)
                    for meld_index, meld in enumerate(melds, start=1):
                        meld_orders = {by_id[tile_id].order for tile_id in meld["ids"]}
                        if blocked1 & meld_orders:
                            continue
                        if timed_out():
                            return best_plan
                        stats["node_total"] = len(melds)
                        stats["node_index"] = meld_index
                        stats["node_searchable"] = len(pool) - len(blocked1 | meld_orders)
                        component_descs = [pair["desc"], win_component["desc"], meld["desc"]]
                        prewin_ids = list(pair["ids"]) + list(win_component["ids"]) + list(meld["ids"])
                        if _occupied_prefix_count(
                                quad_pair["quad_ids"],
                                prewin_ids,
                                None,
                                by_id,
                                wall_start_order,
                        ) > 13:
                            stats["nonwall_prunes"] += 1
                            continue
                        signature = _candidate_signature(prewin_ids)
                        signature_text = _signature_to_text(signature)
                        if signature_text in skip_signatures:
                            stats["duplicate_prunes"] += 1
                            continue
                        if signature in seen_candidate_signatures:
                            stats["duplicate_prunes"] += 1
                            continue
                        seen_candidate_signatures.add(signature)
                        emit_progress(
                            "开始验证听牌方案\n"
                            f"搜索次数: {stats['branch_attempts']}\n"
                            f"当前节点类型: 雀头面子验证\n"
                            f"当前双杠1: {_quad_label(quad_pair['quad_ids'][:4], by_id)}\n"
                            f"当前双杠2: {_quad_label(quad_pair['quad_ids'][4:], by_id)}\n"
                            f"当前组合: {_component_text(component_descs)}",
                        )
                        plan, reason = _evaluate_candidate(
                            deck_map,
                            hand_ids,
                            replacement_ids,
                            by_id,
                            quad_pair["quad_ids"],
                            prewin_ids,
                            component_descs,
                            remaining_changes,
                            per_change_limit,
                        )
                        stats["reachability_checks"] += 1
                        if plan is not None:
                            plan["quad_faces"] = list(quad_pair["faces"])
                            plan["plan_signature"] = signature_text
                            stats["candidate_hands"] += 1
                            if stop_after_first:
                                if emit_candidate is not None:
                                    emit_candidate(plan)
                                emit_progress("方案验证成功\n" + _plan_summary(plan, deck_map), force=True)
                                return plan
                            if best_plan is None or plan["draws_needed"] < best_plan["draws_needed"]:
                                best_plan = plan
                                local_best_draws = plan["draws_needed"]
                                stats["latest_result"] = f"已找到更优方案 waits={','.join(plan['waits'])}"
                                if emit_candidate is not None:
                                    emit_candidate(plan)
                                emit_progress("方案验证成功\n" + _plan_summary(plan, deck_map), force=True)
                            continue
                        if reason.startswith("换牌可达性上界不足"):
                            stats["reachability_upper_prunes"] += 1
                        stats["latest_result"] = f"最近失败: {reason}"
                        emit_progress(
                            "方案验证失败\n"
                            f"当前双杠1: {_quad_label(quad_pair['quad_ids'][:4], by_id)}\n"
                            f"当前双杠2: {_quad_label(quad_pair['quad_ids'][4:], by_id)}\n"
                            f"当前组合: {_component_text(component_descs)}\n"
                            f"失败原因: {reason}",
                        )
    return best_plan


def _evaluate_tenpai_candidate(
        deck_map: Dict[int, str],
        hand_ids: Sequence[int],
        replacement_ids: Sequence[int],
        by_id: Dict[int, PoolEntry],
        quad_ids: Sequence[int],
        prewin_ids: Sequence[int],
        component_descs: Sequence[str],
        remaining_changes: int,
        per_change_limit: int,
) -> tuple[Optional[dict], str]:
    prewin_physical_ids = list(quad_ids) + list(prewin_ids)
    concealed_hand7 = [_norm(deck_map[tile_id]) for tile_id in prewin_ids]
    if len(concealed_hand7) != 7:
        return None, f"candidate-hand-size-invalid: {len(concealed_hand7)}"

    pair_wait_shape_reason = _non_souzu_pair_wait_shape_reason(concealed_hand7)
    if pair_wait_shape_reason is not None:
        return None, f"{pair_wait_shape_reason}; hand7={_hand13_debug_text(concealed_hand7)}"

    opened_face_cnt = Counter(_norm(deck_map[tile_id]) for tile_id in list(quad_ids))
    waits = _waits_for_open_two_melds_faces(concealed_hand7, opened_face_cnt)
    waits = sorted(waits, key=lambda tile: TILE_INDEX[tile])
    if not waits:
        return None, f"no-tenpai-wait; hand7={_hand13_debug_text(concealed_hand7)}"
    if any(not face.endswith("s") for face in waits):
        return None, f"non-souzu-waits: {','.join(waits)}"

    prewin_wall_entries = sorted(
        [by_id[tile_id] for tile_id in prewin_physical_ids if by_id[tile_id].source == "wall"],
        key=lambda entry: entry.source_index,
    )

    essential_nonwall_ids = {tile_id for tile_id in prewin_physical_ids if by_id[tile_id].source != "wall"}
    if len(essential_nonwall_ids) > 13:
        return None, f"too-many-nonwall-before-draw: {len(essential_nonwall_ids)}"
    kong_slot_gain = len(quad_ids) // 4
    filler_need = max(0, len(prewin_wall_entries) - kong_slot_gain)
    filler_ids = _select_initial_fillers(hand_ids, essential_nonwall_ids, filler_need, deck_map)
    if filler_ids is None:
        return None, f"not-enough-fillers: need={filler_need}"

    switch_target_ids = list(essential_nonwall_ids) + filler_ids
    if len(switch_target_ids) != 13:
        return None, f"target13-size-invalid: {len(switch_target_ids)}"
    ok, reason, switch_discards, switch_in, batches = _simulate_switch_reachability(
        hand_ids,
        prewin_physical_ids,
        replacement_ids,
        0,
        remaining_changes,
        per_change_limit,
        deck_map,
    )
    if not ok:
        return None, f"switch-reachability-failed: {reason}"
    ordered_wall_draws = [
        entry.tile_id
        for entry in sorted(
            {
                by_id[tile_id]
                for tile_id in list(quad_ids) + list(prewin_ids)
                if by_id[tile_id].source == "wall"
            },
            key=lambda entry: entry.source_index,
        )
    ]
    draws_needed = max((by_id[tile_id].source_index + 1 for tile_id in ordered_wall_draws), default=0)

    return {
        "status": "plan",
        "mode": "quad-first-dfs",
        "draws_needed": draws_needed,
        "switch_discards": switch_discards,
        "switch_in": switch_in,
        "switch_batch_sizes": batches,
        "wall_draws": ordered_wall_draws,
        "post_draw_discards": filler_ids,
        "waits": waits,
        "quad_faces": [],
        "target13": concealed_hand7,
        "remaining_changes": remaining_changes,
        "win_tile_faces": [],
        "win_tile_draw_index": None,
        "component_descs": list(component_descs),
        "retargeted_win_tile_id": None,
    }, "ok"


def _search_remaining_tenpai_plan(
        pool: Sequence[PoolEntry],
        by_id: Dict[int, PoolEntry],
        quad_pair: dict,
        deck_map: Dict[int, str],
        hand_ids: Sequence[int],
        replacement_ids: Sequence[int],
        remaining_changes: int,
        per_change_limit: int,
        stats: dict,
        emit_progress: Callable[[str], None],
        emit_candidate: Optional[Callable[[dict], None]],
        timed_out: Callable[[], bool],
        best_draws_limit: Optional[int] = None,
        stop_after_first: bool = False,
        skip_signatures: Optional[Set[str]] = None,
) -> Optional[dict]:
    quad_orders = {by_id[tile_id].order for tile_id in quad_pair["quad_ids"]}
    pair_cache: Dict[tuple[frozenset[int], bool, int], List[dict]] = {}
    meld_cache: Dict[tuple[frozenset[int], bool, int], List[dict]] = {}
    single_cache: Dict[tuple[frozenset[int], bool, int], List[dict]] = {}
    taatsu_cache: Dict[tuple[frozenset[int], bool, int], List[dict]] = {}
    seen_candidate_signatures: Set[tuple[int, ...]] = set()
    best_plan: Optional[dict] = None
    local_best_draws = best_draws_limit
    wall_start_order = len(hand_ids) + len(replacement_ids)
    skip_signatures = skip_signatures or set()

    def get_pairs(blocked_orders: Set[int], *, allow_replacement: bool, min_order: int) -> List[dict]:
        key = (_blocked_key(blocked_orders), allow_replacement, min_order)
        cached = pair_cache.get(key)
        if cached is not None:
            stats["state_cache_hits"] += 1
            return cached
        built = _pair_components(pool, blocked_orders, allow_replacement=allow_replacement, min_order=min_order)
        pair_cache[key] = built
        return built

    def get_melds(blocked_orders: Set[int], *, allow_replacement: bool, min_order: int) -> List[dict]:
        key = (_blocked_key(blocked_orders), allow_replacement, min_order)
        cached = meld_cache.get(key)
        if cached is not None:
            stats["state_cache_hits"] += 1
            return cached
        built = _meld_components(pool, blocked_orders, allow_replacement=allow_replacement, min_order=min_order)
        meld_cache[key] = built
        return built

    def get_singles(blocked_orders: Set[int], *, allow_replacement: bool, min_order: int) -> List[dict]:
        key = (_blocked_key(blocked_orders), allow_replacement, min_order)
        cached = single_cache.get(key)
        if cached is not None:
            stats["state_cache_hits"] += 1
            return cached
        built = _single_components(pool, blocked_orders, allow_replacement=allow_replacement, min_order=min_order)
        single_cache[key] = built
        return built

    def get_taatsus(blocked_orders: Set[int], *, allow_replacement: bool, min_order: int) -> List[dict]:
        key = (_blocked_key(blocked_orders), allow_replacement, min_order)
        cached = taatsu_cache.get(key)
        if cached is not None:
            stats["state_cache_hits"] += 1
            return cached
        built = _taatsu_components(pool, blocked_orders, allow_replacement=allow_replacement, min_order=min_order)
        taatsu_cache[key] = built
        return built

    def get_singles(blocked_orders: Set[int], *, allow_replacement: bool, min_order: int) -> List[dict]:
        key = (_blocked_key(blocked_orders), allow_replacement, min_order)
        cached = single_cache.get(key)
        if cached is not None:
            stats["state_cache_hits"] += 1
            return cached
        built = _single_components(pool, blocked_orders, allow_replacement=allow_replacement, min_order=min_order)
        single_cache[key] = built
        return built

    def get_taatsus(blocked_orders: Set[int], *, allow_replacement: bool, min_order: int) -> List[dict]:
        key = (_blocked_key(blocked_orders), allow_replacement, min_order)
        cached = taatsu_cache.get(key)
        if cached is not None:
            stats["state_cache_hits"] += 1
            return cached
        built = _taatsu_components(pool, blocked_orders, allow_replacement=allow_replacement, min_order=min_order)
        taatsu_cache[key] = built
        return built

    def try_candidate(prewin_ids: Sequence[int], component_descs: Sequence[str]) -> Optional[dict]:
        nonlocal best_plan, local_best_draws
        if _occupied_prefix_count(quad_pair["quad_ids"], prewin_ids, None, by_id, wall_start_order) > 13:
            stats["nonwall_prunes"] += 1
            return None
        signature = _candidate_signature(prewin_ids)
        signature_text = _signature_to_text(signature)
        if signature_text in skip_signatures or signature in seen_candidate_signatures:
            stats["duplicate_prunes"] += 1
            return None
        seen_candidate_signatures.add(signature)
        plan, reason = _evaluate_tenpai_candidate(
            deck_map,
            hand_ids,
            replacement_ids,
            by_id,
            quad_pair["quad_ids"],
            prewin_ids,
            component_descs,
            remaining_changes,
            per_change_limit,
        )
        stats["reachability_checks"] += 1
        if plan is None:
            if reason.startswith("换牌可达性上界不足"):
                stats["reachability_upper_prunes"] += 1
            stats["latest_result"] = f"最近失败: {reason}"
            return None
        plan["quad_faces"] = list(quad_pair["faces"])
        plan["plan_signature"] = signature_text
        stats["candidate_hands"] += 1
        if best_plan is None or plan["draws_needed"] < best_plan["draws_needed"]:
            best_plan = plan
            local_best_draws = plan["draws_needed"]
            stats["latest_result"] = f"已找到更快方案: waits={','.join(plan['waits'])}"
            if emit_candidate is not None:
                emit_candidate(plan)
            emit_progress("方案验证成功\n" + _plan_summary(plan, deck_map), force=True)
        if stop_after_first:
            return plan
        return plan

    emit_progress(
        "开始搜索目标 13 张听牌形\n"
        "说明: 固定双杠后，直接搜索能形成索子听牌的 7 张暗手\n"
        f"当前双杠: {_quad_label(quad_pair['quad_ids'][:4], by_id)} | {_quad_label(quad_pair['quad_ids'][4:], by_id)}",
        force=True,
    )

    base_allow_replacement = _replacement_used_count(quad_pair["quad_ids"], by_id) <= 13
    base_min_order = wall_start_order if _occupied_prefix_count(
        quad_pair["quad_ids"], [], None, by_id, wall_start_order
    ) > 13 else 0

    melds0 = get_melds(quad_orders, allow_replacement=base_allow_replacement, min_order=base_min_order)
    stats["node_total"] = len(melds0)

    for meld1_index, meld1 in enumerate(melds0, start=1):
        if timed_out():
            return best_plan
        blocked1 = quad_orders | {by_id[tile_id].order for tile_id in meld1["ids"]}
        if _target13_lower_bound(quad_pair["quad_ids"], list(meld1["ids"]), None, by_id) > 13:
            stats["target13_prunes"] += 1
            continue
        stats["branch_attempts"] += 1
        stats["dfs_nodes"] += 1
        stats["node_index"] = meld1_index
        stats["node_searchable"] = len(pool) - len(blocked1)

        allow_after_meld1 = _replacement_used_count(list(quad_pair["quad_ids"]) + list(meld1["ids"]), by_id) <= 13
        min_after_meld1 = wall_start_order if _occupied_prefix_count(
            quad_pair["quad_ids"], list(meld1["ids"]), None, by_id, wall_start_order
        ) > 13 else 0

        melds1 = get_melds(blocked1, allow_replacement=allow_after_meld1, min_order=min_after_meld1)
        for meld2 in melds1:
            if timed_out():
                return best_plan
            blocked2 = blocked1 | {by_id[tile_id].order for tile_id in meld2["ids"]}
            pre_ids = list(meld1["ids"]) + list(meld2["ids"])
            if _target13_lower_bound(quad_pair["quad_ids"], pre_ids, None, by_id) > 13:
                stats["target13_prunes"] += 1
                continue

            singles = get_singles(blocked2, allow_replacement=allow_after_meld1, min_order=min_after_meld1)
            for single in singles:
                plan = try_candidate(pre_ids + list(single["ids"]), [meld1["desc"], meld2["desc"], single["desc"]])
                if stop_after_first and plan is not None:
                    return plan

        pairs1 = get_pairs(blocked1, allow_replacement=allow_after_meld1, min_order=min_after_meld1)
        for pair1 in pairs1:
            if timed_out():
                return best_plan
            blocked_pair = blocked1 | {by_id[tile_id].order for tile_id in pair1["ids"]}
            pre_ids = list(meld1["ids"]) + list(pair1["ids"])
            if _target13_lower_bound(quad_pair["quad_ids"], pre_ids, None, by_id) > 13:
                stats["target13_prunes"] += 1
                continue

            pairs2 = get_pairs(blocked_pair, allow_replacement=allow_after_meld1, min_order=min_after_meld1)
            for pair2 in pairs2:
                if set(pair1["ids"]) & set(pair2["ids"]):
                    continue
                plan = try_candidate(pre_ids + list(pair2["ids"]), [meld1["desc"], pair1["desc"], pair2["desc"]])
                if stop_after_first and plan is not None:
                    return plan

            taatsus = get_taatsus(blocked_pair, allow_replacement=allow_after_meld1, min_order=min_after_meld1)
            for taatsu in taatsus:
                if set(pair1["ids"]) & set(taatsu["ids"]):
                    continue
                plan = try_candidate(pre_ids + list(taatsu["ids"]), [meld1["desc"], pair1["desc"], taatsu["desc"]])
                if stop_after_first and plan is not None:
                    return plan

    return best_plan


def _search_quad_pair_worker(
        pool: Sequence[PoolEntry],
        by_id: Dict[int, PoolEntry],
        quad_pair: dict,
        deck_map: Dict[int, str],
        hand_ids: Sequence[int],
        replacement_ids: Sequence[int],
        remaining_changes: int,
        per_change_limit: int,
        best_draws_limit: Optional[int],
        stop_after_first: bool,
        skip_signatures: Optional[Set[str]],
) -> dict:
    stats = {
        "dfs_nodes": 0,
        "branch_attempts": 0,
        "candidate_hands": 0,
        "reachability_checks": 0,
        "duplicate_prunes": 0,
        "state_cache_hits": 0,
        "target13_prunes": 0,
        "nonwall_prunes": 0,
        "reachability_upper_prunes": 0,
    }
    plan = _search_remaining_plan(
        pool,
        by_id,
        quad_pair,
        deck_map,
        hand_ids,
        replacement_ids,
        remaining_changes,
        per_change_limit,
        stats,
        lambda *_args, **_kwargs: None,
        None,
        lambda: False,
        best_draws_limit,
        stop_after_first,
        skip_signatures,
    )
    if plan is not None:
        plan["quad_faces"] = list(quad_pair["faces"])
    return {
        "quad_pair": quad_pair,
        "plan": plan,
        "stats": stats,
    }


def _register_active_executor(executor: ProcessPoolExecutor) -> None:
    with _ACTIVE_EXECUTORS_LOCK:
        _ACTIVE_EXECUTORS.add(executor)


def _unregister_active_executor(executor: ProcessPoolExecutor) -> None:
    with _ACTIVE_EXECUTORS_LOCK:
        _ACTIVE_EXECUTORS.discard(executor)


def _process_snapshot(process: object) -> dict:
    pid = getattr(process, "pid", None)
    alive = False
    try:
        alive = bool(process.is_alive())
    except Exception:
        pass
    exitcode = getattr(process, "exitcode", None)
    if alive:
        status = "running"
    elif exitcode is None:
        status = "idle"
    elif exitcode == 0:
        status = "exited"
    else:
        status = "terminated"
    return {
        "pid": int(pid) if pid is not None else None,
        "alive": alive,
        "exitcode": exitcode,
        "status": status,
    }


def _executor_process_snapshot(executor: ProcessPoolExecutor) -> List[dict]:
    process_map = getattr(executor, "_processes", None) or {}
    return [
        snapshot
        for snapshot in (
            _process_snapshot(process)
            for process in list(process_map.values())
        )
        if snapshot.get("pid") is not None
    ]


def get_active_search_runtime_snapshot(*, searching: bool = False) -> dict:
    processes: List[dict] = []
    with _ACTIVE_EXECUTORS_LOCK:
        executors = list(_ACTIVE_EXECUTORS)
    for executor in executors:
        try:
            processes.extend(_executor_process_snapshot(executor))
        except Exception:
            continue
    processes.sort(key=lambda item: int(item.get("pid") or 0))
    return {
        "searching": bool(searching),
        "process_count": len(processes),
        "processes": processes,
        "updated_at": round(time.time(), 3),
    }


def terminate_active_search_workers() -> dict:
    with _ACTIVE_EXECUTORS_LOCK:
        executors = list(_ACTIVE_EXECUTORS)
    for executor in executors:
        try:
            _terminate_executor_processes(executor)
        except Exception:
            pass
        try:
            executor.shutdown(wait=False, cancel_futures=True)
        except Exception:
            pass
    return get_active_search_runtime_snapshot(searching=False)


def _worker_quad_label(quad_pair: dict, by_id: Dict[int, PoolEntry]) -> str:
    return (
        f"{_quad_label(quad_pair['quad_ids'][:4], by_id)} | "
        f"{_quad_label(quad_pair['quad_ids'][4:], by_id)}"
    )


def _init_search_worker() -> None:
    try:
        signal.signal(signal.SIGINT, signal.SIG_IGN)
    except Exception:
        pass


def _terminate_executor_processes(executor: ProcessPoolExecutor, *, wait_timeout: float = 2.0) -> None:
    process_map = getattr(executor, "_processes", None) or {}
    processes = list(process_map.values())
    for proc in processes:
        try:
            if proc is not None and proc.is_alive():
                proc.terminate()
        except Exception:
            pass
    deadline = time.monotonic() + max(0.1, wait_timeout)
    for proc in processes:
        try:
            remaining = max(0.0, deadline - time.monotonic())
            if proc is not None:
                proc.join(timeout=remaining)
        except Exception:
            pass
    for proc in processes:
        try:
            if proc is not None and proc.is_alive() and hasattr(proc, "kill"):
                proc.kill()
        except Exception:
            pass
    for proc in processes:
        try:
            if proc is not None:
                proc.join(timeout=0.2)
        except Exception:
            pass


def recommend_souzu_tenpai_switch(
        deck_map: Dict[int, str],
        hand_ids: List[int],
        replacement_ids: List[int],
        wall_ids: List[int],
        switch_used_tiles: List[int],
        total_change_tile_count: int,
        change_tile_count: int,
        boss_buff: Optional[Sequence[int]] = None,
        progress_cb: Optional[Callable[[str], None]] = None,
        candidate_cb: Optional[Callable[[dict], None]] = None,
        telemetry_cb: Optional[Callable[[dict], None]] = None,
        should_stop: Optional[Callable[[], bool]] = None,
        stop_after_first: bool = False,
        skip_signatures: Optional[Set[str]] = None,
) -> dict:
    started_at = time.monotonic()
    remaining_changes = max(0, int(total_change_tile_count or 0) - int(change_tile_count or 0))
    # 换三张debuff
    per_change_limit = 3 if 901 in (boss_buff or []) else 13
    used_count = len(switch_used_tiles or [])
    remaining_replacements = _limit_replacement_search_window(
        replacement_ids[used_count:],
        remaining_changes,
        per_change_limit,
    )

    stats = {
        "quads": 0,
        "quad_pairs": 0,
        "current_quad_pair": 0,
        "dfs_nodes": 0,
        "branch_attempts": 0,
        "candidate_hands": 0,
        "reachability_checks": 0,
        "latest_result": "尚无结果",
        "node_total": 0,
        "node_index": 0,
        "node_searchable": 0,
        "duplicate_prunes": 0,
        "state_cache_hits": 0,
        "target13_prunes": 0,
        "nonwall_prunes": 0,
        "reachability_upper_prunes": 0,
        "speed_prunes": 0,
        "last_node_souzu_prunes": 0,
    }
    last_progress_emit_at = 0.0
    worker_states: List[dict] = []
    parallel_info: dict = {"mode": "single", "enabled": False, "max_workers": 1, "total_jobs": 0, "completed_jobs": 0}
    parallel_fallback_reason: Optional[str] = None
    cpu_count = os.cpu_count() or 1
    parallel_attempted = False
    parallel_disabled_reason: Optional[str] = None
    parallel_start_error: Optional[str] = None

    def stop_requested() -> bool:
        return bool(should_stop and should_stop())

    def emit_telemetry() -> None:
        if telemetry_cb is None:
            return
        now = time.monotonic()
        snapshot: List[dict] = []
        for item in worker_states:
            copied = dict(item)
            started_at = copied.get("started_at")
            copied["elapsed_sec"] = round(max(0.0, now - float(started_at)), 2) if started_at is not None else 0.0
            copied.pop("started_at", None)
            snapshot.append(copied)
        telemetry_cb({
            "worker_states": snapshot,
            "parallel_info": {
                **dict(parallel_info),
                "cpu_count": cpu_count,
                "quad_pair_count": len(quad_pairs),
                "parallel_ok": parallel_ok,
                "attempted": parallel_attempted,
                **({"disabled_reason": parallel_disabled_reason} if parallel_disabled_reason else {}),
                **({"start_error": parallel_start_error} if parallel_start_error else {}),
                **({"fallback_reason": parallel_fallback_reason} if parallel_fallback_reason else {}),
            },
            "runtime": get_active_search_runtime_snapshot(searching=True),
        })

    def emit_progress(text: str, *, force: bool = False) -> None:
        nonlocal last_progress_emit_at
        if progress_cb is None:
            return
        now = time.monotonic()
        if not force and (now - last_progress_emit_at) < 0.15:
            return
        last_progress_emit_at = now
        elapsed = max(time.monotonic() - started_at, 1e-6)
        speed = stats["reachability_checks"] / elapsed
        progress_cb(
            "\n".join([
                text,
                f"已耗时: {elapsed:.1f} 秒",
                f"剩余换牌次数: {remaining_changes}",
                f"已完成搜索次数: {stats['reachability_checks']}",
                f"搜索速度: {speed:.1f} 次/秒",
                f"可成杠数量: {stats['quads']}",
                f"双杠候选数: {stats['quad_pairs']}",
                f"当前双杠序号: {stats['current_quad_pair']}",
                f"目标 14 搜索节点: {stats['dfs_nodes']}",
                f"分支搜索次数: {stats['branch_attempts']}",
                f"当前节点可搜牌数: {stats['node_searchable']}",
                f"当前节点进度: {stats['node_index']} / {stats['node_total']}",
                f"已找到候选方案: {stats['candidate_hands']}",
                f"重复分支剪枝: {stats['duplicate_prunes']}",
                f"状态缓存命中: {stats['state_cache_hits']}",
                f"目标 13 下界剪枝: {stats['target13_prunes']}",
                f"非牌山超限剪枝: {stats['nonwall_prunes']}",
                f"换牌上界剪枝: {stats['reachability_upper_prunes']}",
                f"速度劣化剪枝: {stats['speed_prunes']}",
                f"最后节点非索剪枝: {stats['last_node_souzu_prunes']}",
                f"换牌可达性校验: {stats['reachability_checks']}",
                f"当前最新结果: {stats['latest_result']}",
            ])
        )

    if len(hand_ids) != 13:
        return {"status": "impossible", "reason": "switch-hand-must-be-13"}
    if len(wall_ids) < 1:
        return {"status": "impossible", "reason": "wall-less-than-two-draws"}

    emit_progress("正在准备搜索\n当前阶段: 构建牌池", force=True)
    pool, by_id = _build_pool(deck_map, hand_ids, remaining_replacements, wall_ids)
    quads = _enumerate_quads(pool, by_id)
    quad_pairs = _enumerate_quad_pairs(quads)
    debug_pool = _build_debug_pool_info(
        deck_map,
        hand_ids,
        remaining_replacements,
        wall_ids,
        quads,
        by_id,
        replacement_total_remaining=max(0, len(replacement_ids) - used_count),
        replacement_window_count=len(remaining_replacements),
        replacement_used_count=used_count,
    )
    stats["quads"] = len(quads)
    stats["quad_pairs"] = len(quad_pairs)

    emit_progress("正在搜索双杠组合", force=True)
    if quads:
        quad_lines = [
            f"{idx}. {_quad_label(quad['ids'], by_id)} | 速度评分: {_quad_score_text(quad['time_key'])}"
            for idx, quad in enumerate(quads, start=1)
        ]
        emit_progress("杠搜索完成，所有可成杠如下\n" + "\n".join(quad_lines), force=True)

    if not quad_pairs:
        return {
            "status": "impossible",
            "reason": "cannot-form-two-quads",
            "remaining_changes": remaining_changes,
            "debug_pool": debug_pool,
            "runtime": get_active_search_runtime_snapshot(searching=False),
        }

    best_plan: Optional[dict] = None
    best_draws_limit: Optional[int] = None
    parallel_ok = len(quad_pairs) >= 2 and cpu_count > 1
    if len(quad_pairs) < 2:
        parallel_disabled_reason = "候选双杠不足，至少需要 2 组候选双杠才会启用并行搜索"
    elif cpu_count <= 1:
        parallel_disabled_reason = f"os.cpu_count() 返回 {cpu_count}，未达到并行搜索所需的最小值 2"
    if parallel_ok:
        parallel_attempted = True
        max_workers = max(1, min(len(quad_pairs), max(1, cpu_count - 1), 4))
        completed = 0
        parallel_info = {"mode": "process", "enabled": True, "max_workers": max_workers, "total_jobs": len(quad_pairs), "completed_jobs": 0}
        worker_states = [
            {
                "worker_id": idx + 1,
                "kind": "process",
                "status": "idle",
                "current_quad_index": None,
                "current_quad_label": "",
                "completed_jobs": 0,
                "last_result": "",
                "last_draws_needed": None,
                "started_at": None,
            }
            for idx in range(max_workers)
        ]
        emit_telemetry()
        emit_progress(f"正在并行验证双杠组合\n候选数: {len(quad_pairs)}\n并行进程数: {max_workers}", force=True)
        executor: Optional[ProcessPoolExecutor] = None
        try:
            executor = ProcessPoolExecutor(max_workers=max_workers, initializer=_init_search_worker)
            _register_active_executor(executor)
            future_map: Dict[Any, tuple[int, int, dict]] = {}
            next_quad_index = 0

            def submit_one(worker_slot: int) -> bool:
                nonlocal next_quad_index
                if next_quad_index >= len(quad_pairs):
                    return False
                quad_index = next_quad_index
                quad_pair = quad_pairs[quad_index]
                next_quad_index += 1
                worker_states[worker_slot].update({
                    "status": "running",
                    "current_quad_index": quad_index + 1,
                    "current_quad_label": _worker_quad_label(quad_pair, by_id),
                    "started_at": time.monotonic(),
                })
                future = executor.submit(
                    _search_quad_pair_worker,
                    pool,
                    by_id,
                    quad_pair,
                    deck_map,
                    hand_ids,
                    remaining_replacements,
                    remaining_changes,
                    per_change_limit,
                    best_draws_limit,
                    stop_after_first,
                    set(skip_signatures or []),
                )
                future_map[future] = (worker_slot, quad_index, quad_pair)
                return True

            for worker_slot in range(max_workers):
                if not submit_one(worker_slot):
                    break
            emit_telemetry()
            pending = set(future_map.keys())
            while pending:
                if stop_requested():
                    _terminate_executor_processes(executor)
                    executor.shutdown(wait=False, cancel_futures=True)
                    for worker in worker_states:
                        if worker["status"] == "running":
                            worker["status"] = "stopped"
                    parallel_info["completed_jobs"] = completed
                    emit_telemetry()
                    return {
                        "status": "impossible",
                        "reason": "stopped-by-user",
                        "remaining_changes": remaining_changes,
                        "debug_pool": debug_pool,
                        "runtime": get_active_search_runtime_snapshot(searching=False),
                    }
                done, pending = wait(pending, timeout=0.1, return_when=FIRST_COMPLETED)
                if not done:
                    emit_telemetry()
                    continue
                for future in done:
                    result = future.result()
                    completed += 1
                    worker_slot, quad_index, quad_pair = future_map.pop(future)
                    worker = worker_states[worker_slot]
                    worker["completed_jobs"] = int(worker.get("completed_jobs", 0)) + 1
                    worker["status"] = "completed"
                    worker["last_result"] = "found-plan" if result.get("plan") is not None else "no-plan"
                    worker["last_draws_needed"] = result.get("plan", {}).get("draws_needed") if result.get("plan") is not None else None
                    worker_stats = result.get("stats") or {}
                    stats["current_quad_pair"] = completed
                    parallel_info["completed_jobs"] = completed
                    for key in (
                            "dfs_nodes",
                            "branch_attempts",
                            "candidate_hands",
                            "reachability_checks",
                            "duplicate_prunes",
                            "state_cache_hits",
                            "target13_prunes",
                            "nonwall_prunes",
                            "reachability_upper_prunes",
                    ):
                        stats[key] += int(worker_stats.get(key, 0) or 0)
                    stats["latest_result"] = f"已完成双杠 {completed}/{len(quad_pairs)}"
                    emit_progress(
                        "并行验证双杠组合\n"
                        f"已完成: {completed}/{len(quad_pairs)}\n"
                        f"当前双杠1: {_quad_label(quad_pair['quad_ids'][:4], by_id)}\n"
                        f"当前双杠2: {_quad_label(quad_pair['quad_ids'][4:], by_id)}",
                        force=True,
                    )
                    emit_telemetry()
                    plan = result.get("plan")
                    if plan is not None:
                        if best_plan is None or plan["draws_needed"] < best_plan["draws_needed"]:
                            best_plan = plan
                            best_draws_limit = plan["draws_needed"]
                            stats["latest_result"] = f"已更新当前最优方案，需要摸 {best_draws_limit}"
                            if candidate_cb is not None:
                                candidate_cb(plan)
                            emit_progress("已更新当前最优方案", force=True)
                            emit_telemetry()
                        if stop_after_first:
                            _terminate_executor_processes(executor)
                            executor.shutdown(wait=False, cancel_futures=True)
                            for item in worker_states:
                                if item["status"] == "running":
                                    item["status"] = "stopped"
                            parallel_info["completed_jobs"] = completed
                            best_plan["worker_states"] = [{k: v for k, v in item.items() if k != "started_at"} for item in worker_states]
                            best_plan["parallel_info"] = parallel_info
                            best_plan["debug_pool"] = debug_pool
                            best_plan["runtime"] = get_active_search_runtime_snapshot(searching=False)
                            return best_plan
                        if best_draws_limit is not None and best_draws_limit <= 1:
                            _terminate_executor_processes(executor)
                            executor.shutdown(wait=False, cancel_futures=True)
                            for item in worker_states:
                                if item["status"] == "running":
                                    item["status"] = "stopped"
                            parallel_info["completed_jobs"] = completed
                            best_plan["worker_states"] = [{k: v for k, v in item.items() if k != "started_at"} for item in worker_states]
                            best_plan["parallel_info"] = parallel_info
                            best_plan["debug_pool"] = debug_pool
                            best_plan["runtime"] = get_active_search_runtime_snapshot(searching=False)
                            return best_plan
                    worker["status"] = "idle" if next_quad_index < len(quad_pairs) else "done"
                    worker["current_quad_index"] = None
                    worker["current_quad_label"] = ""
                    worker["started_at"] = None
                    submit_one(worker_slot)
                    pending = set(future_map.keys())
                    emit_telemetry()
            for item in worker_states:
                if item["status"] not in ("stopped", "done"):
                    item["status"] = "done"
            if best_plan is not None:
                parallel_info["completed_jobs"] = completed
                best_plan["worker_states"] = [{k: v for k, v in item.items() if k != "started_at"} for item in worker_states]
                best_plan["parallel_info"] = parallel_info
                best_plan["debug_pool"] = debug_pool
                best_plan["runtime"] = get_active_search_runtime_snapshot(searching=False)
                return best_plan
            return {
                "status": "impossible",
                "reason": "no-reliable-plan-found",
                "remaining_changes": remaining_changes,
                "debug_pool": debug_pool,
                "worker_states": [{k: v for k, v in item.items() if k != "started_at"} for item in worker_states],
                "parallel_info": parallel_info,
                "runtime": get_active_search_runtime_snapshot(searching=False),
            }
        except Exception as exc:
            process_snapshot = []
            try:
                if executor is not None:
                    process_snapshot = _executor_process_snapshot(executor)
            except Exception:
                process_snapshot = []
            try:
                if executor is not None:
                    _terminate_executor_processes(executor)
                    executor.shutdown(wait=False, cancel_futures=True)
            except Exception:
                pass
            if isinstance(exc, BrokenProcessPool):
                logger.error(
                    "parallel search worker crashed: {} | processes={}",
                    exc,
                    process_snapshot,
                )
            logger.exception("parallel search startup/execution failed")
            parallel_traceback = traceback.format_exc()
            print(parallel_traceback, flush=True)
            parallel_start_error = str(exc)
            parallel_fallback_reason = str(exc)
            emit_progress(f"并行搜索不可用，回退为单进程搜索: {exc}", force=True)

        finally:
            if executor is not None:
                _unregister_active_executor(executor)

    parallel_info = {
        "mode": "single",
        "enabled": False,
        "max_workers": 1,
        "total_jobs": len(quad_pairs),
        "completed_jobs": 0,
        "cpu_count": cpu_count,
        "quad_pair_count": len(quad_pairs),
        "parallel_ok": parallel_ok,
        "attempted": parallel_attempted,
    }
    if parallel_disabled_reason:
        parallel_info["disabled_reason"] = parallel_disabled_reason
    if parallel_start_error:
        parallel_info["start_error"] = parallel_start_error
    if parallel_fallback_reason:
        parallel_info["fallback_reason"] = parallel_fallback_reason
    worker_states = [{
        "worker_id": 1,
        "kind": "main",
        "status": "running",
        "current_quad_index": None,
        "current_quad_label": "",
        "completed_jobs": 0,
        "last_result": "",
        "last_draws_needed": None,
        "started_at": time.monotonic(),
    }]
    emit_telemetry()
    for quad_index, quad_pair in enumerate(quad_pairs, start=1):
        if stop_requested():
            worker_states[0]["status"] = "stopped"
            parallel_info["completed_jobs"] = quad_index - 1
            return {
                "status": "impossible",
                "reason": "stopped-by-user",
                "remaining_changes": remaining_changes,
                "debug_pool": debug_pool,
                "worker_states": [{k: v for k, v in worker_states[0].items() if k != "started_at"}],
                "parallel_info": parallel_info,
                "parallel_fallback_reason": parallel_fallback_reason,
                "runtime": get_active_search_runtime_snapshot(searching=False),
            }
        stats["current_quad_pair"] = quad_index
        worker_states[0]["current_quad_index"] = quad_index
        worker_states[0]["current_quad_label"] = _worker_quad_label(quad_pair, by_id)
        worker_states[0]["started_at"] = time.monotonic()
        emit_telemetry()
        stats["latest_result"] = f"正在验证双杠 {quad_index}/{len(quad_pairs)}"
        emit_progress(
            "正在验证双杠组合\n"
            f"双杠评分: {quad_pair['pair_score']}\n"
            f"一杠: {_quad_label(quad_pair['quad_ids'][:4], by_id)}\n"
            f"二杠: {_quad_label(quad_pair['quad_ids'][4:], by_id)}",
            force=True,
        )
        plan = _search_remaining_plan(
            pool,
            by_id,
            quad_pair,
            deck_map,
            hand_ids,
            remaining_replacements,
            remaining_changes,
            per_change_limit,
            stats,
            emit_progress,
            candidate_cb,
            stop_requested,
            best_draws_limit,
            stop_after_first,
            skip_signatures,
        )
        if plan is not None:
            plan["quad_faces"] = list(quad_pair["faces"])
            if best_plan is None or plan["draws_needed"] < best_plan["draws_needed"]:
                best_plan = plan
                best_draws_limit = plan["draws_needed"]
                stats["latest_result"] = f"已更新当前最优方案，需要摸 {best_draws_limit}"
                emit_progress("已更新当前最优方案", force=True)
                if best_draws_limit <= 1:
                    worker_states[0]["status"] = "done"
                    worker_states[0]["completed_jobs"] = quad_index
                    worker_states[0]["last_result"] = "found-plan"
                    worker_states[0]["last_draws_needed"] = best_draws_limit
                    parallel_info["completed_jobs"] = quad_index
                    best_plan["worker_states"] = [{k: v for k, v in worker_states[0].items() if k != "started_at"}]
                    best_plan["parallel_info"] = parallel_info
                    best_plan["parallel_fallback_reason"] = parallel_fallback_reason
                    best_plan["debug_pool"] = debug_pool
                    best_plan["runtime"] = get_active_search_runtime_snapshot(searching=False)
                    return best_plan
            if stop_after_first:
                worker_states[0]["status"] = "done"
                worker_states[0]["completed_jobs"] = quad_index
                worker_states[0]["last_result"] = "found-plan"
                worker_states[0]["last_draws_needed"] = plan["draws_needed"]
                parallel_info["completed_jobs"] = quad_index
                best_plan["worker_states"] = [{k: v for k, v in worker_states[0].items() if k != "started_at"}]
                best_plan["parallel_info"] = parallel_info
                best_plan["parallel_fallback_reason"] = parallel_fallback_reason
                best_plan["debug_pool"] = debug_pool
                best_plan["runtime"] = get_active_search_runtime_snapshot(searching=False)
                return best_plan
        worker_states[0]["completed_jobs"] = quad_index
        worker_states[0]["last_result"] = "found-plan" if plan is not None else "no-plan"
        worker_states[0]["last_draws_needed"] = plan.get("draws_needed") if plan is not None else None
        parallel_info["completed_jobs"] = quad_index
        emit_telemetry()

    if stop_requested():
        worker_states[0]["status"] = "stopped"
        return {
            "status": "impossible",
            "reason": "stopped-by-user",
            "remaining_changes": remaining_changes,
            "debug_pool": debug_pool,
            "worker_states": [{k: v for k, v in worker_states[0].items() if k != "started_at"}],
            "parallel_info": parallel_info,
            "parallel_fallback_reason": parallel_fallback_reason,
            "runtime": get_active_search_runtime_snapshot(searching=False),
        }
    if best_plan is not None:
        worker_states[0]["status"] = "done"
        best_plan["worker_states"] = [{k: v for k, v in worker_states[0].items() if k != "started_at"}]
        best_plan["parallel_info"] = parallel_info
        best_plan["parallel_fallback_reason"] = parallel_fallback_reason
        best_plan["debug_pool"] = debug_pool
        best_plan["runtime"] = get_active_search_runtime_snapshot(searching=False)
        return best_plan
    worker_states[0]["status"] = "done"
    return {
        "status": "impossible",
        "reason": "no-reliable-plan-found",
        "remaining_changes": remaining_changes,
        "debug_pool": debug_pool,
        "worker_states": [{k: v for k, v in worker_states[0].items() if k != "started_at"}],
        "parallel_info": parallel_info,
        "parallel_fallback_reason": parallel_fallback_reason,
        "runtime": get_active_search_runtime_snapshot(searching=False),
    }
