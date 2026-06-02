import asyncio
import copy
import threading
import time
from typing import Tuple, Any, Dict, List, Set, Optional, Union, Sequence

from loguru import logger

import backend.app
from backend.app import AMULET_REG, BADGE_REG
from backend.app import MANAGER, GAME_STATE, broadcast
from backend.autorun.util.chiitoi_recommender import chiitoi_recommendation_json
from backend.autorun.util.retry_1004 import call_with_1004_retry_async
from backend.autorun.util.souzu_switch_recommender import (
    recommend_souzu_tenpai_switch,
    list_reachable_quads_for_switch,
    validate_manual_souzu_switch_plan,
    SOUZU_SWITCH_EXECUTION_EVENT,
    souzu_switch_execution_payload,
)
from backend.autorun.util.suannkou_recommender import plan_pure_pinzu_suu_ankou_v2
from backend.autorun.util.wanxiang_switch_recommender import recommend_wanxiang_four_meld_switch
from backend.msgbox import _ui_confirm_blocking

ID_KAVI = 230
ID_MOON_PROTECTION = 227
ID_STAR_VACUUM = 207
BADGE_LIFE = 600100
BADGE_CONDUCTION = 600170
BADGE_EXPANSION = 600160
ID_UNSTABLE = 228
ID_THEFT = 229
ID_HACKER = 232
ID_HANABI_PLUS = 2221
_SWITCH_RECOMMENDATION_SEQ = 0
_SWITCH_STOP_EVENT = threading.Event()
_SWITCH_SEARCH_TASK: asyncio.Task | None = None
_LAST_SWITCH_PLAN: dict | None = None
_DISCARD_RECOMMENDATION_SEQ = 0


def _coerce_int_list(values: Any) -> list[int]:
    out: list[int] = []
    if not isinstance(values, (list, tuple)):
        return out
    for value in values:
        try:
            out.append(int(value))
        except Exception:
            continue
    return out


def _value_change_value(data: Any, key: str, default: Any = None) -> Any:
    if not isinstance(data, dict) or key not in data:
        return default
    item = data.get(key)
    if isinstance(item, dict) and "value" in item:
        return item.get("value", default)
    return item


def _parse_tile_score_map(raw_list: Any) -> Dict[str, str] | None:
    if raw_list is None:
        return None
    if not isinstance(raw_list, list):
        return {}
    return {
        str(item.get("tile", "")): str(item.get("score", ""))
        for item in raw_list
        if isinstance(item, dict)
        and item.get("tile") is not None
        and item.get("score") is not None
    }


def _parse_fan_value_map(raw_list: Any) -> Dict[str, str] | None:
    if raw_list is None:
        return None
    if not isinstance(raw_list, list):
        return {}
    return {
        str(item.get("id", "")): str(item.get("value", ""))
        for item in raw_list
        if isinstance(item, dict)
        and item.get("id") is not None
        and item.get("value") is not None
    }


def _redeal_wall_tile_ids(
        pool: list[dict],
        hands: list[int] | None = None,
        ming: list[dict] | None = None,
        dora_tiles: list[int] | None = None,
        limit: int = 9,
) -> list[int]:
    excluded_ids = set(hands or [])
    excluded_ids.update(dora_tiles or [])
    for item in ming or []:
        if not isinstance(item, dict):
            continue
        excluded_ids.update(item.get("tileList", []) or [])
    return [
        item["id"]
        for item in pool
        if isinstance(item, dict) and item.get("id") not in excluded_ids
    ][:limit]


def _redeal_wall_limit(desktop_remain: Any) -> int:
    try:
        remain = int(desktop_remain)
    except (TypeError, ValueError):
        return 9
    if remain <= 0:
        return 9
    return min(remain, 9)


def _handle_type10_draw_event(event: Dict[str, Any], reason: str) -> None:
    global _DISCARD_RECOMMENDATION_SEQ

    state = event.get("state", {})
    stage = state.get("current", -1)
    value_changes = event.get("valueChanges", {})
    round_info = value_changes.get("round", {})
    desktop_remain = round_info.get("desktopRemain", {}).get("value", None)
    effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
    ting_list = round_info.get("tingList", {}).get("value", None)
    next_operation = round_info.get("nextOperation", {}).get("value", None)
    character = value_changes.get("character", {}).get("value", None)
    after_draw_hands = round_info.get("hands", {}).get("value", None)
    if after_draw_hands:
        GAME_STATE.on_draw_tile(after_draw_hands, after_draw_hands[len(after_draw_hands) - 1], push_gamestate=False)

    GAME_STATE.update_other_info(
        desktop_remain=desktop_remain,
        stage=stage,
        effect_list=effect_list,
        ting_list=ting_list,
        character=character,
        next_operation=next_operation,
        reason=reason,
    )

    loop = asyncio.get_running_loop()
    _DISCARD_RECOMMENDATION_SEQ += 1
    rec_seq = _DISCARD_RECOMMENDATION_SEQ
    loop.create_task(
        _compute_and_broadcast_discard_recommendations(
            seq=rec_seq,
            deck_map=dict(GAME_STATE.deck_map),
            hand_tiles=list(GAME_STATE.hand_tiles),
            wall_tiles=list(GAME_STATE.wall_tiles),
        )
    )


def _normalize_switch_debug_snapshot(snapshot: Any, *, wall_limit: int = 36) -> dict:
    if not isinstance(snapshot, dict):
        raise ValueError("snapshot must be an object")

    raw_deck_map = snapshot.get("deck_map")
    if not isinstance(raw_deck_map, dict) or not raw_deck_map:
        raise ValueError("snapshot.deck_map is required")

    deck_map: dict[int, str] = {}
    for raw_key, raw_face in raw_deck_map.items():
        try:
            deck_map[int(raw_key)] = str(raw_face)
        except Exception:
            continue
    if not deck_map:
        raise ValueError("snapshot.deck_map has no valid tile ids")

    hand_tiles = _coerce_int_list(snapshot.get("hand_tiles"))
    replacement_tiles = _coerce_int_list(snapshot.get("replacement_tiles"))
    wall_tiles_all = _coerce_int_list(snapshot.get("wall_tiles"))
    switch_used_tiles = _coerce_int_list(snapshot.get("switch_used_tiles"))
    boss_buff = _coerce_int_list(snapshot.get("boss_buff"))

    referenced_ids = hand_tiles + replacement_tiles + wall_tiles_all + switch_used_tiles
    missing_ids = [tile_id for tile_id in referenced_ids if tile_id not in deck_map]
    if missing_ids:
        preview = ",".join(str(tile_id) for tile_id in missing_ids[:8])
        suffix = "" if len(missing_ids) <= 8 else "..."
        raise ValueError(f"snapshot.deck_map missing tile ids: {preview}{suffix}")

    wall_limit_int = max(2, min(36, int(wall_limit or 36)))
    wall_tiles = wall_tiles_all if wall_limit_int >= 36 else wall_tiles_all[:wall_limit_int]

    return {
        "stage": int(snapshot.get("stage", 2) or 0),
        "deck_map": deck_map,
        "hand_tiles": hand_tiles,
        "replacement_tiles": replacement_tiles,
        "wall_tiles_all": wall_tiles_all,
        "wall_tiles": wall_tiles,
        "switch_used_tiles": switch_used_tiles,
        "total_change_tile_count": int(snapshot.get("total_change_tile_count", 0) or 0),
        "change_tile_count": int(snapshot.get("change_tile_count", len(switch_used_tiles)) or 0),
        "boss_buff": boss_buff,
    }


def _live_switch_search_state(*, wall_limit: int = 36) -> dict:
    wall_limit_int = max(2, min(36, int(wall_limit or 36)))
    wall_tiles_all = list(getattr(GAME_STATE, "wall_tiles", None) or [])
    wall_tiles = wall_tiles_all if wall_limit_int >= 36 else wall_tiles_all[:wall_limit_int]
    return {
        "stage": int(getattr(GAME_STATE, "stage", 0) or 0),
        "deck_map": dict(getattr(GAME_STATE, "deck_map", None) or {}),
        "hand_tiles": list(getattr(GAME_STATE, "hand_tiles", None) or []),
        "replacement_tiles": list(getattr(GAME_STATE, "replacement_tiles", None) or []),
        "wall_tiles_all": wall_tiles_all,
        "wall_tiles": wall_tiles,
        "switch_used_tiles": list(getattr(GAME_STATE, "switch_used_tiles", None) or []),
        "total_change_tile_count": int(getattr(GAME_STATE, "total_change_tile_count", 0) or 0),
        "change_tile_count": int(getattr(GAME_STATE, "change_tile_count", 0) or 0),
        "boss_buff": list(getattr(GAME_STATE, "boss_buff", None) or []),
    }


def _cache_switch_plan(plan: dict | None) -> None:
    global _LAST_SWITCH_PLAN
    _LAST_SWITCH_PLAN = copy.deepcopy(plan) if isinstance(plan, dict) else None


def _current_discard_from_plan(plan: dict) -> int | None:
    if not isinstance(plan, dict):
        return None
    if plan.get("status") != "plan":
        return None
    d = plan.get("discards") or []
    return int(d[0]) if d else None


def _is_wanxiang_switch_plan(plan: dict | None) -> bool:
    if not isinstance(plan, dict):
        return False
    return (
            plan.get("search_algorithm") == "wanxiang_four_meld_switch"
            or plan.get("mode") == "wanxiang-four-meld-switch"
            or str(plan.get("plan_signature") or "").startswith("wanxiang|")
    )


def _wrap_discard_recommendation_entry(yaku_key: str, plan: dict) -> dict:
    entry = {
        "status": plan.get("status"),
        "draws_needed": plan.get("draws_needed"),
        "target14": plan.get("target14") or [],
        "discards": plan.get("discards") or [],
    }
    if "pair_hint" in plan:
        entry["pair_hint"] = plan["pair_hint"]
    if "mode" in plan:
        entry["mode"] = plan["mode"]
    if "reason" in plan:
        entry["reason"] = plan["reason"]
    cur = _current_discard_from_plan(plan)
    if cur is not None:
        entry["discard"] = cur
    return {"yaku": yaku_key, "data": entry}


async def _compute_and_broadcast_discard_recommendations(
        *,
        seq: int,
        deck_map: Dict[int, str],
        hand_tiles: List[int],
        wall_tiles: List[int],
) -> None:
    global _DISCARD_RECOMMENDATION_SEQ

    chiitoi, suuannkou = await asyncio.gather(
        asyncio.to_thread(chiitoi_recommendation_json, deck_map, hand_tiles, wall_tiles),
        asyncio.to_thread(plan_pure_pinzu_suu_ankou_v2, hand_tiles, wall_tiles, deck_map),
    )

    if seq != _DISCARD_RECOMMENDATION_SEQ:
        return

    await broadcast(chiitoi)

    payload = {
        "type": "discard_recommendation",
        "data": [
            _wrap_discard_recommendation_entry("chiitoi", chiitoi),
            _wrap_discard_recommendation_entry("suuannkou", suuannkou),
        ],
    }
    await broadcast(payload)
    # 已经移除了自动自摸
    # win_entries = [e for e in payload["data"] if e["data"].get("status") == "win_now"]
    # if not win_entries or not MANAGER.get("game.auto_tsumo"):
    #     return
    #
    # peer_key = None
    # addon_now = _addon.WS_ADDON_INSTANCE
    # if addon_now and addon_now.last_flow:
    #     f = addon_now.last_flow
    #     peer_key = f"{f.client_conn.address[0]}|{f.server_conn.address[0]}"
    #
    # def _do_inject():
    #     addon = _addon.WS_ADDON_INSTANCE
    #     if not addon:
    #         logger.warning("WS_ADDON_INSTANCE not ready; skip inject")
    #         return
    #     ok, reason, _ = addon.inject_now(
    #         method=".lq.Lobby.amuletActivityOperate",
    #         data={"activityId": 260511, "type": 8, "tileList": []},
    #         t="Req",
    #         peer_key=peer_key,
    #     )
    #     logger.info(f"success: {ok}, reason: {reason}")
    #
    # ctx.master.event_loop.call_later(0.3, _do_inject)


