import asyncio
import copy
import threading
from collections import OrderedDict
from typing import Tuple, Any, Dict, List, Set, Optional, Union, Sequence
import time

from loguru import logger
from mitmproxy import ctx

import backend.app
import backend.mitm.addon as _addon
from backend import big_number
from backend.app import AMULET_REG, BADGE_REG
from backend.app import MANAGER, GAME_STATE, broadcast
from backend.autorun.util.chiitoi_recommender import chiitoi_recommendation_json
from backend.autorun.util.retry_1004 import call_with_1004_retry_async
from backend.autorun.util.souzu_switch_recommender import (
    recommend_souzu_tenpai_switch,
    list_reachable_quads_for_switch,
    validate_manual_souzu_switch_plan,
)
from backend.autorun.util.suannkou_recommender import plan_pure_pinzu_suu_ankou_v2
from backend.msgbox import _ui_confirm_blocking, ui_alert

ID_KAVI = 230
BADGE_LIFE = 600100
BADGE_CONDUCTION = 600170
BADGE_EXPANSION = 600160
ID_UNSTABLE = 228
ID_THEFT = 229
ID_HACKER = 232
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
    #         data={"activityId": 250811, "type": 8, "tileList": []},
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
        if cur_stage != 2 or cur_count > prev_change_count:
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


async def execute_current_switch_plan() -> tuple[bool, str]:
    plan = copy.deepcopy(_LAST_SWITCH_PLAN) if isinstance(_LAST_SWITCH_PLAN, dict) else None
    if not plan or plan.get("status") != "plan":
        return False, "当前没有可执行的黑洞换牌方案。"

    batches = _plan_batches(plan)
    if not batches:
        return False, "当前方案没有可执行的换牌步骤。"

    if int(getattr(GAME_STATE, "stage", 0) or 0) != 2:
        return False, "当前不在换牌阶段，无法执行黑洞换牌。"

    bot = getattr(backend.app, "PACKET_BOT", None)
    if not bot:
        return False, "发包模块未就绪，请先进入青云之志并保持连接。"

    for index, discard_ids in enumerate(batches, start=1):
        current_hand = list(getattr(GAME_STATE, "hand_tiles", None) or [])
        if not current_hand:
            return False, "当前手牌为空，无法继续换牌。"
        missing = [tid for tid in discard_ids if tid not in current_hand]
        if missing:
            return False, f"第 {index} 步换牌所需的手牌已发生变化，请重进青云之志查看最新状态。"

        prefer_keep = [tid for tid in current_hand if tid not in discard_ids]
        buffs = set(getattr(GAME_STATE, "boss_buff", None) or [])
        if 901 in buffs:
            if len(discard_ids) > 3:
                return False, f"第{index}步方案要求换出 {len(discard_ids)} 张，超过 901 的单次上限 3 张。"
            filtered_ids = prefer_keep
        else:
            filtered_ids = prefer_keep

        prev_change_count = int(getattr(GAME_STATE, "change_tile_count", 0) or 0)
        ok, reason, _resp = await call_with_1004_retry_async(
            bot.op_change,
            tile_ids=filtered_ids,
            delay_sec=3,
            interval=3,
            timeout=3000,
            to_thread=True,
        )
        if not ok:
            return False, reason or f"第 {index} 步换牌失败"
        advanced = await _wait_for_switch_state_advance(prev_change_count)
        if not advanced and index < len(batches):
            return False, f"第 {index} 步换牌后状态未及时刷新，请重进青云之志查看最新状态。"

    await ui_alert(
        title_key="黑洞换牌已完成",
        message_key="已按当前方案完成全部换牌，请按“跳过”键结束换牌阶段。",
        ok_key="common.ok",
        timeout=45.0,
    )
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
    deck_map = state.get("deck_map") or {}
    if any(str(face) == "bd" for face in deck_map.values()):
        return "当前有万象，暂不支持条子换牌推荐"
    return None


