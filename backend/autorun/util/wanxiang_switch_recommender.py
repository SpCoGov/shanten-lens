from collections import defaultdict
from itertools import combinations
from typing import Callable, Dict, List, Optional, Sequence, Set

from backend.autorun.util.souzu_switch_recommender import (
    RED_MAP,
    TILE_INDEX,
    PoolEntry,
    _build_debug_pool_info,
    _build_pool,
    _candidate_ids,
    _component_sort_prefix,
    _entry_label,
    _limit_replacement_search_window,
    _norm,
    _parse_face,
    _simulate_switch_reachability,
)

WANXIANG_TILE_ID = 1000
WANXIANG_FACE = "bd"


def _meld_time(ids: Sequence[int], by_id: Dict[int, PoolEntry]) -> tuple[int, int, int]:
    entries = [by_id[int(tile_id)] for tile_id in ids]
    return (
        max(entry.order for entry in entries),
        max((entry.source_index + 1 for entry in entries if entry.source == "replacement"), default=0),
        max((entry.source_index + 1 for entry in entries if entry.source == "wall"), default=0),
    )


def _meld_desc(face_list: Sequence[str], ids: Sequence[int], by_id: Dict[int, PoolEntry]) -> str:
    shape = "".join(sorted(face_list, key=lambda tile: TILE_INDEX.get(tile, 99)))
    entries = " / ".join(_entry_label(by_id[int(tile_id)]) for tile_id in ids)
    return f"面子[{shape}] <- {entries}"


def _enumerate_meld_components(pool: Sequence[PoolEntry], by_id: Dict[int, PoolEntry]) -> List[dict]:
    by_face: Dict[str, List[PoolEntry]] = defaultdict(list)
    for entry in pool:
        if entry.tile_id == WANXIANG_TILE_ID or entry.face == WANXIANG_FACE:
            continue
        by_face[entry.face].append(entry)

    result: List[dict] = []
    seen: Set[tuple[int, ...]] = set()

    for face, entries in by_face.items():
        if len(entries) < 3:
            continue
        for combo in combinations(entries, 3):
            ids = tuple(sorted(entry.tile_id for entry in combo))
            if ids in seen:
                continue
            seen.add(ids)
            faces = (face, face, face)
            result.append({
                "ids": ids,
                "faces": faces,
                "desc": _meld_desc(faces, ids, by_id),
                "time_key": _meld_time(ids, by_id),
                "sort_key": (_meld_time(ids, by_id), 0, TILE_INDEX.get(face, 99), ids),
            })

    for face, entries_a in by_face.items():
        rank, suit = _parse_face(face)
        if suit not in "mps" or rank > 7:
            continue
        face_b = f"{rank + 1}{suit}"
        face_c = f"{rank + 2}{suit}"
        if face_b not in by_face or face_c not in by_face:
            continue
        for a in entries_a:
            for b in by_face[face_b]:
                for c in by_face[face_c]:
                    ids = tuple(sorted((a.tile_id, b.tile_id, c.tile_id)))
                    if ids in seen:
                        continue
                    seen.add(ids)
                    faces = (face, face_b, face_c)
                    result.append({
                        "ids": ids,
                        "faces": faces,
                        "desc": _meld_desc(faces, ids, by_id),
                        "time_key": _meld_time(ids, by_id),
                        "sort_key": (_meld_time(ids, by_id), 1, TILE_INDEX.get(face, 99), ids),
                    })

    result.sort(key=_component_sort_prefix)
    return result


def _target_signature(melds: Sequence[dict]) -> str:
    ids = sorted(int(tile_id) for meld in melds for tile_id in meld.get("ids", ()))
    return "wanxiang|" + ",".join(str(tile_id) for tile_id in ids)


def _target_faces(deck_map: Dict[int, str], ids: Sequence[int]) -> List[str]:
    return [_norm(deck_map[int(tile_id)]) for tile_id in ids]


