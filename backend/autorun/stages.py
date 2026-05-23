from __future__ import annotations

import asyncio
from typing import Any, Dict, List, Optional

from loguru import logger

from backend.autorun.actions import AutoRunActions
from backend.autorun.strategy import (
    CONDUCTION_BADGE_ID,
    HAPPINESS_BADGE_ID,
    WHEEL_AMULET_REG_ID,
    AutoRunStrategyContext,
    select_items_to_sell_for_purchase,
)
from backend.autorun.util.suannkou_recommender import plan_pure_pinzu_suu_ankou_v2
from backend.autorun.value import extract_amulet_signature, is_needed_for_any_target, reg_id_of_raw, total_volume


def _extract_new_amulets_from_select_resp(resp: Optional[dict]) -> List[Dict[str, Any]]:
    result: List[Dict[str, Any]] = []
    if not isinstance(resp, dict):
        return result

    data = resp.get("data")
    if isinstance(data, dict):
        events = data.get("events")
    else:
        events = resp.get("events", resp.get("event"))
    if not isinstance(events, list):
        return result

    for event in events:
        if not isinstance(event, dict):
            continue
        try:
            event_type = int(event.get("type", -1))
        except Exception:
            continue
        if event_type != 14:
            continue

        hooks = event.get("effectedHooks")
        if not isinstance(hooks, list):
            continue
        for hook in hooks:
            if not isinstance(hook, dict):
                continue
            try:
                hook_id = int(hook.get("id", -1))
            except Exception:
                continue
            if hook_id != 1631:
                continue

            hook_result = hook.get("result")
            if not isinstance(hook_result, dict):
                continue
            transform_effects = hook_result.get("transformEffect")
            if not isinstance(transform_effects, list):
                continue
            for transform in transform_effects:
                if not isinstance(transform, dict):
                    continue
                add_result = transform.get("addResult")
                if isinstance(add_result, dict):
                    result.append(add_result)
    return result


def _strategy_context(runner: Any, game_state: Any) -> AutoRunStrategyContext:
    return AutoRunStrategyContext(
        game_state=game_state,
        targets=runner.targets,
        need_pionner_badge_count=runner.need_pionner_badge_count,
        amulet_registry=runner._get_amulet_registry(),
    )


def _happiness_victim_uid(effect_list: List[Dict[str, Any]], targets: List[Dict[str, Any]]) -> Optional[int]:
    for item in effect_list or []:
        _reg_id, _is_plus, badge_id = extract_amulet_signature(item)
        if badge_id != HAPPINESS_BADGE_ID or is_needed_for_any_target(item, targets):
            continue
        uid = item.get("uid")
        if uid is None:
            continue
        try:
            return int(uid)
        except Exception:
            return None
    return None


async def _sell_happiness_after_refresh(actions: AutoRunActions, runner: Any, game_state: Any) -> None:
    victim_uid = _happiness_victim_uid(game_state.effect_list or [], runner.targets)
    if victim_uid is None:
        return
    await actions.set_step("game.sell_happiness_after_refresh")
    await actions.run_or_abort(
        actions.bot.sell_effect,
        uid=victim_uid,
        log_operation=True,
        action="sell_happiness_after_refresh",
        op_reason="refresh_shop_success_sell_non_target_happiness_badge",
        op_details={"sell_uid": victim_uid},
    )


async def _sort_if_needed(
        actions: AutoRunActions,
        runner: Any,
        game_state: Any,
        *,
        mode: str,
        step: str,
) -> None:
    try:
        uids = runner.strategy.sort_uids(_strategy_context(runner, game_state), mode)
        if not uids:
            return
        await actions.set_step(step)
        ok, reason, resp = await actions.bot_call(
            actions.bot.sort_effect,
            sorted_uid=uids,
            delay_sec=2.0,
            interval=0.3,
            timeout=10,
        )
        if not ok:
            logger.warning("{} sort_effect failed: {}, resp: {}", mode, reason, resp)
    except Exception as exc:
        logger.warning("{} sort attempt error: {}", mode, exc)