async def _wait_for_switch_state_advance(prev_change_count: int, timeout_sec: float = 5.0) -> bool:
    deadline = time.monotonic() + max(0.1, float(timeout_sec))
    while time.monotonic() < deadline:
        cur_stage = int(getattr(GAME_STATE, "stage", -1) or -1)
        cur_count = int(getattr(GAME_STATE, "change_tile_count", 0) or 0)
        if cur_stage not in [2, 4, 5] or cur_count > prev_change_count:
            return True
        await asyncio.sleep(0.1)
    return False


def _plan_batches(plan: dict) -> list[list[int]]:
    discards = list(plan.get("switch_discards") or [])
    batches = list(plan.get("switch_batch_sizes") or [])
    if not discards:
        return []
    if isinstance(discards[0], int):
        flat_discards = [int(x) for x in discards]
        if not batches:
            return [flat_discards] if flat_discards else []
        out: list[list[int]] = []
        cursor = 0
        for size in batches:
            size_int = max(0, int(size or 0))
            out.append(flat_discards[cursor:cursor + size_int])
            cursor += size_int
        return [batch for batch in out if batch]
    if not batches:
        return [list(batch or []) for batch in discards if isinstance(batch, list)]
    normalized_discards = [list(batch or []) for batch in discards if isinstance(batch, (list, tuple))]
    return [batch for batch in normalized_discards if batch]


RED_TILE_MAP = {"0m": "5m", "0p": "5p", "0s": "5s"}


def _norm_face(face: str) -> str:
    return RED_TILE_MAP.get(str(face), str(face))


def _full_plan_quad_faces(plan: dict) -> list[str]:
    faces = [_norm_face(face) for face in list(plan.get("quad_faces") or []) if face]
    if len(faces) >= 2:
        return faces[:2]
    target = [_norm_face(face) for face in list(plan.get("target14") or []) if face]
    counts: dict[str, int] = {}
    out: list[str] = []
    for face in target:
        counts[face] = counts.get(face, 0) + 1
    for face, count in counts.items():
        if count >= 4:
            out.append(face)
        if len(out) >= 2:
            break
    return out


def _find_quad_ids_in_hand(face: str, used_ids: set[int] | None = None) -> list[int]:
    used = used_ids or set()
    deck_map = getattr(GAME_STATE, "deck_map", None) or {}
    hand_tiles = list(getattr(GAME_STATE, "hand_tiles", None) or [])
    ids = [
        int(tile_id)
        for tile_id in hand_tiles
        if int(tile_id) not in used and _norm_face(deck_map.get(tile_id, "")) == _norm_face(face)
    ]
    return ids[:4] if len(ids) >= 4 else []


def _operation_tile_groups(op: dict) -> list[list[int]]:
    groups: list[list[int]] = []

    def add(raw: Any) -> None:
        ids = _coerce_int_list(raw)
        if len(ids) >= 4:
            groups.append(ids[:4])

    for key in ("tiles", "tileList", "tile_list"):
        add(op.get(key))

    gang = op.get("gang")
    if isinstance(gang, list):
        for item in gang:
            if isinstance(item, dict):
                for key in ("tiles", "tileList", "tile_list"):
                    add(item.get(key))
            else:
                add(item)

    for value in op.values():
        if isinstance(value, list):
            add(value)
    return groups


def _next_operation_has_type(op_type: int) -> bool:
    operations = list(getattr(GAME_STATE, "next_operation", None) or [])
    return any(int(op.get("type", -1) or -1) == op_type for op in operations if isinstance(op, dict))


def _available_kan_ids_for_face(face: str, used_ids: set[int] | None = None) -> list[int]:
    used = used_ids or set()
    deck_map = getattr(GAME_STATE, "deck_map", None) or {}
    operations = list(getattr(GAME_STATE, "next_operation", None) or [])
    for op in operations:
        if not isinstance(op, dict) or int(op.get("type", -1) or -1) != 4:
            continue
        for ids in _operation_tile_groups(op):
            if any(int(tile_id) in used for tile_id in ids):
                continue
            if all(_norm_face(deck_map.get(int(tile_id), "")) == _norm_face(face) for tile_id in ids):
                return [int(tile_id) for tile_id in ids[:4]]
    return []


async def _wait_for_available_kan_ids(
        face: str,
        used_ids: set[int],
        *,
        timeout_sec: float = 120.0,
        on_wait: Any = None,
) -> list[int]:
    deadline = time.monotonic() + max(0.1, float(timeout_sec))
    last_emit = 0.0
    while time.monotonic() < deadline:
        ids = _available_kan_ids_for_face(face, used_ids)
        if ids:
            return ids
        now = time.monotonic()
        if on_wait is not None and now - last_emit >= 1.0:
            last_emit = now
            await on_wait()
        await asyncio.sleep(0.2)
    return []


async def _wait_for_operation_type(
        op_type: int,
        *,
        timeout_sec: float = 120.0,
        on_wait: Any = None,
) -> bool:
    deadline = time.monotonic() + max(0.1, float(timeout_sec))
    last_emit = 0.0
    while time.monotonic() < deadline:
        if _next_operation_has_type(op_type):
            return True
        now = time.monotonic()
        if on_wait is not None and now - last_emit >= 1.0:
            last_emit = now
            await on_wait()
        await asyncio.sleep(0.2)
    return False


async def _wait_for_switch_stage_exit(timeout_sec: float = 8.0) -> bool:
    deadline = time.monotonic() + max(0.1, float(timeout_sec))
    while time.monotonic() < deadline:
        if int(getattr(GAME_STATE, "stage", 0) or 0) not in [2, 4, 5]:
            return True
        await asyncio.sleep(0.2)
    return False


def _pick_full_plan_discard_id(plan: dict) -> int | None:
    current_hand = [int(tile_id) for tile_id in list(getattr(GAME_STATE, "hand_tiles", None) or [])]
    if not current_hand:
        return None
    hand_set = set(current_hand)
    for tile_id in list(plan.get("post_draw_discards") or []):
        try:
            tid = int(tile_id)
        except (TypeError, ValueError):
            continue
        if tid in hand_set:
            return tid

    keep_counts: dict[str, int] = {}
    for face in list(plan.get("target13") or []):
        norm = _norm_face(face)
        keep_counts[norm] = keep_counts.get(norm, 0) + 1
    if not keep_counts:
        return None

    deck_map = getattr(GAME_STATE, "deck_map", None) or {}
    remaining_keep = dict(keep_counts)
    for tile_id in current_hand:
        face = _norm_face(deck_map.get(tile_id, ""))
        if remaining_keep.get(face, 0) > 0:
            remaining_keep[face] -= 1
            continue
        return tile_id
    return None


def _target_face_counts_for_full_plan(plan: dict) -> dict[str, int]:
    faces = list(plan.get("target14") or [])
    if not faces:
        faces = list(plan.get("target13") or [])
    counts: dict[str, int] = {}
    for face in faces:
        norm = _norm_face(face)
        counts[norm] = counts.get(norm, 0) + 1
    return counts


def _full_plan_target_draw_count(plan: dict) -> int:
    try:
        return int(plan.get("draws_needed", 0) or 0)
    except (TypeError, ValueError):
        return 0


def _pick_discard_against_face_counts(face_counts: dict[str, int]) -> int | None:
    current_hand = [int(tile_id) for tile_id in list(getattr(GAME_STATE, "hand_tiles", None) or [])]
    if not current_hand or not face_counts:
        return None

    deck_map = getattr(GAME_STATE, "deck_map", None) or {}
    remaining_keep = dict(face_counts)
    for tile_id in current_hand:
        face = _norm_face(deck_map.get(tile_id, ""))
        if remaining_keep.get(face, 0) > 0:
            remaining_keep[face] -= 1
            continue
        return tile_id
    return None


async def execute_current_switch_plan(*, finalize_event: bool = True, finish_switch: bool = False) -> tuple[bool, str]:
    plan = copy.deepcopy(_LAST_SWITCH_PLAN) if isinstance(_LAST_SWITCH_PLAN, dict) else None
    if not plan or plan.get("status") != "plan":
        return False, "当前没有可执行的黑洞换牌方案。"

    if _is_wanxiang_switch_plan(plan):
        finish_switch = False

    batches = _plan_batches(plan)
    if not batches:
        return False, "当前方案没有可执行的换牌步骤。"

    if int(getattr(GAME_STATE, "stage", 0) or 0) not in [2, 4, 5]:
        return False, "当前不在换牌阶段，无法执行黑洞换牌。"

    bot = getattr(backend.app, "PACKET_BOT", None)
    if not bot:
        return False, "发包模块未就绪，请先进入青云之志并保持连接。"

    batch_count = len(batches)

    async def emit_execution(
            status: str,
            *,
            batch_index: int = 0,
            reason: str = "",
            reason_key: str = "",
            reason_values: dict | None = None,
            ok: bool | None = None,
            phase: str = "",
            phase_key: str = "",
    ) -> None:
        await broadcast({
            "type": SOUZU_SWITCH_EXECUTION_EVENT,
            "data": souzu_switch_execution_payload(
                status,
                batch_count=batch_count,
                batch_index=batch_index,
                reason=reason,
                reason_key=reason_key,
                reason_values=reason_values,
                ok=ok,
                phase=phase,
                phase_key=phase_key,
            ),
        })

    async def heartbeat(batch_index: int, phase_key: str) -> None:
        while True:
            await asyncio.sleep(1.0)
            await emit_execution("running", batch_index=batch_index, phase_key=phase_key)

    await emit_execution("running")
    final_ok = True
    final_reason = ""
    final_reason_key = ""
    final_reason_values: dict | None = None
    final_batch_index = 0
    try:
        for index, discard_ids in enumerate(batches, start=1):
            final_batch_index = index
            await emit_execution("running", batch_index=index, phase_key="blackhole.execute_phase_prepare")
            current_hand = list(getattr(GAME_STATE, "hand_tiles", None) or [])
            if not current_hand:
                final_ok = False
                final_reason_key = "blackhole.execute_reason_empty_hand"
                break
            missing = [tid for tid in discard_ids if tid not in current_hand]
            if missing:
                final_ok = False
                final_reason_key = "blackhole.execute_reason_hand_changed"
                final_reason_values = {"index": index}
                break

            prefer_keep = [tid for tid in current_hand if tid not in discard_ids]
            buffs = set(getattr(GAME_STATE, "boss_buff", None) or [])
            if 901 in buffs:
                if len(discard_ids) > 3:
                    final_ok = False
                    final_reason_key = "blackhole.execute_reason_limit_exceeded"
                    final_reason_values = {"index": index, "count": len(discard_ids)}
                    break
                filtered_ids = prefer_keep
            else:
                filtered_ids = prefer_keep

            prev_change_count = int(getattr(GAME_STATE, "change_tile_count", 0) or 0)
            hb_task = asyncio.create_task(heartbeat(index, "blackhole.execute_phase_sending"))
            try:
                ok, reason, _resp = await call_with_1004_retry_async(
                    bot.op_change,
                    tile_ids=filtered_ids,
                    delay_sec=3,
                    interval=3,
                    timeout=3000,
                    to_thread=True,
                )
            finally:
                hb_task.cancel()
                await asyncio.gather(hb_task, return_exceptions=True)
            if not ok:
                final_ok = False
                final_reason_key = "blackhole.execute_reason_step_failed"
                final_reason_values = {"index": index, "reason": reason or "unknown"}
                break
            await emit_execution("running", batch_index=index, phase_key="blackhole.execute_phase_waiting")
            advanced = await _wait_for_switch_state_advance(prev_change_count)
            if not advanced:
                final_ok = False
                final_reason_key = "blackhole.execute_reason_state_stale"
                final_reason_values = {"index": index}
                break
        if final_ok and finish_switch:
            await emit_execution("running", batch_index=batch_count, phase_key="blackhole.execute_phase_finish_switch")
            can_finish_switch = await _wait_for_operation_type(
                100,
                timeout_sec=8.0,
                on_wait=lambda: emit_execution(
                    "running",
                    batch_index=batch_count,
                    phase_key="blackhole.execute_phase_finish_switch",
                ),
            )
            if not can_finish_switch:
                final_ok = False
                final_reason_key = "blackhole.execute_reason_finish_switch_unavailable"
            else:
                ok, reason, _resp = await call_with_1004_retry_async(
                    bot.op_skip_change,
                    delay_sec=3,
                    interval=3,
                    timeout=3000,
                    to_thread=True,
                )
                if not ok:
                    final_ok = False
                    final_reason_key = "blackhole.execute_reason_finish_switch_failed"
                    final_reason_values = {"reason": reason or "unknown"}
                else:
                    await _wait_for_switch_stage_exit()
    except Exception as exc:
        final_ok = False
        final_reason_key = "blackhole.execute_reason_unknown"
        final_reason_values = {"reason": str(exc) or "unknown"}
    if finalize_event:
        await emit_execution(
            "completed" if final_ok else "failed",
            batch_index=final_batch_index or batch_count,
            reason=final_reason,
            reason_key=final_reason_key,
            reason_values=final_reason_values,
            ok=final_ok,
        )
    return final_ok, final_reason