def recommend_wanxiang_four_meld_switch(
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
        skip_signatures: Optional[Set[str]] = None,
        auto_wall_limit: bool = True,
        search_algorithm: Optional[str] = None,
) -> dict:
    del telemetry_cb, auto_wall_limit, search_algorithm
    remaining_changes = max(0, int(total_change_tile_count or 0) - int(change_tile_count or 0))
    per_change_limit = 3 if 901 in (boss_buff or []) else 13
    used_count = len(switch_used_tiles or [])
    remaining_replacements = _limit_replacement_search_window(
        replacement_ids[used_count:],
        remaining_changes,
        per_change_limit,
    )

    if len(hand_ids) != 13:
        return {"status": "impossible", "reason": "switch-hand-must-be-13"}
    if WANXIANG_TILE_ID not in hand_ids:
        return {"status": "impossible", "reason": "wanxiang-not-in-hand"}

    if progress_cb is not None:
        progress_cb("万象四面子搜索\n当前阶段: 构建牌池")

    pool, by_id = _build_pool(deck_map, hand_ids, remaining_replacements, [])
    melds = _enumerate_meld_components(pool, by_id)
    debug_pool = _build_debug_pool_info(
        deck_map,
        hand_ids,
        remaining_replacements,
        wall_ids,
        [],
        by_id,
        replacement_total_remaining=max(0, len(replacement_ids) - used_count),
        replacement_window_count=len(remaining_replacements),
        replacement_used_count=used_count,
        focus_face=WANXIANG_FACE,
    )

    if not melds:
        return {
            "status": "impossible",
            "reason": "cannot-form-four-melds-with-wanxiang",
            "remaining_changes": remaining_changes,
            "debug_pool": debug_pool,
        }

    seen: Set[str] = set()
    skip_signatures = skip_signatures or set()
    checks = 0

    def stopped() -> bool:
        return bool(should_stop and should_stop())

    def overlaps(groups: Sequence[dict]) -> bool:
        used: Set[int] = set()
        for group in groups:
            for raw_id in group.get("ids", ()):
                tile_id = int(raw_id)
                if tile_id in used:
                    return True
                used.add(tile_id)
        return False

    def visit(start_index: int, chosen: List[dict], used_ids: Set[int]) -> Optional[dict]:
        nonlocal checks
        if stopped():
            return {"status": "impossible", "reason": "stopped-by-user"}
        if len(chosen) == 4:
            signature = _target_signature(chosen)
            if signature in seen or signature in skip_signatures:
                return None
            seen.add(signature)

            meld_ids = [int(tile_id) for group in chosen for tile_id in group.get("ids", ())]
            occupied_ids = [WANXIANG_TILE_ID] + meld_ids
            checks += 1
            ok, reason, switch_discards, switch_in, batches = _simulate_switch_reachability(
                hand_ids,
                occupied_ids,
                remaining_replacements,
                0,
                remaining_changes,
                per_change_limit,
                deck_map,
            )
            if not ok:
                return None

            target13_ids = _candidate_ids(occupied_ids, deck_map)
            target13_faces = _target_faces(deck_map, target13_ids)
            component_descs = [
                "万象固定作雀头其中一张，禁止换出。",
                *[str(group.get("desc") or "") for group in chosen],
            ]
            draws_needed = max(
                (by_id[tile_id].source_index + 1 for tile_id in meld_ids if by_id[tile_id].source == "replacement"),
                default=0,
            )
            plan = {
                "status": "plan",
                "mode": "wanxiang-four-meld-switch",
                "draws_needed": draws_needed,
                "switch_discards": switch_discards,
                "switch_in": switch_in,
                "switch_batch_sizes": batches,
                "wall_draws": [],
                "post_draw_discards": [],
                "waits": [],
                "quad_faces": [],
                "target13": target13_faces,
                "target14": target13_faces,
                "remaining_changes": remaining_changes,
                "component_descs": component_descs,
                "plan_signature": signature,
                "debug_pool": debug_pool,
            }
            if candidate_cb is not None:
                candidate_cb(plan)
            return plan

        remaining_slots = 4 - len(chosen)
        for index in range(start_index, len(melds) - remaining_slots + 1):
            if progress_cb is not None and len(chosen) == 0 and index % 100 == 0:
                progress_cb(
                    "\n".join([
                        "万象四面子搜索",
                        f"当前面子节点: {index + 1} / {len(melds)}",
                        f"已完成可达性校验: {checks}",
                    ])
                )
            meld = melds[index]
            meld_ids = {int(tile_id) for tile_id in meld.get("ids", ())}
            if used_ids & meld_ids:
                continue
            found = visit(index + 1, [*chosen, meld], used_ids | meld_ids)
            if isinstance(found, dict):
                return found
        return None

    early_plan = visit(0, [], set())
    if isinstance(early_plan, dict) and early_plan.get("status") == "plan":
        return early_plan
    if isinstance(early_plan, dict):
        return early_plan

    return {
        "status": "impossible",
        "reason": "cannot-form-four-melds-with-wanxiang",
        "remaining_changes": remaining_changes,
        "debug_pool": debug_pool,
    }