def _souzu_search_algorithm_label(value: str) -> str:
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
    search_algorithm = "target_enumeration_search"
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

    search_task = asyncio.create_task(asyncio.to_thread(
        recommend_souzu_tenpai_switch,
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
    if int(state.get("stage", 0) or 0) != 2:
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

    if int(state.get("stage", 0) or 0) != 2:
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


# MARK: on_outbound
def on_outbound(view: Dict) -> Tuple[str, Any]:
    if backend.app.AUTORUNNER.running:
        return "pass", None
    try:
        if view.get("type") == "Req" and view.get("method") == ".lq.Lobby.amuletActivitySelectPack":
            data = view.get("data") or {}
            raw_id = int(data.get("id", 0))
            cfg = MANAGER.to_table_payload("fuse") or {}
            if raw_id == 0:
                if bool(cfg.get("enable_skip_guard", True)):
                    has_hit, values = _fuse_hits_values()
                    if has_hit:
                        ok = _ui_confirm_blocking(
                            title_key="fuse.guard.skipPack.title",
                            message_key="fuse.guard.skipPack.message",
                            values=values,
                            ok_key="common.continue",
                            cancel_key="common.cancel",
                            timeout=45.0,
                        )
                        return ("pass", None) if ok else ("drop", None)
                return "pass", None
            if bool(cfg.get("enable_shop_force_pick", False)):
                hit_exist, picked_is_hit, values = _must_pick_guard(raw_id)
                if hit_exist and not picked_is_hit:
                    ok = _ui_confirm_blocking(
                        title_key="fuse.guard.forcePick.title",
                        message_key="fuse.guard.forcePick.message",
                        values=values,
                        ok_key="common.continue",
                        cancel_key="common.cancel",
                        timeout=45.0,
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
                                ok = _ui_confirm_blocking(
                                    title_key="fuse.guard.kaviPrestartConduction.title",
                                    message_key="fuse.guard.kaviPrestartConduction.message",
                                    values=values,
                                    ok_key="common.continue",
                                    cancel_key="common.cancel",
                                    timeout=45.0,
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
                            ok2 = _ui_confirm_blocking(
                                title_key="fuse.guard.kaviPrestartExpansion.title",
                                message_key="fuse.guard.kaviPrestartExpansion.message",
                                values=values2,
                                ok_key="common.continue",
                                cancel_key="common.cancel",
                                timeout=45.0,
                            )
                            return ("pass", None) if ok2 else ("drop", None)

            return "pass", None
        # 黑客、不稳定存的第一个数据为复制或变身的护身符：{"id":2320,"store":[2290,1234]} 229为盗印，不稳定228、黑客232、卡维230
        if view.get("type") == "Req" and view.get("method") == ".lq.Lobby.amuletActivityOperate":
            cfg = MANAGER.to_table_payload("fuse") or {}
            if not bool(cfg.get("enable_anti_steal_eat", True)):
                return "pass", None
            if view.get("data").get("type") != 8:
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

            ok = _ui_confirm_blocking(
                title_key="fuse.guard.kaviTheft.title",
                message_key="fuse.guard.kaviTheft.message",
                values={
                    "protectedBadges": protected_badges_text,
                    "pairsText": pairs_text,
                },
                ok_key="common.continue",
                cancel_key="common.cancel",
                timeout=45.0,
            )
            return ("pass", None) if ok else ("drop", None)
        if view.get("type") == "Req" and view.get("method") == ".lq.Lobby.amuletActivityEndShopping":
            cfg = MANAGER.to_table_payload("fuse") or {}
            if not bool(cfg.get("enable_exit_life_guard", True)):
                return "pass", None
            ef = _effects()
            has_life = any(_bid(e) == BADGE_LIFE for e in ef)
            if has_life:
                return "pass", None

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
            ok = _ui_confirm_blocking(
                title_key="fuse.guard.noLife.title",
                message_key="fuse.guard.noLife.message",
                values={
                    "lifeBadgeId": BADGE_LIFE,
                    "amulets": amulets_payload,
                },
                ok_key="common.continue",
                cancel_key="common.cancel",
                timeout=45.0,
            )
            return ("pass", None) if ok else ("drop", None)
        return "pass", None
    except Exception:
        logger.exception("error occurred")
        return "pass", None


def on_inbound(view: Dict) -> Tuple[str, Any]:
    """
    .lq.Lobby.fetchAmuletActivityData           进入青云之志界面
    .lq.Lobby.amuletActivityGiveup              放弃
    .lq.Lobby.amuletActivityOperate             游戏中打牌等操作
    .lq.Lobby.amuletActivityStartGame           游戏开始，这回合获得的牌山数组似乎没有任何作用
    .lq.Lobby.amuletActivityUpgrade             回合开始

    .lq.Lobby.amuletActivitySelectFreeEffect    选择免费的卡包
    .lq.Lobby.amuletActivityBuy                 购买卡包
    .lq.Lobby.amuletActivitySelectPack          选择卡护身符、跳过
    .lq.Lobby.amuletActivitySellEffect          卖出护身符
    .lq.Lobby.amuletActivityEffectSort          对护身符排序
    .lq.Lobby.amuletActivityUpgradeShopBuff     升级增益
    .lq.Lobby.amuletActivityRefreshShop         刷新商店
    .lq.Lobby.amuletActivityEndShopping         购买结束

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
    # 开始新游戏
    # MARK: amuletActivityUpgrade
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityUpgrade":
        data = dict(view["data"])
        modify = False
        events = data.get("events", [])
        matched = next((e for e in events if e.get("type") == 23), None)
        if matched:
            value_changes = matched.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            total_change_tile_count = round_info.get("totalChangeTileCount", {}).get("value", None)
            change_tile_count = round_info.get("changeTileCount", {}).get("value", None)
            hands = round_info.get("hands", {}).get("value", None)
            pool = round_info.get("pool", {}).get("value", None)
            dora_tiles = round_info.get("dora", {}).get("value", None)
            tian_dora_tiles = round_info.get("tianDora", {}).get("value", None)
            ting_list = round_info.get("tingList", {}).get("value", None)
            next_operation = round_info.get("nextOperation", {}).get("value", None)
            locked_tiles = round_info.get("lockedTile", {}).get("value", None)
            used_desktop = round_info.get("usedDesktop", {}).get("value", None)
            point = round_info.get("point", {}).get("value", "0")
            target_point = round_info.get("targetPoint", {}).get("value", "0")
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            game = value_changes.get("game", {})
            boss_buff = game.get("bossBuff", {}).get("value", None)
            used = round_info.get("used", {}).get("value", None)
            record = value_changes.get("record", None)
            GAME_STATE.update_record(record)
            if hands and pool:
                GAME_STATE.update_pool(pool, hand_tiles=hands, locked_tiles=locked_tiles, used=used, dora_tiles=dora_tiles, used_desktop=used_desktop, push_gamestate=False)
                new_wall = reorder_wall_tiles_by_amulet221(GAME_STATE.deck_map, GAME_STATE.wall_tiles, GAME_STATE.effect_list)
                GAME_STATE.update_wall(new_wall)
                desktop_remain = round_info.get("desktopRemain", {}).get("value", 0)
                level = value_changes.get("game", {}).get("level", {}).get("value", 0)
                try:
                    level_int = int(level)
                except (TypeError, ValueError):
                    level_int = 0
                if level_int <= 503 and str(level_int)[-1] != "3":
                    boss_buff = []
                # 进入换牌阶段
                switch_stage_event = next((e for e in events if e.get("type") == 19), None)
                if switch_stage_event:
                    value_changes_19 = switch_stage_event.get("valueChanges", {})
                    stage = value_changes_19.get("stage", -1)
                    ended = value_changes_19.get("ended", False)
                    GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended, level=level, effect_list=effect_list, ting_list=ting_list, next_operation=next_operation, total_change_tile_count=total_change_tile_count, change_tile_count=change_tile_count, boss_buff=boss_buff, target_point=target_point, point=point, tian_dora_tiles=tian_dora_tiles, reason=".lq.Lobby.amuletActivityUpgrade:19")
                else:
                    GAME_STATE.update_other_info(desktop_remain=desktop_remain, level=level, effect_list=effect_list, ting_list=ting_list, next_operation=next_operation, total_change_tile_count=total_change_tile_count, change_tile_count=change_tile_count, boss_buff=boss_buff, target_point=target_point, point=point, tian_dora_tiles=tian_dora_tiles, reason=".lq.Lobby.amuletActivityUpgrade:23")
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
        matched = next((e for e in events if e.get("type") == 48), None)
        if matched:
            value_changes = matched.get("valueChanges", {})
            coin = int(value_changes.get("game", {}).get("coin", {}).get("value", None))
            GAME_STATE.update_other_info(coin=coin, reason=".lq.Lobby.amuletActivityUpgrade:48")
        matched = next((e for e in events if e.get("type") == 49), None)
        if matched:
            value_changes = matched.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(stage=stage, reason=".lq.Lobby.amuletActivityUpgrade:49")
        if modify:
            return "modify", data
    # 游戏中打牌等操作
    # MARK: amuletActivityOperate
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityOperate":
        data = view.get("data", {})
        events = data.get("events", [])
        # type = 100: 游戏结束
        end_event = next((e for e in events if e.get("type") == 100), None)
        if end_event:
            value_changes = end_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            ended = value_changes.get("ended", True)
            GAME_STATE.update_other_info(stage=stage, ended=ended, reason=".lq.Lobby.amuletActivityOperate:100")
            return "pass", None
        # type = 4: 换牌
        switch_event = next((e for e in events if e.get("type") == 4), None)
        if switch_event:
            value_changes = switch_event.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            change_tile_count = round_info.get("changeTileCount", {}).get("value", None)
            used = round_info.get("used", {}).get("value", [])
            GAME_STATE.update_switch_used_tiles(used=used, push_gamestate=False, reason=".lq.Lobby.amuletActivityOperate:4")
            hands = round_info.get("hands", {}).get("value", [])
            GAME_STATE.update_hand_tiles(hand_tiles=hands, push_gamestate=False)
            stage = value_changes.get("stage", -1)
            next_operation = round_info.get("nextOperation", {}).get("value", None)
            ting_list = round_info.get("tingList", {}).get("value", None)
            GAME_STATE.update_other_info(stage=stage, change_tile_count=change_tile_count, next_operation=next_operation, ting_list=ting_list)
        # type = 5: 跳过换牌
        skip_switch_event = next((e for e in events if e.get("type") == 5), None)
        if skip_switch_event:
            value_changes = skip_switch_event.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            tian_dora_tiles = round_info.get("tianDora", {}).get("value", None)
            GAME_STATE.update_other_info(tian_dora_tiles=tian_dora_tiles, reason=".lq.Lobby.amuletActivityOperate:5")
        # type = 6: 摸牌
        draw_event = next((e for e in events if e.get("type") == 6), None)
        if draw_event:
            value_changes = draw_event.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            desktop_remain = round_info.get("desktopRemain", {}).get("value", 0)
            stage = value_changes.get("stage", -1)
            ended = value_changes.get("ended", False)
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            ting_list = round_info.get("tingList", {}).get("value", None)
            next_operation = round_info.get("nextOperation", {}).get("value", None)
            after_draw_hands = round_info.get("hands", {}).get("value", None)
            if after_draw_hands:
                GAME_STATE.on_draw_tile(after_draw_hands, after_draw_hands[len(after_draw_hands) - 1], push_gamestate=False)

            GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended, effect_list=effect_list, ting_list=ting_list, next_operation=next_operation, reason=".lq.Lobby.amuletActivityOperate:6")

            loop = asyncio.get_running_loop()
            global _DISCARD_RECOMMENDATION_SEQ
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

        coin_event = next((e for e in events if e.get("type") == 11), None)
        if coin_event:
            value_changes = coin_event.get("valueChanges", {})
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            coin = int(value_changes.get("game", {}).get("coin", {}).get("value", None))
            GAME_STATE.update_other_info(coin=coin, effect_list=effect_list, reason=".lq.Lobby.amuletActivityOperate:11")
        shop_event = next((e for e in events if e.get("type") == 12), None)
        if shop_event:
            value_changes = shop_event.get("valueChanges", {})
            shop = value_changes.get("shop", {})
            goods = shop.get("goods", {}).get("value", None)
            refresh_price = shop.get("refreshPrice", {}).get("value", None)
            GAME_STATE.update_other_info(goods=goods, refresh_price=refresh_price, reason=".lq.Lobby.amuletActivityOperate:12")
        reward_pack_event = next((e for e in events if e.get("type") == 15), None)
        if reward_pack_event:
            value_changes = reward_pack_event.get("valueChanges", {})
            effect = value_changes.get("effect", {})
            level_reward_candidates = effect.get("levelRewardCandidates", {}).get("value", None)
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(candidate_effect_list=level_reward_candidates, stage=stage, reason=".lq.Lobby.amuletActivityOperate:15")
        finish_event = next((e for e in events if e.get("type") == 24), None)
        if finish_event:
            value_changes = finish_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(stage=stage, reason=".lq.Lobby.amuletActivityOperate:24")
        _handle_amulet_activity_operate_type8_events(events)
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
            locked_tiles = round_info.get("lockedTile", [])
            effect_list = game.get("effect", {}).get("effectList", None)
            total_chance_tile_count = round_info.get("totalChangeTileCount", None)
            chance_tile_count = round_info.get("changeTileCount", None)
            used = round_info.get("used", [])
            used_desktop = round_info.get("usedDesktop", [])
            GAME_STATE.update_pool(pool, hand_tiles=hands, locked_tiles=locked_tiles, push_gamestate=False, used=used, dora_tiles=dora_tiles, used_desktop=used_desktop, reason=".lq.Lobby.fetchAmuletActivityData")
            desktop_remain = round_info.get("desktopRemain", 0)
            point = round_info.get("point", "0")
            target_point = round_info.get("targetPoint", "0")
            stage = game.get("stage", -1)
            ended = game.get("ended", False)
            coin = int(game.get("game", {}).get("coin", ""))
            boss_buff = game.get("game", {}).get("bossBuff", None)
            tile_score_map_jsonarray = game.get("game", {}).get("tileScoreMap", None)
            tile_score_map = None
            if tile_score_map_jsonarray:
                tile_score_map = {
                    str(item.get("tile", "")): str(item.get("score", ""))
                    for item in tile_score_map_jsonarray
                    if item.get("tile") is not None and item.get("score") is not None
                }
            level = game.get("game", {}).get("level", 0)
            if boss_buff and level:
                try:
                    level_int = int(level)
                except (TypeError, ValueError):
                    level_int = 0
                if level_int <= 503 and str(level_int)[-1] != "3":
                    boss_buff = []
            shop = game.get("shop", {})
            free_candidate_effect_list = game.get("effect", {}).get("freeRewardCandidates", None)
            max_effect_volume = game.get("effect", {}).get("maxEffectVolume", 0)
            candidate_effect_list = shop.get("candidateEffectList", [])
            if free_candidate_effect_list:
                candidate_effect_list = free_candidate_effect_list
            goods = shop.get("goods", [])
            refresh_price = shop.get("refreshPrice", 0)
            record = game.get("record", None)
            ting_list = round_info.get("tingList", None)
            next_operation = round_info.get("nextOperation", None)
            GAME_STATE.update_record(record)
            if desktop_remain < 36:
                new_wall = reorder_wall_tiles_by_amulet221(GAME_STATE.deck_map, GAME_STATE.wall_tiles, effect_list)
                GAME_STATE.update_wall(new_wall)
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended, level=level, effect_list=effect_list, candidate_effect_list=candidate_effect_list, coin=coin, ting_list=ting_list, next_operation=next_operation, goods=goods, refresh_price=refresh_price, total_change_tile_count=total_chance_tile_count, change_tile_count=chance_tile_count, max_effect_volume=max_effect_volume, boss_buff=boss_buff, tile_score_map=tile_score_map, target_point=target_point, point=point, tian_dora_tiles=tian_dora_tiles, push_gamestate=False)
                GAME_STATE.refresh_wall_by_remaning()
            else:
                new_wall = reorder_wall_tiles_by_amulet221(GAME_STATE.deck_map, GAME_STATE.wall_tiles, effect_list)
                GAME_STATE.update_wall(new_wall)
                GAME_STATE.update_other_info(desktop_remain=desktop_remain, stage=stage, ended=ended, level=level, effect_list=effect_list, candidate_effect_list=candidate_effect_list, coin=coin, ting_list=ting_list, next_operation=next_operation, goods=goods, refresh_price=refresh_price, total_change_tile_count=total_chance_tile_count, change_tile_count=chance_tile_count, max_effect_volume=max_effect_volume, boss_buff=boss_buff, tile_score_map=tile_score_map, target_point=target_point, point=point, tian_dora_tiles=tian_dora_tiles, push_gamestate=True)
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
    # MARK: amuletActivitySelectFreeEffect
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivitySelectFreeEffect":
        data = view.get("data", {})
        events = data.get("events", [])
        start_event = next((e for e in events if e.get("type") == 2), None)
        value_changes = start_event.get("valueChanges", {})
        stage = value_changes.get("stage", -1)
        effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
        ended = value_changes.get("ended", False)
        record = value_changes.get("record", None)
        GAME_STATE.update_record(record)
        GAME_STATE.update_other_info(stage=stage, ended=ended, effect_list=effect_list, reason=".lq.Lobby.amuletActivitySelectFreeEffect:2")
    # MARK: amuletActivityStartGame
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityStartGame":
        data = view.get("data", {})
        events = data.get("events", [])
        start_event = next((e for e in events if e.get("type") == 1), None)
        result = start_event.get("result", {}).get("newGameResult", {})
        stage = result.get("stage", -1)
        ended = result.get("ended", False)
        record = result.get("record", None)
        effect = result.get("effect", None)
        free_candidate_effect_list = effect.get("freeRewardCandidates", [])
        max_effect_volume = effect.get("maxEffectVolume", None)
        GAME_STATE.update_record(record)
        GAME_STATE.update_other_info(stage=stage, ended=ended, candidate_effect_list=free_candidate_effect_list, max_effect_volume=max_effect_volume, reason=".lq.Lobby.amuletActivityStartGame:1")
    # MARK: amuletActivityBuy
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityBuy":
        data = dict(view["data"])
        events = data.get("events", [])
        buy_amulet_event = next((e for e in events if e.get("type") == 13), None)
        if buy_amulet_event:
            value_changes = buy_amulet_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            game = value_changes.get("game", {})
            ended = value_changes.get("ended", False)
            coin = int(game.get("coin", {}).get("value", None))
            shop = value_changes.get("shop", {})
            goods = shop.get("goods", {}).get("value", None)
            record = value_changes.get("record", None)
            GAME_STATE.update_record(record)
            candidate_effect_list = shop.get("candidateEffectList", {}).get("value", None)
            GAME_STATE.update_other_info(stage=stage, coin=coin, ended=ended, candidate_effect_list=candidate_effect_list, goods=goods, reason=".lq.Lobby.amuletActivityBuy:13")
    # MARK: amuletActivitySelectPack
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivitySelectPack":
        data = view.get("data", {})
        events = data.get("events", [])
        select_amulet_event = next((e for e in events if e.get("type") == 14), None)
        if select_amulet_event:
            value_changes = select_amulet_event.get("valueChanges", {})
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            stage = value_changes.get("stage", -1)
            record = value_changes.get("record", None)
            GAME_STATE.update_record(record)
            GAME_STATE.update_other_info(stage=stage, effect_list=effect_list, reason=".lq.Lobby.amuletActivitySelectPack:14")
    # MARK: amuletActivitySellEffect
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivitySellEffect":
        data = dict(view["data"])
        events = data.get("events", [])
        sell_amulet_event = next((e for e in events if e.get("type") == 17), None)
        if sell_amulet_event:
            value_changes = sell_amulet_event.get("valueChanges", {})
            game = value_changes.get("game", {})
            stage = value_changes.get("stage", -1)
            coin = int(game.get("coin", {}).get("value", None))
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            ended = value_changes.get("ended", False)
            record = value_changes.get("record", None)
            shop = value_changes.get("shop", {})
            goods = shop.get("goods", {}).get("value", None)
            GAME_STATE.update_record(record)
            GAME_STATE.update_other_info(stage=stage, coin=coin, ended=ended, effect_list=effect_list, goods=goods, reason=".lq.Lobby.amuletActivitySellEffect:17")
    # MARK: amuletActivityRefreshShop
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityRefreshShop":
        data = dict(view["data"])
        events = data.get("events", [])
        refresh_shop_event = next((e for e in events if e.get("type") == 18), None)
        if refresh_shop_event:
            value_changes = refresh_shop_event.get("valueChanges", {})
            game = value_changes.get("game", {})
            stage = value_changes.get("stage", -1)
            coin = int(game.get("coin", {}).get("value", None))
            record = value_changes.get("record", None)
            shop = value_changes.get("shop", {})
            goods = shop.get("goods", {}).get("value", None)
            refresh_price = shop.get("refreshPrice", {}).get("value", None)
            GAME_STATE.update_record(record)
            GAME_STATE.update_other_info(stage=stage, coin=coin, goods=goods, refresh_price=refresh_price, reason=".lq.Lobby.amuletActivitySellEffect:18")
    # MARK: amuletActivityEndShopping
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityEndShopping":
        data = view.get("data", {})
        events = data.get("events", [])
        end_shopping_event = next((e for e in events if e.get("type") == 22), None)
        if end_shopping_event:
            value_changes = end_shopping_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            GAME_STATE.update_other_info(stage=stage, reason=".lq.Lobby.amuletActivityEndShopping:22")
    # MARK: amuletActivityEffectSort
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityEffectSort":
        data = view.get("data", {})
        events = data.get("events", [])
        amulet_sort_event = next((e for e in events if e.get("type") == 20), None)
        if amulet_sort_event:
            value_changes = amulet_sort_event.get("valueChanges", {})
            effect_list = value_changes.get("effect", {}).get("effectList", {}).get("value", None)
            GAME_STATE.update_other_info(effect_list=effect_list, reason=".lq.Lobby.amuletActivityEffectSort:20")
    # MARK: amuletActivitySelectRewardPack
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivitySelectRewardPack":
        data = view.get("data", {})
        events = data.get("events", [])
        select_reward_event = next((e for e in events if e.get("type") == 16), None)
        if select_reward_event:
            value_changes = select_reward_event.get("valueChanges", {})
            effect = value_changes.get("effect", {})
            effect_list = effect.get("effectList", {}).get("value", None)
            level_reward_candidates = effect.get("levelRewardCandidates", {}).get("value", None)
            GAME_STATE.update_other_info(effect_list=effect_list, candidate_effect_list=level_reward_candidates, reason=".lq.Lobby.amuletActivitySelectRewardPack:16")
        shop_event = next((e for e in events if e.get("type") == 12), None)
        if shop_event:
            value_changes = shop_event.get("valueChanges", {})
            stage = value_changes.get("stage", -1)
            shop = value_changes.get("shop", {})
            goods = shop.get("goods", {}).get("value", None)
            refresh_price = shop.get("refreshPrice", {}).get("value", None)
            ended = shop_event.get("ended", False)
            GAME_STATE.update_other_info(stage=stage, goods=goods, refresh_price=refresh_price, ended=ended, reason=".lq.Lobby.amuletActivitySelectRewardPack:12")
    # MARK: amuletActivityUpgradeShopBuff
    if view["type"] == "Res" and view["method"] == ".lq.Lobby.amuletActivityUpgradeShopBuff":
        data = view.get("data", {})
        events = data.get("events", [])
        upgrade_shop_buff = next((e for e in events if e.get("type") == 21), None)
        if upgrade_shop_buff:
            value_changes = upgrade_shop_buff.get("valueChanges", {})
            round_info = value_changes.get("round", {})
            tian_dora_tiles = round_info.get("tianDora", {}).get("value", None)
            game = value_changes.get("game", {})
            coin = int(game.get("coin", {}).get("value", None))
            record = value_changes.get("record", None)
            GAME_STATE.update_record(record)
            GAME_STATE.update_other_info(coin=coin, tian_dora_tiles=tian_dora_tiles, reason=".lq.Lobby.amuletActivityUpgradeShopBuff:21")
    return "pass", None


def _handle_amulet_activity_operate_type8_events(events: List[Dict[str, Any]]) -> None:
    type8_events = [event for event in events if event.get("type") == 8]
    if not type8_events:
        return

    current_point = GAME_STATE.point
    tile_score_map = None
    for index, event in enumerate(type8_events):
        is_last_event = index == len(type8_events) - 1
        value_changes = event.get("valueChanges", None)

        if value_changes:
            if is_last_event:
                tile_score_map_jsonarray = value_changes.get("game", {}).get("tileScoreMap", {}).get("value", None)
                if tile_score_map_jsonarray:
                    tile_score_map = {
                        str(item.get("tile", "")): str(item.get("score", ""))
                        for item in tile_score_map_jsonarray
                        if item.get("tile") is not None and item.get("score") is not None
                    }
                point = value_changes.get("round", {}).get("point", {}).get("value", "0")
                current_point = point
    GAME_STATE.update_other_info(point=current_point, tile_score_map=tile_score_map, push_gamestate=True, reason=".lq.Lobby.amuletActivityOperate:8")


def has_amulet_221(effects: List[Dict[str, Any]]) -> bool:
    for e in effects or []:
        try:
            eid = int(e.get("id", -1))
        except Exception:
            continue
        if eid // 10 == 221:
            return True
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