async def _handle_start_game(actions: AutoRunActions, runner: Any) -> None:
    await actions.set_step("start_game")
    ok, _resp = await actions.run_or_abort(
        actions.bot.start_game,
        action="start_game",
        op_reason="start_next_run",
        op_details={"next_run_index": int(runner.runs or 0) + 1},
    )
    if ok:
        runner.runs += 1
        runner.need_start_game = False


async def _handle_stage_1(actions: AutoRunActions, game_state: Any) -> None:
    await actions.set_step("game.select_free_effect")
    first_effect = game_state.candidate_effect_list[0].get("id")
    await actions.run_or_abort(
        actions.bot.select_free_effect,
        selected_id=first_effect,
        log_operation=True,
        action="select_free_effect",
        op_reason="free_effect_auto_select_first_candidate",
        op_details={"selected_id": first_effect, "candidate_effect_list": game_state.candidate_effect_list},
    )


async def _handle_stage_2(actions: AutoRunActions, game_state: Any) -> None:
    await actions.set_step(f"game.change_tile({game_state.change_tile_count}/{game_state.total_change_tile_count})")
    if game_state.change_tile_count >= game_state.total_change_tile_count:
        await actions.run_or_abort(
            actions.bot.op_skip_change,
            action="skip_change_tile",
            op_reason="change_tile_count_reached_limit",
            op_details={
                "change_tile_count": game_state.change_tile_count,
                "total_change_tile_count": game_state.total_change_tile_count,
            },
        )
        return

    prefer_keep = [
        tile_id
        for tile_id in game_state.hand_tiles
        if (face := game_state.deck_map.get(tile_id)) is not None and (face == "bd" or face.endswith("p"))
    ]
    if 901 in (game_state.boss_buff or []):
        keep_target = max(0, len(game_state.hand_tiles) - 3)
        if len(prefer_keep) >= keep_target:
            filtered_ids = prefer_keep[:keep_target]
        else:
            rest = [tile_id for tile_id in game_state.hand_tiles if tile_id not in prefer_keep]
            filtered_ids = prefer_keep + rest[:max(0, keep_target - len(prefer_keep))]
    else:
        filtered_ids = prefer_keep
    await actions.run_or_abort(
        actions.bot.op_change,
        tile_ids=filtered_ids,
        action="change_tile",
        op_reason="keep_pinzu_and_red_dora_tiles",
        op_details={
            "kept_tile_ids": filtered_ids,
            "prefer_keep": prefer_keep,
            "boss_buff": game_state.boss_buff,
        },
    )


async def _handle_stage_3(actions: AutoRunActions, runner: Any, game_state: Any) -> None:
    await actions.set_step(f"game.discard({game_state.level})")
    suuannkou = await asyncio.to_thread(
        plan_pure_pinzu_suu_ankou_v2,
        game_state.hand_tiles,
        game_state.wall_tiles,
        game_state.deck_map,
    )

    status = suuannkou.get("status")
    if status == "impossible":
        if runner.strategy.should_remake(_strategy_context(runner, game_state), "impossible"):
            await actions.remake("impossible", game_state)
        return

    if status == "win_now":
        await _sort_if_needed(actions, runner, game_state, mode="pre_win", step="game.pre_win_sort")
        await actions.set_step("game.tsumo")
        await actions.run_or_abort(
            actions.bot.op_tsumo,
            action="tsumo",
            op_reason="planner_reports_win_now",
            op_details={"plan_status": status},
        )
        return

    if status == "plan":
        discard = suuannkou["discards"][0]
        await actions.run_or_abort(
            actions.bot.discard_by_tile_id,
            tile_id=discard,
            action="discard",
            op_reason="planner_selected_discard",
            op_details={"discard_tile_id": discard, "plan": suuannkou},
        )


