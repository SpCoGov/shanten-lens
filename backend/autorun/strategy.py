from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple

from backend.autorun.value import (
    calc_candidate_price,
    calc_effect_price,
    candidate_badge_id,
    extract_amulet_signature,
    is_needed_for_any_target,
    match_targets_for_amulet,
    reg_id_of_raw,
)


NEED_PIONNER_BADGE_COUNT = 4
PIONNER_BADGE_ID = 600070
HAPPINESS_BADGE_ID = 600110
CONDUCTION_BADGE_ID = 600160
WHEEL_AMULET_REG_ID = 146


@dataclass
class CandidateDecision:
    raw_id: Optional[int]
    badge_id: Optional[int]
    selection_value: int
    sell_uid: Optional[int] = None
    reason: str = ""
    considered: List[Dict[str, Any]] = field(default_factory=list)


@dataclass
class SellCandidate:
    item: Dict[str, Any]
    sell_priority: int


@dataclass
class AutoRunStrategyContext:
    game_state: Any
    targets: List[Dict[str, Any]]
    need_pionner_badge_count: int = NEED_PIONNER_BADGE_COUNT
    amulet_registry: Any = None


@dataclass
class DecisionTraceEntry:
    name: str
    inputs: Dict[str, Any] = field(default_factory=dict)
    result: Dict[str, Any] = field(default_factory=dict)


def _owned_count_with_badge(effect_list: List[Dict[str, Any]], want_badge: int) -> int:
    count = 0
    for effect_item in effect_list or []:
        _reg_id, _is_plus, badge_id = extract_amulet_signature(effect_item)
        if badge_id == int(want_badge):
            count += 1
    return count


def _required_nonplus_badges_for_reg(targets: List[Dict[str, Any]], reg_id: int) -> set[int]:
    required: set[int] = set()
    for target in targets or []:
        if target.get("kind") != "amulet":
            continue
        try:
            target_reg = int(target.get("id"))
        except Exception:
            continue
        if target_reg != reg_id or bool(target.get("plus", False)):
            continue
        target_badge = target.get("badge", None)
        if target_badge in (None, ""):
            continue
        try:
            required.add(int(target_badge))
        except Exception:
            pass
    return required


def _find_owned_uid_for_reg(effect_list: List[Dict[str, Any]], reg_id: int) -> Optional[int]:
    for effect_item in effect_list or []:
        try:
            raw_id = int(effect_item.get("id", 0))
            if raw_id // 10 != reg_id:
                continue
            uid = effect_item.get("uid")
            return int(uid) if uid is not None else None
        except Exception:
            continue
    return None


def select_items_to_sell_for_purchase(
        free_space: int,
        need_space: int,
        sell_candidates: List[Dict[str, Any]],
) -> Tuple[List[Dict[str, Any]], int, bool]:
    if need_space <= free_space:
        return [], 0, True

    gap = need_space - max(0, free_space)
    chosen: List[Dict[str, Any]] = []
    freed = 0
    for item in sell_candidates:
        volume = int(item.get("volume", 0) or 0)
        if volume <= 0:
            continue
        chosen.append(item)
        freed += volume
        if freed >= gap:
            return chosen, freed, True
    return chosen, freed, False


class AutoRunStrategy:
    def __init__(self) -> None:
        self.decision_trace: List[DecisionTraceEntry] = []

    def clear_trace(self) -> None:
        self.decision_trace.clear()

    def trace_decision(self, name: str, inputs: Dict[str, Any], result: Dict[str, Any]) -> None:
        self.decision_trace.append(DecisionTraceEntry(name=name, inputs=dict(inputs), result=dict(result)))
        if len(self.decision_trace) > 100:
            del self.decision_trace[:-100]

    def choose_candidate(self, context: AutoRunStrategyContext) -> CandidateDecision:
        raise NotImplementedError

    def rank_sell_candidates(self, context: AutoRunStrategyContext) -> List[SellCandidate]:
        raise NotImplementedError

    def choose_same_reg_sell_uid(self, context: AutoRunStrategyContext, reg_id: int) -> Optional[int]:
        raise NotImplementedError

    def should_refresh_shop(self, context: AutoRunStrategyContext) -> bool:
        return True

    def should_remake(self, context: AutoRunStrategyContext, reason: str) -> bool:
        return True

    def sort_uids(self, context: AutoRunStrategyContext, mode: str) -> Optional[List[int]]:
        return None