async def execute_current_full_plan() -> tuple[bool, str]:
    plan = copy.deepcopy(_LAST_SWITCH_PLAN) if isinstance(_LAST_SWITCH_PLAN, dict) else None
    if not plan or plan.get("status") != "plan":
        return False, "current plan unavailable"

    if _is_wanxiang_switch_plan(plan):
        return False, "wanxiang plan does not support finish switch"

    batches = _plan_batches(plan)
    batch_count = len(batches)

    async def emit_execution(
            status: str,
            *,
            batch_index: int = 0,
            reason: str = "",
            reason_key: str = "",
            reason_values: dict | None = None,
            ok: bool | None = None,
            phase_key: str = "",
    ) -> None:
        await broadcast({
            "type": SOUZU_SWITCH_EXECUTION_EVENT,
            "data": souzu_switch_execution_payload(
                status,
                batch_count=batch_count,
                batch_index=batch_index,
                reason=reason,
                reason_key=reason_key,
                reason_values=reason_values,
                ok=ok,
                phase_key=phase_key,
                execution_kind="full_plan",
            ),
        })

    bot = getattr(backend.app, "PACKET_BOT", None)
    if not bot:
        await emit_execution("failed", reason_key="blackhole.execute_reason_bot_not_ready", ok=False)
        return False, "bot not ready"

    ok, reason = await execute_current_switch_plan(finalize_event=False, finish_switch=True)
    if not ok:
        await emit_execution(
            "failed",
            batch_index=batch_count,
            reason_key="blackhole.execute_reason_switch_failed",
            reason_values={"reason": reason or "unknown"},
            ok=False,
        )
        return False, reason

    quad_faces = _full_plan_quad_faces(plan)
    if len(quad_faces) < 2:
        await emit_execution("failed", batch_index=batch_count, reason_key="blackhole.execute_reason_no_quad_plan", ok=False)
        return False, "no quad plan"

    keep_counts = _target_face_counts_for_full_plan(plan)
    used_quad_ids: set[int] = set()
    completed_quad_indexes: set[int] = set()
    target_draw_count = _full_plan_target_draw_count(plan)
    advanced_draw_count = 1 if target_draw_count > 0 else 0
    deadline = time.monotonic() + 180.0
    last_wait_emit = 0.0

    def target_reached() -> bool:
        return target_draw_count > 0 and advanced_draw_count >= target_draw_count

    while len(completed_quad_indexes) < len(quad_faces[:2]):
        if time.monotonic() >= deadline:
            await emit_execution(
                "failed",
                batch_index=batch_count,
                reason_key="blackhole.execute_reason_full_timeout",
                ok=False,
            )
            return False, "full plan timeout"

        kan_done = False
        for face_index, face in enumerate(quad_faces[:2], start=1):
            if face_index in completed_quad_indexes:
                continue
            quad_ids = _available_kan_ids_for_face(face, used_quad_ids)
            if len(quad_ids) != 4:
                continue
            used_quad_ids.update(quad_ids)
            await emit_execution(
                "running",
                batch_index=batch_count,
                phase_key="blackhole.execute_phase_kan",
                reason_values={"index": face_index},
            )
            ok, reason, _resp = await call_with_1004_retry_async(
                bot.op_kan,
                tile_ids=quad_ids,
                delay_sec=3,
                interval=3,
                timeout=3000,
                to_thread=True,
            )
            if not ok:
                await emit_execution(
                    "failed",
                    batch_index=batch_count,
                    reason_key="blackhole.execute_reason_kan_failed",
                    reason_values={"index": face_index, "face": face, "reason": reason or "unknown"},
                    ok=False,
                )
                return False, reason or f"kan failed: {face}"
            completed_quad_indexes.add(face_index)
            advanced_draw_count += 1
            kan_done = True
            await asyncio.sleep(0.4)
            break
        if kan_done:
            continue

        if _next_operation_has_type(1):
            discard_id = _pick_discard_against_face_counts(keep_counts)
            if discard_id is None:
                await emit_execution(
                    "failed",
                    batch_index=batch_count,
                    reason_key="blackhole.execute_reason_collect_discard_unavailable",
                    ok=False,
                )
                return False, "collect discard unavailable"
            await emit_execution(
                "running",
                batch_index=batch_count,
                phase_key="blackhole.execute_phase_auto_discard",
                reason_values={"tile_id": discard_id},
            )
            ok, reason, _resp = await call_with_1004_retry_async(
                bot.discard_by_tile_id,
                tile_id=discard_id,
                delay_sec=3,
                interval=3,
                timeout=3000,
                to_thread=True,
            )
            if not ok:
                await emit_execution(
                    "failed",
                    batch_index=batch_count,
                    reason_key="blackhole.execute_reason_auto_discard_failed",
                    reason_values={"tile_id": discard_id, "reason": reason or "unknown"},
                    ok=False,
                )
                return False, reason or "auto discard failed"
            advanced_draw_count += 1
            await asyncio.sleep(0.4)
            continue

        now = time.monotonic()
        if now - last_wait_emit >= 1.0:
            last_wait_emit = now
            await emit_execution(
                "running",
                batch_index=batch_count,
                phase_key="blackhole.execute_phase_wait_operation",
            )
        await asyncio.sleep(0.2)

    if target_draw_count <= 0:
        await emit_execution("completed", batch_index=batch_count, reason_key="blackhole.execute_reason_full_completed", ok=True)
        return True, ""

    while not target_reached():
        if time.monotonic() >= deadline:
            await emit_execution(
                "failed",
                batch_index=batch_count,
                reason_key="blackhole.execute_reason_discard_not_available",
                ok=False,
            )
            return False, "discard not available"
        if _next_operation_has_type(1):
            discard_id = _pick_full_plan_discard_id(plan) or _pick_discard_against_face_counts(keep_counts)
            if discard_id is None:
                await emit_execution("failed", batch_index=batch_count, reason_key="blackhole.execute_reason_no_discard", ok=False)
                return False, "no discard"
            await emit_execution(
                "running",
                batch_index=batch_count,
                phase_key="blackhole.execute_phase_auto_discard",
                reason_values={"tile_id": discard_id},
            )
            logger.info(f"discard {discard_id} {GAME_STATE.deck_map.get(discard_id)}")
            ok, reason, _resp = await call_with_1004_retry_async(
                bot.discard_by_tile_id,
                tile_id=discard_id,
                delay_sec=3,
                interval=3,
                timeout=3000,
                to_thread=True,
            )
            if not ok:
                await emit_execution(
                    "failed",
                    batch_index=batch_count,
                    reason_key="blackhole.execute_reason_discard_failed",
                    reason_values={"tile_id": discard_id, "reason": reason or "unknown"},
                    ok=False,
                )
                return False, reason or "discard failed"
            advanced_draw_count += 1
            await asyncio.sleep(0.4)
            continue
        await emit_execution("running", batch_index=batch_count, phase_key="blackhole.execute_phase_wait_operation")
        await asyncio.sleep(0.4)

    await emit_execution("completed", batch_index=batch_count, reason_key="blackhole.execute_reason_full_completed", ok=True)
    return True, ""


def _current_discard(plan: dict) -> int | None:
    if not isinstance(plan, dict):
        return None
    if plan.get("status") != "plan":
        return None
    d = plan.get("discards") or []
    return int(d[0]) if d else None


def _switch_search_params_from_state(state: dict) -> dict:
    hand_tiles = list(state.get("hand_tiles") or [])
    replacement_tiles = list(state.get("replacement_tiles") or [])
    used_count = len(state.get("switch_used_tiles") or [])
    remaining_changes = max(0, int(state.get("total_change_tile_count", 0) or 0) - int(state.get("change_tile_count", 0) or 0))
    per_change_limit = 3 if 901 in (state.get("boss_buff") or []) else 13
    replacement_read_limit = min(max(0, len(replacement_tiles) - used_count), remaining_changes * per_change_limit)
    considered_tiles = list(state.get("wall_tiles") or [])
    return {
        "max_change_count": int(state.get("total_change_tile_count", 0) or 0),
        "per_change_limit": per_change_limit,
        "considered_tile_count": len(hand_tiles) + replacement_read_limit + len(considered_tiles),
    }


def _souzu_switch_disabled_reason(state: dict) -> str | None:
    return None


def _has_wanxiang(state: dict) -> bool:
    deck_map = state.get("deck_map") or {}
    hand_tiles = {int(tile_id) for tile_id in list(state.get("hand_tiles") or [])}
    return 1000 in hand_tiles or any(str(face) == "bd" for face in deck_map.values())


def _souzu_search_algorithm_label(value: str) -> str:
    if value == "wanxiang_four_meld_switch":
        return "万象四面子换牌搜索"
    return "目标牌型枚举搜索"


def _wrap_entry(yaku_key: str, plan: dict) -> dict:
    entry = {
        "status": plan.get("status"),
        "draws_needed": plan.get("draws_needed"),
        "target14": plan.get("target14") or [],
        "discards": plan.get("discards") or [],
    }
    for key in (
            "pair_hint",
            "mode",
            "reason",
            "progress",
            "search_algorithm",
            "search_algorithm_label",
            "switch_discards",
            "switch_in",
            "wall_draws",
            "post_draw_discards",
            "waits",
            "quad_faces",
            "switch_batch_sizes",
            "remaining_changes",
            "target13",
            "plan_signature",
            "quad_catalog",
            "max_change_count",
            "per_change_limit",
            "considered_tile_count",
            "request_source",
            "component_descs",
            "manual_searchable",
            "manual_search_reason",
            "debug_pool",
    ):
        if key in plan:
            entry[key] = plan[key]
    cur = _current_discard(plan)
    if cur is not None:
        entry["discard"] = cur
    return {"yaku": yaku_key, "data": entry}