async def _end_or_remake_when_shop_blocked(
        actions: AutoRunActions,
        runner: Any,
        game_state: Any,
        reason: str,
) -> None:
    if runner.cutoff_level <= game_state.level and runner.strategy.should_remake(_strategy_context(runner, game_state), reason):
        await actions.remake(reason, game_state)
        return
    await actions.set_step("game.end_shopping")
    await actions.run_or_abort(
        actions.bot.end_shopping,
        action="end_shopping",
        op_reason=f"{reason}_but_before_cutoff_or_strategy_declined_remake",
        op_details={"cutoff_level": runner.cutoff_level, "level": game_state.level},
    )


async def _refresh_shop(actions: AutoRunActions, runner: Any, game_state: Any) -> None:
    await actions.set_step("game.refresh_shop")
    ok, _resp = await actions.run_or_abort(
        actions.bot.refresh_shop,
        action="refresh_shop",
        op_reason="shop_has_no_affordable_or_available_pack",
        op_details={"coin": game_state.coin, "refresh_price": game_state.refresh_price},
    )
    if ok:
        await _sell_happiness_after_refresh(actions, runner, game_state)


async def _handle_stage_4(actions: AutoRunActions, runner: Any, game_state: Any) -> None:
    await actions.set_step("game.buy_pack")
    candidates = [good for good in game_state.goods if not good.get("sold", False)]
    context = _strategy_context(runner, game_state)

    if not candidates:
        if game_state.refresh_price > game_state.coin:
            await _end_or_remake_when_shop_blocked(actions, runner, game_state, "cutoff_no_shop_options")
            return
        if runner.strategy.should_refresh_shop(context):
            await _refresh_shop(actions, runner, game_state)
        else:
            pass
        return

    candidates.sort(key=lambda good: (
        int(good.get("price", 1_000_000)),
        int(good.get("goodsId", 1_000_000)),
        int(good.get("id", 1_000_000)),
    ))
    cheapest = candidates[0]
    if cheapest["price"] > game_state.coin:
        if game_state.refresh_price > game_state.coin:
            await _end_or_remake_when_shop_blocked(actions, runner, game_state, "cutoff_cannot_afford")
            return
        if runner.strategy.should_refresh_shop(context):
            await _refresh_shop(actions, runner, game_state)
        else:
            pass
        return

    await actions.run_or_abort(
        actions.bot.buy_pack,
        good_id=cheapest["id"],
        retry_2691_refresh=True,
        action="buy_pack",
        op_reason="buy_cheapest_affordable_pack",
        op_details={"selected_good": cheapest, "coin": game_state.coin},
    )


async def _sell_new_useless_amulets_from_resp(
        actions: AutoRunActions,
        runner: Any,
        game_state: Any,
        resp: Optional[dict],
        effect_list_before_select: List[Dict[str, Any]],
) -> bool:
    new_items = _extract_new_amulets_from_select_resp(resp)
    if not new_items:
        return True

    current_effect_list: List[Dict[str, Any]] = [dict(item) for item in (effect_list_before_select or [])]
    for item in new_items:
        current_effect_list.append(dict(item))
        class _State:
            pass

        state = _State()
        state.candidate_effect_list = [{"id": int(item.get("id", 0) or 0), "badgeId": (item.get("badge") or {}).get("id", 0) if isinstance(item.get("badge"), dict) else 0}]
        state.effect_list = effect_list_before_select
        decision = runner.strategy.choose_candidate(_strategy_context(runner, state))
        if decision.selection_value > 0:
            runner.record_operation(
                "keep_new_selected_effect",
                reason=decision.reason,
                result="kept",
                details={"new_effect": item, "decision": decision.__dict__},
                game_state=game_state,
            )
            continue

        reg_id = reg_id_of_raw(int(item.get("id", 0) or 0))
        if reg_id == WHEEL_AMULET_REG_ID:
            runner.record_operation(
                "keep_new_selected_effect",
                reason="wheel_amulet_never_auto_sell",
                result="kept",
                details={"new_effect": item},
                game_state=game_state,
            )
            continue

        state.effect_list = current_effect_list
        uid = runner.strategy.choose_same_reg_sell_uid(_strategy_context(runner, state), reg_id)
        logger.info(
            "[autorun] auto-sell useless selected amulet by same-reg rule: raw_id={} sell_uid={} badge={}",
            item.get("id"),
            uid,
            (item.get("badge") or {}).get("id") if isinstance(item.get("badge"), dict) else None,
        )
        if uid is None:
            continue

        current_effect_list = [effect for effect in current_effect_list if effect.get("uid") != uid]
        await actions.set_step("game.sell_new_useless_effect")
        ok, reason, _resp = await actions.bot_call(actions.bot.sell_effect, uid=uid)
        runner.record_operation(
            "sell_new_useless_effect",
            reason="selected_effect_has_no_strategy_value_same_reg_cleanup",
            result="ok" if ok else reason,
            details={"new_effect": item, "sell_uid": uid},
            game_state=game_state,
        )
        if ok:
            continue
        if reason == "error code: 2699":
            actions.bot.fetch_amulet_activity_data()
            return True
        await actions.abort(reason)
        return False
    return True