class DefaultAutoRunStrategy(AutoRunStrategy):
    def _candidate_considered(
            self,
            candidate_effect_list: List[Dict[str, Any]],
            context: AutoRunStrategyContext,
    ) -> List[Dict[str, Any]]:
        considered: List[Dict[str, Any]] = []
        for candidate in candidate_effect_list:
            try:
                raw_id = int(candidate.get("id", 0) or 0)
            except Exception:
                raw_id = 0
            badge_id = candidate_badge_id(candidate)
            considered.append({
                "raw_id": raw_id,
                "reg_id": reg_id_of_raw(raw_id) if raw_id > 0 else 0,
                "badge_id": badge_id,
                "price": calc_candidate_price(candidate, amulet_registry=context.amulet_registry),
                "selection_value": 0,
                "selected": False,
                "reason": "not_evaluated",
            })
        return considered

    @staticmethod
    def _candidate_item(considered: List[Dict[str, Any]], raw_id: int) -> Optional[Dict[str, Any]]:
        return next((item for item in considered if item.get("raw_id") == raw_id), None)

    @staticmethod
    def _candidate_effect_item(raw_id: int, badge_id: Optional[int]) -> Dict[str, Any]:
        item: Dict[str, Any] = {"id": raw_id}
        if badge_id is not None:
            item["badge"] = {"id": badge_id}
        return item

    def _candidate_target_score(
            self,
            raw_id: int,
            badge_id: Optional[int],
            targets: List[Dict[str, Any]],
    ) -> Tuple[int, str]:
        effect_item = self._candidate_effect_item(raw_id, badge_id)
        matched_indexes = match_targets_for_amulet(effect_item, targets)
        has_badge_target = False
        has_amulet_target = False
        has_amulet_badge_target = False
        for index in matched_indexes:
            try:
                target = targets[index]
            except Exception:
                continue
            if target.get("kind") == "badge":
                has_badge_target = True
            elif target.get("kind") == "amulet":
                has_amulet_target = True
                if target.get("badge") not in (None, ""):
                    has_amulet_badge_target = True

        if has_amulet_badge_target:
            reason = "target_amulet_badge"
        elif has_amulet_target:
            reason = "target_amulet"
        elif has_badge_target:
            reason = "target_badge"
        else:
            reason = ""
        return len(matched_indexes), reason

    def _annotate_candidate_values(
            self,
            considered: List[Dict[str, Any]],
            context: AutoRunStrategyContext,
            want_badges: set[int],
            want_amulet_regs: set[int],
            effect_list: List[Dict[str, Any]],
    ) -> None:
        need_pionner = _owned_count_with_badge(effect_list, PIONNER_BADGE_ID) < context.need_pionner_badge_count
        for item in considered:
            raw_id = int(item.get("raw_id") or 0)
            reg_id = int(item.get("reg_id") or 0)
            badge_id = item.get("badge_id")
            if badge_id is not None:
                try:
                    badge_id = int(badge_id)
                except Exception:
                    badge_id = None

            if badge_id is not None and badge_id in want_badges:
                item["selection_value"] = 99
                item["value_reason"] = "target_badge"
                continue

            if reg_id in want_amulet_regs:
                required_badges = _required_nonplus_badges_for_reg(context.targets, reg_id)
                if not required_badges:
                    item["selection_value"] = 99
                    item["value_reason"] = "target_amulet"
                elif badge_id in required_badges:
                    item["selection_value"] = 99
                    item["value_reason"] = "target_amulet_badge"
                else:
                    item["selection_value"] = 0
                    item["value_reason"] = "target_amulet_badge_mismatch"
                continue

            if need_pionner and badge_id == PIONNER_BADGE_ID:
                item["selection_value"] = 2
                item["value_reason"] = "pionner_badge"
            elif badge_id == HAPPINESS_BADGE_ID:
                item["selection_value"] = 1
                item["value_reason"] = "happiness_badge"
            else:
                item["selection_value"] = 0
                item["value_reason"] = "price_fallback"

    @staticmethod
    def _mark_candidate(
            considered: List[Dict[str, Any]],
            raw_id: Optional[int],
            *,
            selected_reason: str,
            default_reject_reason: str,
    ) -> List[Dict[str, Any]]:
        for item in considered:
            if raw_id is not None and item.get("raw_id") == raw_id:
                item["selected"] = True
                item["reason"] = selected_reason
            elif item.get("reason") == "not_evaluated":
                item["reason"] = default_reject_reason
        return considered

    @staticmethod
    def _base(raw: Any) -> int:
        try:
            return int(raw or 0) // 10
        except Exception:
            return 0

    @staticmethod
    def _first_src_base(row: Dict[str, Any]) -> Optional[int]:
        if not isinstance(row, dict):
            return None
        store = row.get("store")
        if not isinstance(store, list) or not store:
            return None
        try:
            return int(store[0]) // 10
        except Exception:
            return None

    @classmethod
    def _is_theft_like(cls, row: Dict[str, Any]) -> bool:
        unstable_id, theft_id, hacker_id = 228, 229, 232
        base = cls._base(row.get("id"))
        if base == theft_id:
            return True
        if base in (hacker_id, unstable_id) and cls._first_src_base(row) == theft_id:
            return True
        return False

    @classmethod
    def _is_kavi(cls, row: Dict[str, Any]) -> bool:
        return cls._base(row.get("id")) == 230

    def _owned_effect_sell_priority(
            self,
            effect_item: Dict[str, Any],
            context: AutoRunStrategyContext,
    ) -> int:
        if is_needed_for_any_target(effect_item, context.targets):
            return 10 ** 9

        reg_id, _is_plus, badge_id = extract_amulet_signature(effect_item)
        priority = calc_effect_price(effect_item, amulet_registry=context.amulet_registry)

        if badge_id == PIONNER_BADGE_ID:
            priority += 10000
        if badge_id == HAPPINESS_BADGE_ID:
            priority += 1000
        if reg_id == WHEEL_AMULET_REG_ID:
            priority += 10000

        return priority

    def choose_candidate(self, context: AutoRunStrategyContext) -> CandidateDecision:
        candidate_effect_list = list(getattr(context.game_state, "candidate_effect_list", None) or [])
        effect_list = list(getattr(context.game_state, "effect_list", None) or [])
        considered = self._candidate_considered(candidate_effect_list, context)
        if not candidate_effect_list:
            decision = CandidateDecision(None, None, 0, None, "no_candidates")
            self.trace_decision("choose_candidate", {"candidate_count": 0}, decision.__dict__)
            return decision

        want_badges = set()
        want_amulet_regs = set()
        for target in context.targets or []:
            kind = target.get("kind")
            if kind == "badge":
                try:
                    want_badges.add(int(target.get("id")))
                except Exception:
                    pass
            elif kind == "amulet":
                try:
                    want_amulet_regs.add(int(target.get("id")))
                except Exception:
                    pass

        self._annotate_candidate_values(considered, context, want_badges, want_amulet_regs, effect_list)

        zero_raw_ids: set[int] = set()
        best_target_raw: Optional[int] = None
        best_target_badge: Optional[int] = None
        best_target_score = 0
        best_target_reason = ""
        for candidate in candidate_effect_list:
            try:
                raw_id = int(candidate.get("id", 0))
            except Exception:
                continue
            if raw_id <= 0:
                continue
            reg_id = reg_id_of_raw(raw_id)
            badge_id = candidate_badge_id(candidate)

            target_score, target_reason = self._candidate_target_score(raw_id, badge_id, context.targets)
            if target_score > best_target_score:
                best_target_raw = raw_id
                best_target_badge = badge_id
                best_target_score = target_score
                best_target_reason = target_reason

            if reg_id in want_amulet_regs:
                required_badges = _required_nonplus_badges_for_reg(context.targets, reg_id)
                if required_badges and badge_id not in required_badges and target_score <= 0:
                    zero_raw_ids.add(raw_id)
                    item = self._candidate_item(considered, raw_id)
                    if item is not None:
                        item["reason"] = "target_amulet_badge_mismatch"

        if best_target_raw is not None:
            selected_reason = "matched_target"
            default_reject_reason = "lower_target_count_than_selected"
            if best_target_reason == "target_badge":
                selected_reason = "matched_target_badge"
                default_reject_reason = "lower_priority_than_target_badge"
            elif best_target_reason == "target_amulet":
                selected_reason = "matched_target_amulet"
                default_reject_reason = "lower_priority_than_target_amulet"
            elif best_target_reason == "target_amulet_badge":
                selected_reason = "matched_target_amulet_required_badge"
                default_reject_reason = "lower_priority_than_target_amulet_badge"
            decision = CandidateDecision(
                best_target_raw,
                best_target_badge,
                99,
                None,
                best_target_reason,
                self._mark_candidate(
                    considered,
                    best_target_raw,
                    selected_reason=selected_reason,
                    default_reject_reason=default_reject_reason,
                ),
            )
            self.trace_decision("choose_candidate", {"candidate_count": len(candidate_effect_list)}, decision.__dict__)
            return decision

        if _owned_count_with_badge(effect_list, PIONNER_BADGE_ID) < context.need_pionner_badge_count:
            for candidate in candidate_effect_list:
                badge_id = candidate_badge_id(candidate)
                if badge_id == PIONNER_BADGE_ID:
                    raw_id = int(candidate["id"])
                    decision = CandidateDecision(
                        raw_id,
                        badge_id,
                        2,
                        None,
                        "pionner_badge",
                        self._mark_candidate(
                            considered,
                            raw_id,
                            selected_reason="pionner_badge_count_below_target",
                            default_reject_reason="lower_priority_than_pionner_badge",
                        ),
                    )
                    self.trace_decision("choose_candidate", {"candidate_count": len(candidate_effect_list)}, decision.__dict__)
                    return decision

        for candidate in candidate_effect_list:
            badge_id = candidate_badge_id(candidate)
            if badge_id == HAPPINESS_BADGE_ID:
                raw_id = int(candidate["id"])
                decision = CandidateDecision(
                    raw_id,
                    badge_id,
                    1,
                    None,
                    "happiness_badge",
                    self._mark_candidate(
                        considered,
                        raw_id,
                        selected_reason="happiness_badge_fallback_priority",
                        default_reject_reason="lower_priority_than_happiness_badge",
                    ),
                )
                self.trace_decision("choose_candidate", {"candidate_count": len(candidate_effect_list)}, decision.__dict__)
                return decision

        best_raw: Optional[int] = None
        best_badge: Optional[int] = None
        best_price = -10 ** 9
        best_sell_uid: Optional[int] = None
        best_is_zero = False

        for candidate in candidate_effect_list:
            try:
                raw_id = int(candidate.get("id", 0))
            except Exception:
                continue
            if raw_id <= 0:
                continue

            badge_id = candidate_badge_id(candidate)
            reg_id = reg_id_of_raw(raw_id)

            if raw_id in zero_raw_ids:
                price = 0
                uid = _find_owned_uid_for_reg(effect_list, reg_id)
                owned_item = next((item for item in effect_list if item.get("uid") == uid), None)
                sell_uid = uid if owned_item is not None and not is_needed_for_any_target(owned_item, context.targets) else None
            else:
                price = calc_candidate_price(candidate, amulet_registry=context.amulet_registry)
                sell_uid = None

            if price > best_price:
                best_price = price
                best_raw = raw_id
                best_badge = badge_id
                best_sell_uid = sell_uid
                best_is_zero = raw_id in zero_raw_ids

        selection_value = 0 if best_is_zero or best_price <= 0 else 0
        for item in considered:
            if best_raw is not None and item.get("raw_id") == best_raw:
                item["selected"] = True
                item["reason"] = "highest_price_fallback"
            elif item.get("reason") == "not_evaluated":
                item["reason"] = "lower_price_than_selected"
        decision = CandidateDecision(best_raw, best_badge, selection_value, best_sell_uid, "best_price", considered)
        self.trace_decision("choose_candidate", {"candidate_count": len(candidate_effect_list)}, decision.__dict__)
        return decision

    def rank_sell_candidates(self, context: AutoRunStrategyContext) -> List[SellCandidate]:
        effect_list = list(getattr(context.game_state, "effect_list", None) or [])
        if not effect_list:
            return []

        normal: List[SellCandidate] = []
        demoted: List[SellCandidate] = []
        demoted_taken = 0
        for item in effect_list:
            if is_needed_for_any_target(item, context.targets):
                continue
            _reg_id, _is_plus, badge_id = extract_amulet_signature(item)
            candidate = SellCandidate(
                item=item,
                sell_priority=self._owned_effect_sell_priority(item, context),
            )
            if badge_id == PIONNER_BADGE_ID and demoted_taken < context.need_pionner_badge_count:
                demoted.append(candidate)
                demoted_taken += 1
            else:
                normal.append(candidate)

        result = normal + demoted
        self.trace_decision(
            "rank_sell_candidates",
            {"effect_count": len(effect_list)},
            {"sell_count": len(result), "uids": [item.item.get("uid") for item in result]},
        )
        return result

    def choose_same_reg_sell_uid(self, context: AutoRunStrategyContext, reg_id: int) -> Optional[int]:
        candidates: List[Dict[str, Any]] = []
        for effect_item in getattr(context.game_state, "effect_list", None) or []:
            try:
                if int(effect_item.get("id", 0)) // 10 == reg_id:
                    candidates.append(effect_item)
            except Exception:
                continue

        if not candidates:
            return None

        worst = min(
            candidates,
            key=lambda item: (
                self._owned_effect_sell_priority(item, context),
                int(item.get("uid") or 1_000_000_000),
            ),
        )
        uid = worst.get("uid")
        try:
            result = int(uid) if uid is not None else None
        except Exception:
            result = None
        self.trace_decision("choose_same_reg_sell_uid", {"reg_id": reg_id}, {"uid": result})
        return result

    def sort_uids(self, context: AutoRunStrategyContext, mode: str) -> Optional[List[int]]:
        effect_list = getattr(context.game_state, "effect_list", None) or []
        if not isinstance(effect_list, list):
            return None

        kavi, theftlike, others = [], [], []
        for row in effect_list:
            if self._is_kavi(row):
                kavi.append(row)
            elif self._is_theft_like(row):
                theftlike.append(row)
            else:
                others.append(row)

        if mode == "pre_start":
            new_order = kavi + theftlike + others
        elif mode == "pre_win":
            new_order = theftlike + kavi + others
        else:
            return None

        def uids(rows: List[Dict[str, Any]]) -> List[int]:
            out = []
            for row in rows:
                try:
                    out.append(int(row.get("uid")))
                except Exception:
                    pass
            return out

        new_uids = uids(new_order)
        old_uids = uids(effect_list)
        if len(new_uids) != len(old_uids) or set(new_uids) != set(old_uids):
            return None
        if new_uids == old_uids:
            return None
        self.trace_decision("sort_uids", {"mode": mode}, {"uids": new_uids})
        return new_uids


def default_select_amulet_from_candidates(
        candidate_effect_list: List[Dict[str, Any]],
        effect_list: List[Dict[str, Any]],
        targets: List[Dict[str, Any]],
        *,
        need_pionner_badge_count: int = NEED_PIONNER_BADGE_COUNT,
        amulet_registry: Any = None,
) -> CandidateDecision:
    class _State:
        pass

    state = _State()
    state.candidate_effect_list = candidate_effect_list
    state.effect_list = effect_list
    strategy = DefaultAutoRunStrategy()
    return strategy.choose_candidate(AutoRunStrategyContext(
        game_state=state,
        targets=targets,
        need_pionner_badge_count=need_pionner_badge_count,
        amulet_registry=amulet_registry,
    ))