async def _broadcast_switch_recommendation(
        *,
        skip_signatures: list[str] | None = None,
        wall_limit: int = 36,
        search_algorithm: str = "target_enumeration_search",
        snapshot_state: dict | None = None,
        request_source: str = "live",
) -> None:
    global _SWITCH_RECOMMENDATION_SEQ, _SWITCH_STOP_EVENT, _SWITCH_SEARCH_TASK
    _SWITCH_RECOMMENDATION_SEQ += 1
    seq = _SWITCH_RECOMMENDATION_SEQ
    _SWITCH_STOP_EVENT = threading.Event()
    loop = asyncio.get_running_loop()
    send_state = {
        "scheduled": False,
        "sending": False,
        "sent_text": None,
        "sent_candidate_version": -1,
        "flush_queued": False,
        "finished": False,
    }
    state = snapshot_state or _live_switch_search_state(wall_limit=wall_limit)
    search_params = _switch_search_params_from_state(state)
    has_wanxiang = _has_wanxiang(state)
    search_algorithm = "wanxiang_four_meld_switch" if has_wanxiang else "target_enumeration_search"
    search_params["search_algorithm"] = search_algorithm
    search_params["search_algorithm_label"] = _souzu_search_algorithm_label(search_algorithm)
    await broadcast({
        "type": "discard_recommendation",
        "data": [_wrap_entry("souzu_switch", {
            "status": "searching",
            "progress": "正在准备搜索…",
            "request_source": request_source,
            **search_params,
        })],
    })

    latest_progress = {"text": "正在准备搜索…", "candidate": None, "candidate_version": 0, "telemetry": {}}

    latest_progress["candidate"] = dict(search_params)

    async def _flush_progress(force: bool = False) -> None:
        send_state["flush_queued"] = False
        if send_state["finished"]:
            send_state["scheduled"] = False
            send_state["sending"] = False
            return
        if seq != _SWITCH_RECOMMENDATION_SEQ:
            send_state["scheduled"] = False
            send_state["sending"] = False
            return
        if send_state["sending"]:
            return
        send_state["sending"] = True
        try:
            while True:
                message = latest_progress["text"]
                candidate_version = latest_progress["candidate_version"]
                if not force and message == send_state["sent_text"] and candidate_version == send_state["sent_candidate_version"]:
                    break
                await broadcast({
                    "type": "discard_recommendation",
                    "data": [_wrap_entry("souzu_switch", {
                        "status": "searching",
                        "progress": message,
                        "request_source": request_source,
                        **(latest_progress["telemetry"] or {}),
                        **(latest_progress["candidate"] or {}),
                    })],
                })
                send_state["sent_text"] = message
                send_state["sent_candidate_version"] = candidate_version
                if (
                        latest_progress["text"] == message
                        and latest_progress["candidate_version"] == candidate_version
                ):
                    break
        finally:
            send_state["sending"] = False
            send_state["scheduled"] = False

    def _request_flush(force: bool = False) -> None:
        if send_state["finished"]:
            return
        if seq != _SWITCH_RECOMMENDATION_SEQ:
            return
        if send_state["flush_queued"] and not force:
            return
        send_state["flush_queued"] = True

        def _spawn() -> None:
            if send_state["finished"]:
                send_state["flush_queued"] = False
                return
            if seq != _SWITCH_RECOMMENDATION_SEQ:
                send_state["flush_queued"] = False
                return
            asyncio.create_task(_flush_progress(force=force))

        loop.call_soon_threadsafe(_spawn)

    def progress_cb(message: str) -> None:
        if send_state["finished"]:
            return
        if seq != _SWITCH_RECOMMENDATION_SEQ:
            return
        latest_progress["text"] = message
        _request_flush()

    def candidate_cb(plan: dict) -> None:
        if send_state["finished"]:
            return
        if seq != _SWITCH_RECOMMENDATION_SEQ:
            return
        latest_progress["candidate"] = {
            "draws_needed": plan.get("draws_needed"),
            "mode": plan.get("mode"),
            "switch_discards": plan.get("switch_discards") or [],
            "switch_in": plan.get("switch_in") or [],
            "wall_draws": plan.get("wall_draws") or [],
            "post_draw_discards": plan.get("post_draw_discards") or [],
            "waits": plan.get("waits") or [],
            "quad_faces": plan.get("quad_faces") or [],
            "switch_batch_sizes": plan.get("switch_batch_sizes") or [],
            "remaining_changes": plan.get("remaining_changes"),
            "target13": plan.get("target13") or [],
            "plan_signature": plan.get("plan_signature"),
            "request_source": request_source,
            **search_params,
        }
        latest_progress["candidate_version"] += 1
        _request_flush(force=True)

    def telemetry_cb(payload: dict) -> None:
        if send_state["finished"]:
            return
        if seq != _SWITCH_RECOMMENDATION_SEQ:
            return
        latest_progress["telemetry"] = dict(payload or {})
        latest_progress["candidate_version"] += 1
        _request_flush(force=True)

    async def _progress_pump() -> None:
        try:
            while seq == _SWITCH_RECOMMENDATION_SEQ and not send_state["finished"]:
                if not send_state["sending"]:
                    await _flush_progress(force=True)
                await asyncio.sleep(0.1)
        finally:
            send_state["scheduled"] = False

    send_state["scheduled"] = True
    asyncio.create_task(_progress_pump())

    recommender = recommend_wanxiang_four_meld_switch if has_wanxiang else recommend_souzu_tenpai_switch
    search_task = asyncio.create_task(asyncio.to_thread(
        recommender,
        state["deck_map"],
        state["hand_tiles"],
        state["replacement_tiles"],
        state["wall_tiles"],
        state["switch_used_tiles"],
        state["total_change_tile_count"],
        state["change_tile_count"],
        state["boss_buff"],
        progress_cb,
        candidate_cb,
        telemetry_cb,
        _SWITCH_STOP_EVENT.is_set,
        set(skip_signatures or []),
        True,
        search_algorithm,
    ))
    _SWITCH_SEARCH_TASK = search_task
    plan = await search_task
    if seq != _SWITCH_RECOMMENDATION_SEQ:
        return
    if isinstance(plan, dict):
        plan = {**search_params, **plan, "request_source": request_source}
    _cache_switch_plan(plan)
    send_state["finished"] = True
    send_state["flush_queued"] = False
    await broadcast({
        "type": "discard_recommendation",
        "data": [_wrap_entry("souzu_switch", plan)],
    })


async def start_switch_recommendation_search(
        *,
        skip_signatures: list[str] | None = None,
        wall_limit: int = 36,
        search_algorithm: str = "target_enumeration_search",
) -> None:
    _SWITCH_STOP_EVENT.set()
    state = _live_switch_search_state(wall_limit=wall_limit)
    disabled_reason = _souzu_switch_disabled_reason(state)
    if disabled_reason is not None:
        plan = {
            "status": "impossible",
            "reason": disabled_reason,
            "request_source": "live",
            **_switch_search_params_from_state(state),
        }
        _cache_switch_plan(plan)
        await broadcast({
            "type": "discard_recommendation",
            "data": [_wrap_entry("souzu_switch", plan)],
        })
        return
    if int(state.get("stage", 0) or 0) not in [4, 5]:
        _cache_switch_plan({
            "status": "impossible",
            "reason": "stage-not-switch",
            "request_source": "live",
            **_switch_search_params_from_state(state),
        })
        await broadcast({
            "type": "discard_recommendation",
            "data": [_wrap_entry("souzu_switch", {
                "status": "impossible",
                "reason": "stage-not-switch",
                "request_source": "live",
                **_switch_search_params_from_state(state),
            })],
        })
        return
    asyncio.create_task(_broadcast_switch_recommendation(
        skip_signatures=skip_signatures or [],
        wall_limit=wall_limit,
        search_algorithm=search_algorithm,
        snapshot_state=state,
        request_source="live",
    ))


async def start_switch_recommendation_debug_search(
        *,
        snapshot: dict,
        skip_signatures: list[str] | None = None,
        wall_limit: int = 36,
        search_algorithm: str = "target_enumeration_search",
) -> None:
    _SWITCH_STOP_EVENT.set()
    try:
        state = _normalize_switch_debug_snapshot(snapshot, wall_limit=wall_limit)
    except ValueError as exc:
        plan = {
            "status": "impossible",
            "reason": str(exc),
            "request_source": "debug",
            "max_change_count": 0,
            "per_change_limit": 13,
            "considered_tile_count": 0,
        }
        _cache_switch_plan(plan)
        await broadcast({
            "type": "discard_recommendation",
            "data": [_wrap_entry("souzu_switch", plan)],
        })
        return

    disabled_reason = _souzu_switch_disabled_reason(state)
    if disabled_reason is not None:
        plan = {
            "status": "impossible",
            "reason": disabled_reason,
            "request_source": "debug",
            **_switch_search_params_from_state(state),
        }
        _cache_switch_plan(plan)
        await broadcast({
            "type": "discard_recommendation",
            "data": [_wrap_entry("souzu_switch", plan)],
        })
        return

    if int(state.get("stage", 0) or 0) not in [2, 4, 5]:
        plan = {
            "status": "impossible",
            "reason": "stage-not-switch",
            "request_source": "debug",
            **_switch_search_params_from_state(state),
        }
        _cache_switch_plan(plan)
        await broadcast({
            "type": "discard_recommendation",
            "data": [_wrap_entry("souzu_switch", plan)],
        })
        return

    asyncio.create_task(_broadcast_switch_recommendation(
        skip_signatures=skip_signatures or [],
        wall_limit=wall_limit,
        search_algorithm=search_algorithm,
        snapshot_state=state,
        request_source="debug",
    ))


async def validate_manual_switch_debug_plan(
        *,
        snapshot: dict,
        quad_groups: Sequence[Sequence[int]],
        structure_groups: Dict[str, Sequence[int]],
        wall_limit: int = 36,
) -> None:
    try:
        state = _normalize_switch_debug_snapshot(snapshot, wall_limit=wall_limit)
    except ValueError as exc:
        plan = {
            "status": "impossible",
            "reason": str(exc),
            "request_source": "debug",
            "max_change_count": 0,
            "per_change_limit": 13,
            "considered_tile_count": 0,
        }
        _cache_switch_plan(plan)
        await broadcast({"type": "discard_recommendation", "data": [_wrap_entry("souzu_switch", plan)]})
        return

    disabled_reason = _souzu_switch_disabled_reason(state)
    if disabled_reason is not None:
        plan = {
            "status": "impossible",
            "reason": disabled_reason,
            "request_source": "debug",
            **_switch_search_params_from_state(state),
        }
        _cache_switch_plan(plan)
        await broadcast({"type": "discard_recommendation", "data": [_wrap_entry("souzu_switch", plan)]})
        return

    plan = await asyncio.to_thread(
        validate_manual_souzu_switch_plan,
        state["deck_map"],
        state["hand_tiles"],
        state["replacement_tiles"],
        state["wall_tiles"],
        state["switch_used_tiles"],
        state["total_change_tile_count"],
        state["change_tile_count"],
        state["boss_buff"],
        quad_groups,
        structure_groups,
    )
    if isinstance(plan, dict):
        plan = {
            **_switch_search_params_from_state(state),
            **plan,
            "request_source": "debug",
        }
    _cache_switch_plan(plan)
    await broadcast({
        "type": "discard_recommendation",
        "data": [_wrap_entry("souzu_switch", plan)],
    })


async def broadcast_switch_quad_catalog(*, wall_limit: int = 36) -> None:
    state = _live_switch_search_state(wall_limit=wall_limit)
    disabled_reason = _souzu_switch_disabled_reason(state)
    if disabled_reason is not None:
        plan = {
            "status": "impossible",
            "reason": disabled_reason,
            "request_source": "live",
            **_switch_search_params_from_state(state),
        }
        _cache_switch_plan(plan)
        await broadcast({
            "type": "discard_recommendation",
            "data": [_wrap_entry("souzu_switch", plan)],
        })
        return
    plan = await asyncio.to_thread(
        list_reachable_quads_for_switch,
        state["deck_map"],
        state["hand_tiles"],
        state["replacement_tiles"],
        state["wall_tiles"],
        state["switch_used_tiles"],
        state["total_change_tile_count"],
        state["change_tile_count"],
        state["boss_buff"],
    )
    if isinstance(plan, dict):
        plan = {**_switch_search_params_from_state(state), **plan, "request_source": "live"}
    _cache_switch_plan(plan)
    await broadcast({
        "type": "discard_recommendation",
        "data": [_wrap_entry("souzu_switch", plan)],
    })


async def stop_switch_recommendation_search(*, notify_client: bool = True) -> None:
    global _SWITCH_RECOMMENDATION_SEQ, _SWITCH_SEARCH_TASK
    _SWITCH_RECOMMENDATION_SEQ += 1
    _SWITCH_STOP_EVENT.set()
    _SWITCH_SEARCH_TASK = None
    if notify_client:
        _cache_switch_plan({
            "status": "impossible",
            "reason": "stopped-by-user",
            "request_source": "live",
            **_switch_search_params_from_state(_live_switch_search_state()),
        })
        await broadcast({
            "type": "discard_recommendation",
            "data": [_wrap_entry("souzu_switch", {
                "status": "impossible",
                "reason": "stopped-by-user",
                "request_source": "live",
                **_switch_search_params_from_state(_live_switch_search_state()),
            })],
        })


def _first_src_base(row: dict) -> int:
    try:
        s = row.get("store") or []
        return _base(s[0]) if s else 0
    except:
        return 0


def _base(x: Any) -> int:
    try:
        return int(x) // 10
    except:
        return 0