async def _select_stage_candidate(actions: AutoRunActions, game_state: Any, raw_id: Optional[int]) -> tuple[bool, Optional[dict]]:
    if game_state.stage == 5:
        return await actions.run_or_abort(
            actions.bot.select_effect,
            selected_id=raw_id,
            retry_2691_refresh=True,
            log_operation=True,
            action="select_effect",
            op_reason="selected_by_strategy",
            op_details={"selected_id": raw_id},
        )
    return await actions.run_or_abort(
        actions.bot.select_reward_effect,
        selected_id=raw_id,
        retry_2691_refresh=True,
        log_operation=True,
        action="select_reward_effect",
        op_reason="selected_by_strategy",
        op_details={"selected_id": raw_id},
    )


async def _skip_stage_candidate(actions: AutoRunActions, game_state: Any) -> None:
    if game_state.stage == 5:
        await actions.run_or_abort(
            actions.bot.select_effect,
            selected_id=0,
            log_operation=True,
            action="skip_select_effect",
            op_reason="no_candidate_should_be_selected",
        )
    else:
        await actions.run_or_abort(
            actions.bot.select_reward_effect,
            selected_id=0,
            log_operation=True,
            action="skip_select_reward_effect",
            op_reason="no_candidate_should_be_selected",
        )


async def _handle_stage_5_or_7(actions: AutoRunActions, runner: Any, game_state: Any) -> None:
    await actions.set_step("game.select_effect" if game_state.stage == 5 else "game.select_reward_effect")
    context = _strategy_context(runner, game_state)
    decision = runner.strategy.choose_candidate(context)
    runner.record_operation(
        "choose_candidate",
        reason=decision.reason,
        result="selected" if decision.raw_id is not None else "skipped",
        details={
            "selected_raw_id": decision.raw_id,
            "selected_badge_id": decision.badge_id,
            "selection_value": decision.selection_value,
            "sell_uid": decision.sell_uid,
            "considered": decision.considered,
        },
        game_state=game_state,
    )
    if decision.raw_id is None:
        await _skip_stage_candidate(actions, game_state)
        return

    if decision.sell_uid:
        ok, _resp = await actions.run_or_abort(
            actions.bot.sell_effect,
            uid=decision.sell_uid,
            log_operation=True,
            action="sell_before_select_candidate",
            op_reason="strategy_requested_same_reg_cleanup_before_selection",
            op_details={"sell_uid": decision.sell_uid, "decision": decision.__dict__},
        )
        if not ok:
            return

    need_space = 2 if decision.badge_id == CONDUCTION_BADGE_ID else 1
    used_space = total_volume(game_state.effect_list)
    free_space = game_state.max_effect_volume - used_space

    if free_space >= need_space:
        effect_list_before_select = [dict(item) for item in (game_state.effect_list or [])]
        ok, resp = await _select_stage_candidate(actions, game_state, decision.raw_id)
        if not ok:
            return
        if not await _sell_new_useless_amulets_from_resp(actions, runner, game_state, resp, effect_list_before_select):
            return
        if decision.selection_value == 0:
            reg_id = reg_id_of_raw(decision.raw_id)
            if reg_id == WHEEL_AMULET_REG_ID:
                return
            uid = runner.strategy.choose_same_reg_sell_uid(context, reg_id)
            if uid:
                await actions.set_step("game.sell_useless_effect")
                await actions.run_or_abort(
                    actions.bot.sell_effect,
                    uid=uid,
                    retry_2699_refresh=True,
                    log_operation=True,
                    action="sell_useless_effect",
                    op_reason="selected_candidate_has_no_strategy_value_same_reg_cleanup",
                    op_details={"selected_raw_id": decision.raw_id, "sell_uid": uid},
                )
        return

    if decision.selection_value >= 99:
        sell_list = [candidate.item for candidate in runner.strategy.rank_sell_candidates(context)]
        to_sell, _freed, enough = select_items_to_sell_for_purchase(
            free_space=free_space,
            need_space=need_space,
            sell_candidates=sell_list,
        )
        if enough:
            runner.record_operation(
                "make_space_decision",
                reason="target_candidate_selected_but_space_insufficient",
                result="sell_to_make_space",
                details={
                    "selected_raw_id": decision.raw_id,
                    "need_space": need_space,
                    "free_space": free_space,
                    "to_sell": to_sell,
                },
                game_state=game_state,
            )
            for item in to_sell:
                uid = item.get("uid")
                if uid is None:
                    continue
                await actions.set_step("game.selling_to_make_space")
                ok, _resp = await actions.run_or_abort(
                    actions.bot.sell_effect,
                    uid=uid,
                    interval=0.6,
                    log_operation=True,
                    action="sell_to_make_space",
                    op_reason="target_candidate_requires_more_space",
                    op_details={"selected_raw_id": decision.raw_id, "sell_item": item},
                )
                if not ok:
                    return
        else:
            await actions.set_step("game.skip_buy_insufficient_space0")
            runner.record_operation(
                "skip_candidate",
                reason="target_candidate_space_cannot_be_freed",
                result="skipped",
                details={
                    "selected_raw_id": decision.raw_id,
                    "need_space": need_space,
                    "free_space": free_space,
                    "sell_candidates": sell_list,
                },
                game_state=game_state,
            )
            await _skip_stage_candidate(actions, game_state)
        return

    logger.debug(
        "not enough space to buy 1: max: {}, used: {}, free: {}",
        game_state.max_effect_volume,
        used_space,
        free_space,
    )
    await actions.set_step("game.skip_buy_insufficient_space1")
    runner.record_operation(
        "skip_candidate",
        reason="candidate_not_important_and_space_insufficient",
        result="skipped",
        details={
            "selected_raw_id": decision.raw_id,
            "selection_value": decision.selection_value,
            "need_space": need_space,
            "free_space": free_space,
        },
        game_state=game_state,
    )
    await _skip_stage_candidate(actions, game_state)


async def _handle_stage_6(actions: AutoRunActions, runner: Any, game_state: Any) -> None:
    await actions.set_step("game.level_confirm")
    await _sort_if_needed(actions, runner, game_state, mode="pre_start", step="game.pre_start_sort")
    await actions.run_or_abort(
        actions.bot.next_level,
        action="next_level",
        op_reason="level_confirmed",
        op_details={"level": game_state.level},
    )


async def handle_autorun_tick(runner: Any, bot: Any, game_state: Any) -> None:
    actions = AutoRunActions(runner, bot)
    if await runner._check_and_finish_if_done():
        return
    if runner.need_start_game:
        await _handle_start_game(actions, runner)
        return

    if game_state.stage == 1:
        await _handle_stage_1(actions, game_state)
    elif game_state.stage == 2:
        await _handle_stage_2(actions, game_state)
    elif game_state.stage == 3:
        await _handle_stage_3(actions, runner, game_state)
    elif game_state.stage == 4:
        await _handle_stage_4(actions, runner, game_state)
    elif game_state.stage in (5, 7):
        await _handle_stage_5_or_7(actions, runner, game_state)
    elif game_state.stage == 6:
        await _handle_stage_6(actions, runner, game_state)