def _plus(x: Any) -> bool:
    try:
        return abs(int(x)) % 10 == 1
    except:
        return False


def _bid(row: Optional[dict]) -> int:
    if not isinstance(row, dict): return 0
    b = row.get("badge")
    if isinstance(b, dict) and "id" in b:
        try:
            return int(b["id"])
        except:
            return 0
    try:
        return int(row.get("badgeId", 0))
    except:
        return 0


def _effects() -> List[dict]:
    return list(getattr(GAME_STATE, "effect_list", []) or [])


def _name(row: Optional[dict]) -> str:
    if not isinstance(row, dict): return "—"
    a = AMULET_REG.get(_base(row.get("id"))) if AMULET_REG else None
    return a.name if a else f"#{_base(row.get('id'))}"


def _badge_label(row: Optional[dict]) -> str:
    bid = _bid(row)
    if bid <= 0: return "无"
    b = BADGE_REG.get(bid) if BADGE_REG else None
    return f"{bid}（{b.name}）" if b else f"{bid}"


def _neighbors_of_kavi() -> dict:
    ef = _effects()
    n = len(ef)
    for i, row in enumerate(ef):
        rid = row.get("id")
        if _base(rid) == ID_KAVI:
            return {
                "left": ef[i - 1] if i - 1 >= 0 else None,
                "right": ef[i + 1] if i + 1 < n else None,
                "kavi_raw_id": int(rid) if rid is not None else 0,
                "kavi_index": i,
            }
    return {"left": None, "right": None, "kavi_raw_id": 0, "kavi_index": -1}


def _collect_candidate_sets() -> tuple[Set[int], Set[int], List[dict]]:
    lst: List[dict] = list(getattr(GAME_STATE, "candidate_effect_list", []) or [])
    a_set, b_set = set(), set()
    for r in lst:
        try:
            aid = _base(r.get("id", 0))
            bid = int(r.get("badgeId", 0))
        except:
            continue
        if aid > 0: a_set.add(aid)
        if bid > 0: b_set.add(bid)
    return a_set, b_set, lst


def _fuse_hits_values() -> tuple[bool, dict]:
    cfg = MANAGER.to_table_payload("fuse") or {}
    guard = (cfg.get("guard_skip_contains") or {}) if isinstance(cfg.get("guard_skip_contains"), dict) else {}

    watch_a = set(map(int, guard.get("amulets", []) or []))
    watch_b = set(map(int, guard.get("badges", []) or []))

    cand_a, cand_b, _ = _collect_candidate_sets()

    hit_a = cand_a & watch_a
    hit_b = cand_b & watch_b

    has_hit = bool(hit_a or hit_b)

    values = {
        "amuletHitCount": len(hit_a),
        "badgeHitCount": len(hit_b)
    }
    return has_hit, values


def _must_pick_guard(selected_raw_id: int) -> tuple[bool, bool, dict]:
    cfg = MANAGER.to_table_payload("fuse") or {}
    guard = (cfg.get("guard_skip_contains") or {}) if isinstance(cfg.get("guard_skip_contains"), dict) else {}
    watch_a = set(map(int, guard.get("amulets", []) or []))
    watch_b = set(map(int, guard.get("badges", []) or []))

    cand_a, cand_b, cand_list = _collect_candidate_sets()

    # 候选中是否有命中项
    hit_a_all = cand_a & watch_a
    hit_b_all = cand_b & watch_b
    hit_exist = bool(hit_a_all or hit_b_all)

    # 当前选择的是否为命中项
    picked = None
    sel_raw = int(selected_raw_id)
    for row in cand_list:
        try:
            if int(row.get("id", 0)) == sel_raw:
                picked = row
                break
        except Exception:
            pass

    picked_is_hit = False
    if picked:
        base = _base(picked.get("id", 0))
        bid = int(picked.get("badgeId", 0))
        if base in watch_a or (bid > 0 and bid in watch_b):
            picked_is_hit = True

    values = {
        "selBaseId": _base(sel_raw),
        "selRawId": sel_raw
    }

    return hit_exist, picked_is_hit, values


def _confirm_fuse_or_drop(cfg: dict, *, title_key: str, message_key: str, values: dict) -> bool:
    return _ui_confirm_blocking(
        title_key=title_key,
        message_key=message_key,
        values=values,
        ok_key="common.continue",
        cancel_key="common.cancel",
        timeout=45.0,
    )

# MARK: on_outbound
def on_outbound(view: Dict) -> Tuple[str, Any]:
    if backend.app.AUTORUNNER.running:
        return "pass", None
    try:
        if view.get("type") == "Req" and view.get("method") == ".lq.Lobby.amuletActivityOperate":
            data = view.get("data")
            type = data.get("type")
            args = data.get("args")
            cfg = MANAGER.to_table_payload("fuse") or {}
            # 换牌守护：换牌阶段未听牌时，拦截跳过操作。
            if (
                    bool(cfg.get("enable_ting_ready_skip_guard", True))
                    and type == 3
                    and GAME_STATE.stage in [4, 5]
                    and len(GAME_STATE.ting_list or []) == 0
            ):
                ok = _confirm_fuse_or_drop(
                    cfg,
                    title_key="fuse.guard.tingReadySkip.title",
                    message_key="fuse.guard.tingReadySkip.message",
                    values={},
                )
                return ("pass", None) if ok else ("drop", None)
            if type == 3 and GAME_STATE.stage in [2, 9, 16]:
                if bool(cfg.get("enable_skip_guard", True)):
                    has_hit, values = _fuse_hits_values()
                    if has_hit:
                        ok = _confirm_fuse_or_drop(
                            cfg,
                            title_key="fuse.guard.skipPack.title",
                            message_key="fuse.guard.skipPack.message",
                            values=values,
                        )
                        return ("pass", None) if ok else ("drop", None)
                return "pass", None
            if type == 16:
                raw_id = GAME_STATE.candidate_effect_list[int(args[0])].get("id")
                if bool(cfg.get("enable_shop_force_pick", False)):
                    hit_exist, picked_is_hit, values = _must_pick_guard(raw_id)
                    if hit_exist and not picked_is_hit:
                        ok = _confirm_fuse_or_drop(
                            cfg,
                            title_key="fuse.guard.forcePick.title",
                            message_key="fuse.guard.forcePick.message",
                            values=values,
                        )
                        return ("pass", None) if ok else ("drop", None)

                return "pass", None

        if view.get("type") == "Req" and view.get("method") == ".lq.Lobby.amuletActivityUpgrade":
            cfg = MANAGER.to_table_payload("fuse") or {}
            ef = _effects()
            if bool(cfg.get("enable_prestart_kavi_guard", True)):
                has_kavi = any(_base(e.get("id")) == ID_KAVI and _bid(e) == BADGE_CONDUCTION for e in ef)
                if has_kavi:
                    min_cnt = int(cfg.get("conduction_min_count", 3))
                    cnt = sum(1 for e in ef if _bid(e) == BADGE_CONDUCTION)
                    if cnt >= min_cnt:
                        nb = _neighbors_of_kavi()
                        if nb["kavi_index"] >= 0:
                            left, right = nb["left"], nb["right"]
                            no_badge_left = (left is not None and _bid(left) == 0)
                            no_badge_right = (right is not None and _bid(right) == 0)
                            if not (no_badge_left or no_badge_right):
                                values = {
                                    "kaviTypeText": ("P" if _plus(nb["kavi_raw_id"]) else "NP"),
                                    "minCnt": min_cnt,
                                    "cnt": cnt,
                                    "leftName": _name(left) or "—",
                                    "leftBadgeLabel": _badge_label(left) or "—",
                                    "rightName": _name(right) or "—",
                                    "rightBadgeLabel": _badge_label(right) or "—",
                                }
                                ok = _confirm_fuse_or_drop(
                                    cfg,
                                    title_key="fuse.guard.kaviPrestartConduction.title",
                                    message_key="fuse.guard.kaviPrestartConduction.message",
                                    values=values,
                                )
                                return ("pass", None) if ok else ("drop", None)
            if bool(cfg.get("enable_kavi_plus_buffer_guard", True)):
                # 找到卡维 Plus
                try:
                    k_idx = next((i for i, e in enumerate(ef) if _base(e.get("id")) == ID_KAVI and _plus(e.get("id"))), -1)
                except Exception:
                    k_idx = -1

                if k_idx >= 0:
                    # 只有场上真的存在“膨胀”时才检查
                    if any(_bid(e) == BADGE_EXPANSION for e in ef):
                        n = len(ef)

                        def first_seen(step: int) -> tuple[str, dict | None]:
                            j = k_idx + step
                            while 0 <= j < n:
                                row = ef[j]
                                # 命中膨胀
                                if _bid(row) == BADGE_EXPANSION:
                                    return "hit", row
                                # 第一枚非膨胀就返回
                                return "ok", row
                            return "none", None

                        l_state, l_row = first_seen(-1)
                        r_state, r_row = first_seen(1)

                        # 只要有一侧紧邻即膨胀，就提示
                        if l_state == "hit" or r_state == "hit":
                            state_text = {
                                "hit": "E",
                                "ok": "NE",
                                "none": "N",
                            }
                            values2 = {
                                "expBadgeId": BADGE_EXPANSION,
                                "leftStateText": state_text.get(l_state, l_state),
                                "rightStateText": state_text.get(r_state, r_state),
                                "leftName": _name(l_row) or "—",
                                "leftBadgeLabel": _badge_label(l_row) or "—",
                                "rightName": _name(r_row) or "—",
                                "rightBadgeLabel": _badge_label(r_row) or "—",
                            }
                            ok2 = _confirm_fuse_or_drop(
                                cfg,
                                title_key="fuse.guard.kaviPrestartExpansion.title",
                                message_key="fuse.guard.kaviPrestartExpansion.message",
                                values=values2,
                            )
                            return ("pass", None) if ok2 else ("drop", None)

            return "pass", None
        # 黑客、不稳定存的第一个数据为复制或变身的护身符：{"id":2320,"store":[2290,1234]} 229为盗印，不稳定228、黑客232、卡维230
        if (
                view.get("type") == "Req"
                and view.get("method") == ".lq.Lobby.amuletActivityOperate"
                and (view.get("data") or {}).get("type") == 8
        ):
            cfg = MANAGER.to_table_payload("fuse") or {}
            if bool(cfg.get("enable_hanabi_win_guard", True)):
                ef = _effects()
                has_hanabi_plus = False
                for row in ef:
                    if not isinstance(row, dict):
                        continue
                    try:
                        if int(row.get("id", 0)) == ID_HANABI_PLUS:
                            has_hanabi_plus = True
                            break
                    except Exception:
                        continue
                ming = getattr(GAME_STATE, "ming", None) or []
                ming_count = len(ming) if isinstance(ming, list) else 0
                if has_hanabi_plus and ming_count < 2:
                    ok = _confirm_fuse_or_drop(
                        cfg,
                        title_key="fuse.guard.hanabiWin.title",
                        message_key="fuse.guard.hanabiWin.message",
                        values={
                            "hanabiId": ID_HANABI_PLUS,
                            "mingCount": ming_count,
                        },
                    )
                    return ("pass", None) if ok else ("drop", None)

            if not bool(cfg.get("enable_anti_steal_eat", True)):
                return "pass", None
            prot_badges: List[int] = list(map(int, [BADGE_CONDUCTION]))

            ef = _effects()

            kavi_idxs: List[int] = []
            for i, r in enumerate(ef):
                if _base(r.get("id")) == ID_KAVI and _bid(r) in prot_badges:
                    kavi_idxs.append(i)
            if not kavi_idxs:
                return "pass", None

            def theft_like(row: Optional[dict]) -> bool:
                if not isinstance(row, dict): return False
                b = _base(row.get("id"))
                if b == ID_THEFT: return True
                if b in (ID_HACKER, ID_UNSTABLE) and _first_src_base(row) == ID_THEFT: return True
                return False

            n = len(ef)
            risky_pairs: List[tuple[Optional[dict], dict, Optional[dict]]] = []
            for i in kavi_idxs:
                left = ef[i - 1] if i - 1 >= 0 else None
                right = ef[i + 1] if i + 1 < n else None
                if theft_like(right):
                    risky_pairs.append((left, ef[i], right))

            if not risky_pairs:
                return "pass", None

            protected_badges_text = "、".join(str(x) for x in prot_badges)

            lines: List[str] = []
            for (l, k, r) in risky_pairs:
                lines.append(f"• {_name(k)}（{_badge_label(k)}）")
                lines.append(f"  {_name(l) if l else '-'}（{_badge_label(l) if l else '-'}）")
                lines.append(f"  {_name(r) if r else '-'}（{_badge_label(r) if r else '-'}）")
                lines.append("")

            pairs_text = "\n".join(lines).strip()

            ok = _confirm_fuse_or_drop(
                cfg,
                title_key="fuse.guard.kaviTheft.title",
                message_key="fuse.guard.kaviTheft.message",
                values={
                    "protectedBadges": protected_badges_text,
                    "pairsText": pairs_text,
                },
            )
            return ("pass", None) if ok else ("drop", None)
        if view.get("type") == "Req" and view.get("method") == ".lq.Lobby.amuletActivityGameOperate":
            data = view.get("data")
            type = data.get("type")
            if type == 1:
                cfg = MANAGER.to_table_payload("fuse") or {}
                if not bool(cfg.get("enable_missing_hand_tile_guard", True)):
                    return "pass", None

                played_tiles = _coerce_int_list(data.get("tileList"))
                if played_tiles:
                    remaining_hand_tiles = _coerce_int_list(getattr(GAME_STATE, "hand_tiles", None) or [])
                    missing_tiles: List[int] = []
                    for tile_id in played_tiles:
                        try:
                            remaining_hand_tiles.remove(tile_id)
                        except ValueError:
                            missing_tiles.append(tile_id)
                    if missing_tiles:
                        logger.warning(
                            "blocked amuletActivityOperate type=1 with tiles not in hand: missing={}, hand={}, request={}",
                            missing_tiles,
                            _coerce_int_list(getattr(GAME_STATE, "hand_tiles", None) or []),
                            played_tiles,
                        )
                        hand_tiles = _coerce_int_list(getattr(GAME_STATE, "hand_tiles", None) or [])
                        ok = _confirm_fuse_or_drop(
                            cfg,
                            title_key="fuse.guard.missingHandTile.title",
                            message_key="fuse.guard.missingHandTile.message",
                            values={
                                "missingTiles": ", ".join(str(tile_id) for tile_id in missing_tiles),
                                "playedTiles": ", ".join(str(tile_id) for tile_id in played_tiles),
                                "handTiles": ", ".join(str(tile_id) for tile_id in hand_tiles),
                            },
                        )
                        return ("pass", None) if ok else ("drop", None)

        if view.get("type") == "Req" and view.get("method") == ".lq.Lobby.amuletActivityEndShopping":
            cfg = MANAGER.to_table_payload("fuse") or {}
            ef = _effects()

            if bool(cfg.get("enable_exit_coin_guard", True)):
                effect_base_ids = {_base(e.get("id", 0)) for e in ef}
                has_moon_protection = ID_MOON_PROTECTION in effect_base_ids
                has_star_vacuum = ID_STAR_VACUUM in effect_base_ids
                try:
                    coin = int(getattr(GAME_STATE, "coin", 0) or 0)
                except Exception:
                    coin = 0
                if has_moon_protection and has_star_vacuum and coin != 0:
                    moon_row = next((e for e in ef if _base(e.get("id", 0)) == ID_MOON_PROTECTION), None)
                    vacuum_row = next((e for e in ef if _base(e.get("id", 0)) == ID_STAR_VACUUM), None)
                    ok = _confirm_fuse_or_drop(
                        cfg,
                        title_key="fuse.guard.exitCoin.title",
                        message_key="fuse.guard.exitCoin.message",
                        values={
                            "coin": coin,
                            "moonId": ID_MOON_PROTECTION,
                            "moonName": _name(moon_row),
                            "vacuumId": ID_STAR_VACUUM,
                            "vacuumName": _name(vacuum_row),
                        },
                    )
                    if not ok:
                        return "drop", None

            if bool(cfg.get("enable_exit_life_guard", True)):
                has_life = any(_bid(e) == BADGE_LIFE for e in ef)
                if not has_life:
                    amulets_payload = [
                        {
                            "name": _name(r),
                            "badgeLabel": _badge_label(r),
                            "baseId": _base(r.get("id", 0)),
                            "rawId": int(r.get("id", 0) or 0),
                        }
                        for r in ef
                    ]
                    logger.info("run _ui_confirm_blocking")
                    ok = _confirm_fuse_or_drop(
                        cfg,
                        title_key="fuse.guard.noLife.title",
                        message_key="fuse.guard.noLife.message",
                        values={
                            "lifeBadgeId": BADGE_LIFE,
                            "amulets": amulets_payload,
                        },
                    )
                    if not ok:
                        return "drop", None
        return "pass", None
    except Exception:
        logger.exception("error occurred")
        return "pass", None


def on_inbound(view: Dict) -> Tuple[str, Any]:
    """
    以下是局面封包解释：

    注意：以下所有数组都具有有序性。

    1. pool
    数组 储存了牌对象 有108-109张，牌对象包含id和tile。
    有万象（护身符id为169）时pool才会有109个元素，其他情况下都是108张。万象的牌对象id为1000，tile为bd。且万象在pool的最后一张

    2. hands
    数组 储存了牌id。
    开局时一般取pool的前13张，有万象的时候取前12张，和pool的最后一张万象。

    3. dora
    数组 储存了牌id。
    一般为从手牌开始后的10张。也就是从第22（有万象）或23张开始到32（有万象）或33。

    4. lockedTile
    数组 储存了牌id。
    锁住的牌，该数组一定是牌山的牌的子集。锁住的牌虽然存在于pool，但并不会被玩家摸到。

    5. used
    数组 储存了牌id。
    换牌使用过的牌。

    6. usedDesktop
    数组 储存了牌id。
    牌山使用过的牌。

    7. changeTileCount 和 totalChangeTileCount
    整数。
    前者为已替换的次数，后者为最大可替换的次数。

    8. nextOperation
    数组  储存了操作对象。
    操作对象包含操作类型（int）和gang数组。gang数组一般情况下为空、除非操作类型为杠类型。操作类型为杠类型时gang数组保存的是可以杠的牌的id。

    9. point 和 targetPoint
    整数。
    前者为当前已达到的分数，后者为当前关卡目标分数。

    10. desktopRemain
    整数。
    牌山还剩下多少张牌。

    11. showDesktopTiles
    数组 储存了牌位置对象。
    位置对象包含id和pos两个参数，id为牌的id，pos为牌的位置。
    牌山的显示位置：
    牌山在桌面上总共显示为4行9列。从4行9列开始到1行1列的pos为0->35
    锁住的牌一定会被显示在最后，最后一张锁牌显示在牌山的最后。

    12. lockedTileCount
    整数。
    表示锁住的牌的数量。

    13. 牌山
    该数据不包含在封包中，需要自己推导。
    一般为dora后的36张。
    一般情况下玩家第几张摸到的牌就是牌山中第几张的牌。
    当玩家拥有221护身符的时候，摸到的牌的顺序改为一万->九万->一筒->九筒->一条->九条->东南西北白发中。但是pool的顺序不会改变、需要自己根据牌面调整实际上牌山的顺序。

    14. 换牌堆
    该数据不包含在封包中，需要自己推导。
    一般为牌山后的全部（万象除外，万象在玩家的手牌中，也可也视为把万象移动到了pool的第一张）。
    一般情况下玩家第几张换到的牌就是换牌堆中第几张的牌。
    """
    # MARK: on_inbound
    if dict(view["data"]).get("error", None) is not None:
        logger.error(f"error occurred: {dict(view['data'])['error']}")
        return "pass", None
    # 服务器下发公告
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.fetchAnnouncement" and MANAGER.get("game.modify_announcement"):
        newd = dict(view["data"])
        anns: List[Dict] = newd.get("announcements", [])
        anns.insert(0, {
            "id": 9999,
            "title": "欢迎使用向听镜·向聴レンズへようこそ",
            "content": "向听镜已启动，祝各位大大欧气满满！\n向聴レンズが起動しました！みなさんにガチャ運がモリモリ湧いてきますように！",
            "headerImage": "internal://2.jpg"
        })
        return "modify", newd
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityFetchBrief" and MANAGER.get("game.unlock_illustrated_book"):
        dataBig = dict(view["data"])
        illustrated_book = dataBig.get("illustratedBook", None)
        if illustrated_book:
            effect_collection = illustrated_book.get("effectCollection", [])
            effect_collection.clear()
            for effect_id in range(1, 500):
                effect_collection.append(int(str(effect_id) + "0"))
                effect_collection.append(int(str(effect_id) + "1"))
            dataBig["illustratedBook"]["effectCollection"] = effect_collection
            dataBig["illustratedBook"]["badgeCollection"] = [badge_id for badge_id in range(600000, 600500)]
            dataBig["illustratedBook"]["runeStoneCollection"] = [badge_id for badge_id in range(7200, 7300)]
            # dataBig["gameRecords"] = []
            #
            # group_start = 45
            # for n in range(group_start, group_start + 5):
            #     demoGame = {"effectBuilds": [{}, {}, {}, {}, {}, {}, {}, {}]}
            #
            #     start = n * 8
            #
            #     for i in range(1, 9):
            #         demoGame["effectBuilds"][i - 1] = {
            #             "id": int((start + i) * 10),
            #             "badgeId": 600300,
            #             "volume": 1
            #         }
            #
            #     demoGame["level"] = 1200
            #     demoGame["highestLevelScore"] = str(start + 1)
            #     demoGame["highestFan"] = str(start + 8)
            #     demoGame["time"] = 1000000000 - n
            #
            #     dataBig["gameRecords"].append(demoGame)
            # dataBig["statistic"] = {}
        return "modify", dataBig
    # 通用操作
    # MARK: amuletActivityOperate
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityOperate":
        data = dict(view["data"])
        events = data.get("events", [])
        modify = False
        # 进入新节点
        entry_new_node = next((e for e in events if e.get("type") == 21), None)
        if entry_new_node:
            map_info = entry_new_node.get("valueChanges", {}).get("map", {})
            node = _value_change_value(map_info, "node", None)
            GAME_STATE.update_other_info(node=node, reason=".lq.Lobby.amuletActivityGameOperate:21")
        # 新关卡开始
        start_new_game = next((e for e in events if e.get("type") == 4), None)
        if start_new_game:
            state = start_new_game.get("state", {})
            stage = state.get("current", -1)
            value_changes = start_new_game.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            total_change_tile_count = round_info.get("totalChangeTileCount", {}).get("value", None)
            change_tile_count = round_info.get("changeTileCount", {}).get("value", None)
            hands = round_info.get("hands", {}).get("value", None)
            pool = round_info.get("pool", {}).get("value", None)
            dora_tiles = round_info.get("dora", {}).get("value", None)
            tian_dora_tiles = round_info.get("tianDora", {}).get("value", None)
            ming = round_info.get("ming", {}).get("value", None)
            ting_list = round_info.get("tingList", {}).get("value", None)
            next_operation = round_info.get("nextOperation", {}).get("value", None)
            locked_tiles = round_info.get("lockedTile", {}).get("value", None)
            used_desktop = round_info.get("usedDesktop", {}).get("value", None)
            enemy = round_info.get("enemy", {}).get("value", {})
            point = enemy.get("hp", "0")
            target_point = enemy.get("maxHp", "0")
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            map_info = value_changes.get("map", {})
            node = _value_change_value(map_info, "node", None)
            map_nodes = _value_change_value(map_info, "mapNodes", None)
            used = round_info.get("used", {}).get("value", None)
            record = value_changes.get("record", None)
            GAME_STATE.update_record(record)
            if hands and pool:
                use_current_hand_for_sections = has_amulet_2250_or_2251(effect_list)
                GAME_STATE.update_other_info(node=node, map_nodes=map_nodes, push_gamestate=False)
                GAME_STATE.update_pool(pool, hand_tiles=hands, locked_tiles=locked_tiles, used=used, dora_tiles=dora_tiles, used_desktop=used_desktop, use_current_hand_for_sections=use_current_hand_for_sections, push_gamestate=False)
                new_wall = reorder_wall_tiles_by_amulet221(GAME_STATE.deck_map, GAME_STATE.wall_tiles, effect_list)
                GAME_STATE.update_wall(new_wall)
                desktop_remain = round_info.get("desktopRemain", {}).get("value", 0)
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=False, effect_list=effect_list, ting_list=ting_list, next_operation=next_operation, total_change_tile_count=total_change_tile_count, change_tile_count=change_tile_count, target_point=target_point, point=point, tian_dora_tiles=tian_dora_tiles, ming=ming, reason=".lq.Lobby.amuletActivityOperate:4")
                if MANAGER.get("game.public_all"):
                    show_desktop_tiles = round_info.get("showDesktopTiles", {}).get("value", [])
                    show_desktop_tiles.clear()
                    pos = len(GAME_STATE.wall_tiles) + len(GAME_STATE.locked_tiles) - 1

                    for tile in GAME_STATE.wall_tiles:
                        show_desktop_tiles.append({"id": tile, "pos": pos})
                        pos -= 1
                    for tile in GAME_STATE.locked_tiles:
                        show_desktop_tiles.append({"id": tile, "pos": pos})
                        pos -= 1
                    if has_amulet_221(GAME_STATE.effect_list):
                        event_3 = next((e for e in events if e.get("type") == 3), None)
                        if event_3:
                            show_desktop_tiles_list = [
                                event_3.get("valueChanges", {}).get("round", {}).get("showDesktopTiles", {}).get("value", []),
                                next(iter(event_3.get("effectedHooks", []))).get("result", {}).get("modifyChangeDesktop", {}).get("showDesktopTiles", [])
                            ]
                            for tile_list in show_desktop_tiles_list:
                                tile_list.clear()
                                for tile in show_desktop_tiles:
                                    tile_list.append(tile)
                    modify = True
        # 跳过换牌
        skip_switch = next((e for e in events if e.get("type") == 10), None)
        if skip_switch:
            _handle_type10_draw_event(skip_switch, ".lq.Lobby.amuletActivityOperate:10")
        # 购买卡包
        buy_pack = next((e for e in events if e.get("type") == 51), None)
        if buy_pack:
            value_changes = buy_pack.get("valueChanges", {})
            state = buy_pack.get("state", {})
            stage = state.get("current", -1)
            game = value_changes.get("game", {})
            coin = game.get("coin", {}).get("value", -1)
            shop = value_changes.get("shop", {})
            goods = shop.get("goods", {}).get("value", None)
            record = value_changes.get("record", None)
            GAME_STATE.update_record(record)
            effect = value_changes.get("effect", {})
            candidate_effect_list = effect.get("packCandidates", {}).get("value", None)
            GAME_STATE.update_other_info(stage=stage, coin=coin, candidate_effect_list=candidate_effect_list, goods=goods, reason=".lq.Lobby.amuletActivityOperate:57")
        # 选择护身符
        select_amulet = next((e for e in events if e.get("type") == 53), None)
        if select_amulet:
            value_changes = select_amulet.get("valueChanges", {})
            effect = value_changes.get("effect", {})
            effect_list = effect.get("effectList", {}).get("value", None)
            state = select_amulet.get("state", {})
            stage = state.get("current", -1)
            record = value_changes.get("record", None)
            candidate_effect_list = effect.get("packCandidates", {}).get("value", None)
            GAME_STATE.update_record(record)
            GAME_STATE.update_other_info(stage=stage, effect_list=effect_list, candidate_effect_list=candidate_effect_list, reason=".lq.Lobby.amuletActivityOperate:53")
        # 刷新商店
        refresh_shop = next((e for e in events if e.get("type") == 24), None)
        if refresh_shop:
            value_changes = refresh_shop.get("valueChanges", {})
            state = refresh_shop.get("state", {})
            stage = state.get("current", -1)
            game = value_changes.get("game", {})
            coin = int(game.get("coin", {}).get("value", None))
            record = value_changes.get("record", None)
            shop = value_changes.get("shop", {})
            goods = shop.get("goods", {}).get("value", None)
            refresh_price = shop.get("refreshPrice", {}).get("value", None)
            GAME_STATE.update_record(record)
            GAME_STATE.update_other_info(stage=stage, coin=coin, goods=goods, refresh_price=refresh_price, reason=".lq.Lobby.amuletActivityOperate:24")
        end_shopping = next((e for e in events if e.get("type") == 28), None)
        if end_shopping:
            value_changes = end_shopping.get("valueChanges", {})
            shop = value_changes.get("shop", {})
            goods = _value_change_value(shop, "goods", [])
            GAME_STATE.update_other_info(goods=goods, reason=".lq.Lobby.amuletActivityOperate:28")
        entry_level_select = next((e for e in events if e.get("type") == 3), None)
        if entry_level_select:
            state = entry_level_select.get("state", {})
            stage = state.get("current", -1)
            map_info = entry_level_select.get("valueChanges", {}).get("map", {})
            level = _value_change_value(map_info, "level", None)
            node = _value_change_value(map_info, "node", None)
            map_nodes = _value_change_value(map_info, "mapNodes", None)
            GAME_STATE.update_other_info(stage=stage, node=node, level=level, map_nodes=map_nodes, reason=".lq.Lobby.amuletActivityGameOperate:3")
        sell_effect = next((e for e in events if e.get("type") == 27), None)
        if sell_effect:
            value_changes = sell_effect.get("valueChanges", {})
            game = value_changes.get("game", {})
            state = sell_effect.get("state", {})
            stage = state.get("current", -1)
            coin = int(game.get("coin", {}).get("value", None))
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            ended = value_changes.get("ended", False)
            record = value_changes.get("record", None)
            shop = value_changes.get("shop", {})
            goods = shop.get("goods", {}).get("value", None)
            GAME_STATE.update_record(record)
            GAME_STATE.update_other_info(stage=stage, coin=coin, ended=ended, effect_list=effect_list, goods=goods, reason=".lq.Lobby.amuletActivityOperate:27")
        amulet_sort = next((e for e in events if e.get("type") == 14), None)
        if amulet_sort:
            value_changes = amulet_sort.get("valueChanges", {})
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            GAME_STATE.update_other_info(effect_list=effect_list, reason=".lq.Lobby.amuletActivityOperate:14")
        if modify:
            return "modify", data
    # 游戏中打牌等操作
    # MARK: amuletActivityGameOperate
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityGameOperate":
        data = view.get("data", {})
        events = data.get("events", [])
        modify = False
        # # type = 100: 游戏结束
        # end_event = next((e for e in events if e.get("type") == 100), None)
        # if end_event:
        #     value_changes = end_event.get("valueChanges", {})
        #     stage = value_changes.get("stage", -1)
        #     ended = value_changes.get("ended", True)
        #     GAME_STATE.update_other_info(stage=stage, ended=ended, reason=".lq.Lobby.amuletActivityOperate:100")
        #     return "pass", None
        # type = 7: 换牌
        switch_event = next((e for e in events if e.get("type") == 7), None)
        if switch_event:
            state = switch_event.get("state", {})
            stage = state.get("current", -1)
            value_changes = switch_event.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            change_tile_count = round_info.get("changeTileCount", {}).get("value", None)
            used = round_info.get("used", {}).get("value", [])
            GAME_STATE.update_switch_used_tiles(used=used, push_gamestate=False, reason=".lq.Lobby.amuletActivityGameOperate:7")
            hands = round_info.get("hands", {}).get("value", [])
            GAME_STATE.update_hand_tiles(hand_tiles=hands, push_gamestate=False)
            next_operation = round_info.get("nextOperation", {}).get("value", None)
            ting_list = round_info.get("tingList", {}).get("value", None)
            GAME_STATE.update_other_info(stage=stage, change_tile_count=change_tile_count, next_operation=next_operation, ting_list=ting_list)
        # type = 13: 苦战发牌
        redeal = next((e for e in events if e.get("type") == 13), None)
        if redeal:
            value_changes = redeal.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            state = redeal.get("state", {})

            pool = round_info.get("pool", {}).get("value", None)
            used_desktop = round_info.get("usedDesktop", {}).get("value", None)
            locked_tile = round_info.get("lockedTile", {}).get("value", None)
            desktop_remain = round_info.get("desktopRemain", {}).get("value", None)
            stage = state.get("current", -1)
            if pool:
                GAME_STATE.deck_map.clear()
                for item in pool:
                    GAME_STATE.deck_map[item["id"]] = item["tile"]
                GAME_STATE.locked_tiles = locked_tile.copy() if locked_tile else []
                GAME_STATE.used_desktop_tiles = used_desktop.copy() if used_desktop else []
                GAME_STATE.update_wall(_redeal_wall_tile_ids(
                    pool, GAME_STATE.hand_tiles, GAME_STATE.ming, GAME_STATE.dora_tiles, _redeal_wall_limit(desktop_remain)
                ))
                GAME_STATE.update_other_info(
                    desktop_remain=desktop_remain,
                    stage=stage,
                    reason=".lq.Lobby.amuletActivityGameOperate:13",
                )
                if MANAGER.get("game.public_all"):
                    show_desktop_tiles = round_info.get("showDesktopTiles", {}).get("value", [])
                    show_desktop_tiles.clear()
                    pos = len(GAME_STATE.wall_tiles) + len(GAME_STATE.locked_tiles) - 1
                    for tile in GAME_STATE.wall_tiles:
                        show_desktop_tiles.append({"id": tile, "pos": pos})
                        pos -= 1
                    for tile in GAME_STATE.locked_tiles:
                        show_desktop_tiles.append({"id": tile, "pos": pos})
                        pos -= 1
                    modify = True
        # type = 10: 摸牌
        draw_event = next((e for e in events if e.get("type") == 10), None)
        if draw_event:
            _handle_type10_draw_event(draw_event, ".lq.Lobby.amuletActivityGameOperate:10")
        # type = 15: 杠牌
        kang_event = next((e for e in events if e.get("type") == 15), None)
        if kang_event:
            value_changes = kang_event.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            ming = round_info.get("ming", {}).get("value", None)
            GAME_STATE.update_other_info(ming=ming,
                                         reason=".lq.Lobby.amuletActivityGameOperate:15")
        # type = 18: 自摸后的数据成长
        after_tsumo = next((e for e in events if e.get("type") == 18), None)
        if after_tsumo:
            value_changes = after_tsumo.get("valueChanges", {})
            game = value_changes.get("game", {})
            coin = _value_change_value(game, "coin")
            tile_score_map = _parse_tile_score_map(game.get("tileScoreMap", {}).get("value", None))
            fan_value_map = _parse_fan_value_map(game.get("fanValueMap", {}).get("value", None))
            GAME_STATE.update_other_info(coin=coin, tile_score_map=tile_score_map, fan_value_map=fan_value_map,
                                         reason=".lq.Lobby.amuletActivityGameOperate:18")
        # coin_event = next((e for e in events if e.get("type") == 11), None)
        # if coin_event:
        #     value_changes = coin_event.get("valueChanges", {})
        #     effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
        #     coin = int(value_changes.get("game", {}).get("coin", {}).get("value", None))
        #     GAME_STATE.update_other_info(coin=coin, effect_list=effect_list, reason=".lq.Lobby.amuletActivityOperate:11")
        shop_event = next((e for e in events if e.get("type") == 23), None)
        if shop_event:
            value_changes = shop_event.get("valueChanges", {})
            shop = value_changes.get("shop", {})
            goods = shop.get("goods", {}).get("value", None)
            refresh_price = shop.get("refreshPrice", {}).get("value", None)
            state = shop_event.get("state", {})
            stage = state.get("current", -1)
            GAME_STATE.update_other_info(goods=goods, refresh_price=refresh_price, stage=stage, reason=".lq.Lobby.amuletActivityGameOperate:23")
        # reward_pack_event = next((e for e in events if e.get("type") == 15), None)
        # if reward_pack_event:
        #     value_changes = reward_pack_event.get("valueChanges", {})
        #     effect = value_changes.get("effect", {})
        #     level_reward_candidates = effect.get("levelRewardCandidates", {}).get("value", None)
        #     stage = value_changes.get("stage", -1)
        #     GAME_STATE.update_other_info(candidate_effect_list=level_reward_candidates, stage=stage, reason=".lq.Lobby.amuletActivityOperate:15")
        # finish_event = next((e for e in events if e.get("type") == 24), None)
        # if finish_event:
        #     value_changes = finish_event.get("valueChanges", {})
        #     stage = value_changes.get("stage", -1)
        #     GAME_STATE.update_other_info(stage=stage, reason=".lq.Lobby.amuletActivityOperate:24")
        # _handle_amulet_activity_operate_type8_events(events)
        if modify:
            return "modify", data
    # 进入青云之志界面时获取已经开始的游戏数据
    # MARK: fetchAmuletActivityData
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.fetchAmuletActivityData":
        dataBig = dict(view["data"])
        data = dataBig.get("data", {})
        game = data.get("game", None)
        if game:
            round_info = game.get("round", {})
            hands = round_info.get("hands", [])
            pool = round_info.get("pool", [])
            dora_tiles = round_info.get("dora", [])
            tian_dora_tiles = round_info.get("tianDora", [])
            ming = round_info.get("ming", [])
            locked_tiles = round_info.get("lockedTile", [])
            effect = game.get("effect", {})
            effect_list = effect.get("effectList", None)
            total_chance_tile_count = round_info.get("totalChangeTileCount", None)
            chance_tile_count = round_info.get("changeTileCount", None)
            used = round_info.get("used", [])
            used_desktop = round_info.get("usedDesktop", [])
            desktop_remain = round_info.get("desktopRemain", 0)
            enemy = round_info.get("enemy", {})
            point = enemy.get("hp", "0")
            target_point = enemy.get("maxHp", "0")
            character = game.get("character", None)
            state = game.get("state", {})
            stage = state.get("current", -1)
            try:
                change_tile_count_int = int(chance_tile_count or 0)
            except (TypeError, ValueError):
                change_tile_count_int = -1
            use_current_hand_for_sections = (
                    has_amulet_2250_or_2251(effect_list)
                    and int(stage or 0) == 2
                    and change_tile_count_int == 0
            )
            map_info = game.get("map", {})
            level = map_info.get("level", None)
            node = map_info.get("node", None)
            map_nodes = map_info.get("mapNodes", None)
            GAME_STATE.update_other_info(node=node, map_nodes=map_nodes, level=level, push_gamestate=False)
            GAME_STATE.update_pool(pool, hand_tiles=hands, locked_tiles=locked_tiles, push_gamestate=False, used=used, dora_tiles=dora_tiles, used_desktop=used_desktop, use_current_hand_for_sections=use_current_hand_for_sections, reason=".lq.Lobby.fetchAmuletActivityData")
            is_redeal_stage = int(stage or 0) == 7
            if is_redeal_stage:
                GAME_STATE.update_wall(_redeal_wall_tile_ids(
                    pool, hands, ming, dora_tiles, _redeal_wall_limit(desktop_remain)
                ))
            game_info = game.get("game", {})
            coin = int(_value_change_value(game_info, "coin", ""))
            tile_score_map = _parse_tile_score_map(_value_change_value(game_info, "tileScoreMap", None))
            fan_value_map = _parse_fan_value_map(_value_change_value(game_info, "fanValueMap", None))
            shop = game.get("shop", {})
            max_effect_volume = effect.get("maxEffectVolume", 0)
            shop_buff_list = _parse_shop_buff_list(effect.get("shopBuffList", None))
            candidate_effect_list = effect.get("packCandidates", [])
            goods = shop.get("goods", [])
            refresh_price = shop.get("refreshPrice", 0)
            record = game.get("record", None)
            ting_list = round_info.get("tingList", None)
            next_operation = round_info.get("nextOperation", None)
            GAME_STATE.update_record(record)
            if is_redeal_stage:
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, effect_list=effect_list, candidate_effect_list=candidate_effect_list, coin=coin, ting_list=ting_list, character=character, next_operation=next_operation, goods=goods, refresh_price=refresh_price, total_change_tile_count=total_chance_tile_count, change_tile_count=chance_tile_count, max_effect_volume=max_effect_volume, shop_buff_list=shop_buff_list, tile_score_map=tile_score_map, fan_value_map=fan_value_map, target_point=target_point, point=point, tian_dora_tiles=tian_dora_tiles, ming=ming, push_gamestate=True)
            elif desktop_remain < 36:
                new_wall = reorder_wall_tiles_by_amulet221(GAME_STATE.deck_map, GAME_STATE.wall_tiles, effect_list)
                GAME_STATE.update_wall(new_wall)
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, effect_list=effect_list, candidate_effect_list=candidate_effect_list, coin=coin, ting_list=ting_list, character=character, next_operation=next_operation, goods=goods, refresh_price=refresh_price, total_change_tile_count=total_chance_tile_count, change_tile_count=chance_tile_count, max_effect_volume=max_effect_volume, shop_buff_list=shop_buff_list, tile_score_map=tile_score_map, fan_value_map=fan_value_map, target_point=target_point, point=point, tian_dora_tiles=tian_dora_tiles, ming=ming, push_gamestate=False)
                GAME_STATE.refresh_wall_by_remaning()
            else:
                new_wall = reorder_wall_tiles_by_amulet221(GAME_STATE.deck_map, GAME_STATE.wall_tiles, effect_list)
                GAME_STATE.update_wall(new_wall)
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, effect_list=effect_list, candidate_effect_list=candidate_effect_list, coin=coin, ting_list=ting_list, character=character, next_operation=next_operation, goods=goods, refresh_price=refresh_price, total_change_tile_count=total_chance_tile_count, change_tile_count=chance_tile_count, max_effect_volume=max_effect_volume, shop_buff_list=shop_buff_list, tile_score_map=tile_score_map, fan_value_map=fan_value_map, target_point=target_point, point=point, tian_dora_tiles=tian_dora_tiles, ming=ming, push_gamestate=True)
            error_number_test = MANAGER.get("general.error_code_test")
            if error_number_test != 0:
                return "modify", dict({"error": {"code": error_number_test, "u32Params": [], "strParams": [], "jsonParam": ""}})
            if MANAGER.get("game.public_all"):
                show_desktop_tiles = round_info.get("showDesktopTiles", [])
                show_desktop_tiles.clear()
                pos = len(GAME_STATE.wall_tiles) + len(GAME_STATE.locked_tiles) - 1
                for tile in GAME_STATE.wall_tiles:
                    show_desktop_tiles.append({"id": tile, "pos": pos})
                    pos -= 1
                for tile in GAME_STATE.locked_tiles:
                    show_desktop_tiles.append({"id": tile, "pos": pos})
                    pos -= 1
                return "modify", dataBig
    # 只是用来更新一下状态
    # MARK: amuletActivityGiveup
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityGiveup":
        GAME_STATE.on_giveup()
    # MARK: amuletActivityStartGame
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityStartGame":
        data = view.get("data", {})
        events = data.get("events", [])
        start_event = next((e for e in events if e.get("type") == 1), None)
        result = start_event.get("result", {}).get("newGameResult", {})
        state = result.get("state", {})
        stage = state.get("current", 1)
        ended = result.get("ended", False)
        record = result.get("record", None)
        effect = result.get("effect", None)
        max_effect_volume = effect.get("maxEffectVolume", None)
        GAME_STATE.update_record(record)
        GAME_STATE.update_other_info(stage=stage, ended=ended, max_effect_volume=max_effect_volume, reason=".lq.Lobby.amuletActivityStartGame:1")
    # MARK: amuletActivityUpgradeShopBuff
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityUpgradeShopBuff":
        data = view.get("data", {})
        events = data.get("events", [])
        upgrade_shop_buff = next((e for e in events if e.get("type") == 21), None)
        if upgrade_shop_buff:
            value_changes = upgrade_shop_buff.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            tian_dora_tiles = round_info.get("tianDora", {}).get("value", None)
            ming = round_info.get("ming", {}).get("value", None)
            game = value_changes.get("game", {})
            coin = int(game.get("coin", {}).get("value", None))
            effect = value_changes.get("effect", {})
            shop_buff_list = _parse_shop_buff_list(effect.get("shopBuffList", {}).get("value", None))
            record = value_changes.get("record", None)
            GAME_STATE.update_record(record)
            GAME_STATE.update_other_info(coin=coin, shop_buff_list=shop_buff_list, tian_dora_tiles=tian_dora_tiles, ming=ming, reason=".lq.Lobby.amuletActivityUpgradeShopBuff:21")
    return "pass", None


def _handle_amulet_activity_operate_type8_events(events: List[Dict[str, Any]]) -> None:
    type8_events = [event for event in events if event.get("type") == 8]
    if not type8_events:
        return

    current_point = GAME_STATE.point
    tile_score_map = None
    fan_value_map = None
    ming = None
    for index, event in enumerate(type8_events):
        is_last_event = index == len(type8_events) - 1
        value_changes = event.get("valueChanges", None)

        if value_changes:
            if is_last_event:
                game_changes = value_changes.get("game", {})
                tile_score_map = _parse_tile_score_map(game_changes.get("tileScoreMap", {}).get("value", None))
                fan_value_map = _parse_fan_value_map(game_changes.get("fanValueMap", {}).get("value", None))
                point = value_changes.get("round", {}).get("point", {}).get("value", "0")
                current_point = point
    GAME_STATE.update_other_info(point=current_point, tile_score_map=tile_score_map, fan_value_map=fan_value_map, push_gamestate=True, reason=".lq.Lobby.amuletActivityOperate:8")


def _parse_shop_buff_list(raw_list: Any) -> Dict[int, int] | None:
    if raw_list is None:
        return None
    if not isinstance(raw_list, list):
        return {}

    out: Dict[int, int] = {}
    for entry in raw_list:
        if not isinstance(entry, dict):
            continue
        try:
            buff_id = int(entry.get("id"))
        except (TypeError, ValueError):
            continue
        store = entry.get("store")
        if not isinstance(store, list) or len(store) <= 0:
            out[buff_id] = 0
            continue
        try:
            out[buff_id] = int(store[0])
        except (TypeError, ValueError):
            out[buff_id] = 0
    return out


def has_amulet_221(effects: List[Dict[str, Any]]) -> bool:
    for e in effects or []:
        try:
            eid = int(e.get("id", -1))
        except Exception:
            continue
        if eid // 10 == 221:
            return True
        # 不稳定护身符
        if eid == 2280:
            store = e.get("store", [])
            if len(store) >= 1:
                if int(store[0]) // 10 == 221:
                    return True
    return False


def has_amulet_2250_or_2251(effects: List[Dict[str, Any]]) -> bool:
    for e in effects or []:
        try:
            eid = int(e.get("id", -1))
        except Exception:
            continue
        if eid in {2250, 2251}:
            return True
        if eid == 2280:
            store = e.get("store", [])
            if len(store) >= 1:
                try:
                    if int(store[0]) in {2250, 2251}:
                        return True
                except (TypeError, ValueError):
                    continue
    return False


def reorder_wall_tiles_by_amulet221(
        deck_map: Union[Dict[int, str], List[Dict[str, Any]]],
        wall_tiles: List[int],
        effect_list: List[Dict[str, Any]],
) -> List[int]:
    if not has_amulet_221(effect_list):
        return list(wall_tiles)
    if isinstance(deck_map, dict):
        id2tile: Dict[int, str] = deck_map
    else:
        id2tile = {int(x["id"]): str(x["tile"]) for x in deck_map}

    suit_order_digits = ["1", "2", "3", "4", "0", "5", "6", "7", "8", "9"]
    order = []
    for s in ("m", "p", "s"):
        order += [d + s for d in suit_order_digits]
    order += [str(d) + "z" for d in range(1, 8)]

    rank = {t: i for i, t in enumerate(order)}
    original_index = {tid: i for i, tid in enumerate(wall_tiles)}

    def sort_key(tid: int):
        tile = id2tile.get(tid, "")
        return rank.get(tile, len(order)), original_index[tid]

    return sorted(wall_tiles, key=sort_key)
