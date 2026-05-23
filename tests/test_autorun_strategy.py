from __future__ import annotations

import unittest
from enum import Enum

from backend.autorun.strategy import (
    DefaultAutoRunStrategy,
    AutoRunStrategyContext,
    select_items_to_sell_for_purchase,
)
from backend.autorun.value import calc_amulet_price, calc_target_achievement_value


class FakeRarity(Enum):
    GREEN = 1
    BLUE = 2
    ORANGE = 3
    PURPLE = 4


class FakeAmulet:
    def __init__(self, rarity: FakeRarity) -> None:
        self.rarity = rarity


class FakeRegistry:
    def __init__(self) -> None:
        self.items = {
            100: FakeAmulet(FakeRarity.GREEN),
            101: FakeAmulet(FakeRarity.BLUE),
            102: FakeAmulet(FakeRarity.ORANGE),
            103: FakeAmulet(FakeRarity.PURPLE),
            146: FakeAmulet(FakeRarity.GREEN),
            228: FakeAmulet(FakeRarity.PURPLE),
        }

    def get(self, item_id: int):
        return self.items.get(item_id)


class FakeState:
    def __init__(self, *, candidates=None, effects=None) -> None:
        self.candidate_effect_list = list(candidates or [])
        self.effect_list = list(effects or [])


class AutorunStrategyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.registry = FakeRegistry()

    def context(self, *, candidates=None, effects=None, targets=None, need_pionner=4):
        return AutoRunStrategyContext(
            game_state=FakeState(candidates=candidates, effects=effects),
            targets=list(targets or []),
            need_pionner_badge_count=need_pionner,
            amulet_registry=self.registry,
        )

    def test_calc_amulet_price_uses_rarity_gold_and_unstable_rules(self) -> None:
        self.assertEqual(calc_amulet_price(raw_id=1000, amulet_registry=self.registry), 3)
        self.assertEqual(calc_amulet_price(raw_id=1010, amulet_registry=self.registry), 6)
        self.assertEqual(calc_amulet_price(raw_id=1020, amulet_registry=self.registry), 9)
        self.assertEqual(calc_amulet_price(raw_id=1030, amulet_registry=self.registry), 12)
        self.assertEqual(calc_amulet_price(raw_id=1000, badge_id=600050, amulet_registry=self.registry), 9)
        self.assertEqual(calc_amulet_price(raw_id=2280, badge_id=600050, amulet_registry=self.registry), 0)
        self.assertEqual(calc_amulet_price(raw_id=9990, amulet_registry=self.registry), 0)

    def test_calc_target_achievement_value_deduplicates_target_hits(self) -> None:
        targets = [
            {"kind": "amulet", "id": 100, "plus": False, "badge": 600070, "value": 3},
            {"kind": "badge", "id": 600070, "value": 5},
            {"kind": "amulet", "id": 100, "plus": True, "value": 11},
        ]
        effects = [
            {"id": 1000, "badge": {"id": 600070}},
            {"id": 1000, "badge": {"id": 600070}},
            {"id": 1001},
        ]
        self.assertEqual(calc_target_achievement_value(effects, targets), 19)

    def test_choose_candidate_prioritizes_targets_then_pionner_then_happiness(self) -> None:
        strategy = DefaultAutoRunStrategy()
        decision = strategy.choose_candidate(self.context(
            candidates=[{"id": 1010, "badgeId": 600110}, {"id": 1020, "badgeId": 600070}],
            targets=[{"kind": "badge", "id": 600110, "value": 1}],
        ))
        self.assertEqual(decision.raw_id, 1010)
        self.assertEqual(decision.selection_value, 99)
        self.assertEqual(decision.reason, "target_badge")

        decision = strategy.choose_candidate(self.context(
            candidates=[{"id": 1010, "badgeId": 600110}, {"id": 1020, "badgeId": 600070}],
            targets=[],
            need_pionner=4,
        ))
        self.assertEqual(decision.badge_id, 600070)
        self.assertEqual(decision.selection_value, 2)

        decision = strategy.choose_candidate(self.context(
            candidates=[{"id": 1010, "badgeId": 600110}],
            targets=[],
        ))
        self.assertEqual(decision.badge_id, 600110)
        self.assertEqual(decision.selection_value, 1)

    def test_choose_candidate_respects_required_target_badge(self) -> None:
        strategy = DefaultAutoRunStrategy()
        decision = strategy.choose_candidate(self.context(
            candidates=[{"id": 1000, "badgeId": 600120}],
            effects=[{"id": 1000, "uid": 7, "badge": {"id": 600120}}],
            targets=[{"kind": "amulet", "id": 100, "plus": False, "badge": 600070, "value": 1}],
            need_pionner=0,
        ))
        self.assertNotEqual(decision.selection_value, 99)
        self.assertIsNone(decision.sell_uid)

        decision = strategy.choose_candidate(self.context(
            candidates=[{"id": 1000, "badgeId": 600070}],
            targets=[{"kind": "amulet", "id": 100, "plus": False, "badge": 600070, "value": 1}],
        ))
        self.assertEqual(decision.selection_value, 99)
        self.assertEqual(decision.reason, "target_amulet_badge")

    def test_choose_candidate_uses_highest_target_count(self) -> None:
        strategy = DefaultAutoRunStrategy()
        decision = strategy.choose_candidate(self.context(
            candidates=[
                {"id": 1000, "badgeId": 600120},
                {"id": 1010, "badgeId": 600070},
            ],
            targets=[
                {"kind": "amulet", "id": 100, "plus": False, "value": 1},
                {"kind": "amulet", "id": 101, "plus": False, "badge": 600070, "value": 1},
                {"kind": "badge", "id": 600070, "value": 1},
            ],
        ))
        self.assertEqual(decision.raw_id, 1010)
        self.assertEqual(decision.badge_id, 600070)
        self.assertEqual(decision.selection_value, 99)
        self.assertEqual(decision.reason, "target_amulet_badge")

    def test_rank_sell_candidates_protects_targets_and_demotes_kept_badges(self) -> None:
        strategy = DefaultAutoRunStrategy()
        effects = [
            {"id": 1000, "uid": 1, "volume": 1},
            {"id": 1010, "uid": 2, "volume": 1, "badge": {"id": 600070}},
            {"id": 1460, "uid": 3, "volume": 1},
            {"id": 1020, "uid": 4, "volume": 1, "badge": {"id": 600110}},
            {"id": 1030, "uid": 5, "volume": 1},
        ]
        ranked = strategy.rank_sell_candidates(self.context(
            effects=effects,
            targets=[{"kind": "amulet", "id": 103, "value": 1}],
            need_pionner=1,
        ))
        self.assertNotIn(5, [item.item["uid"] for item in ranked])
        self.assertEqual(ranked[-1].item["uid"], 2)
        self.assertGreater(
            next(item.sell_priority for item in ranked if item.item["uid"] == 3),
            next(item.sell_priority for item in ranked if item.item["uid"] == 1),
        )

    def test_select_items_to_sell_for_purchase(self) -> None:
        chosen, freed, enough = select_items_to_sell_for_purchase(
            free_space=0,
            need_space=3,
            sell_candidates=[{"uid": 1, "volume": 1}, {"uid": 2, "volume": 2}],
        )
        self.assertTrue(enough)
        self.assertEqual(freed, 3)
        self.assertEqual([item["uid"] for item in chosen], [1, 2])

        chosen, freed, enough = select_items_to_sell_for_purchase(
            free_space=1,
            need_space=4,
            sell_candidates=[{"uid": 1, "volume": 2}],
        )
        self.assertFalse(enough)
        self.assertEqual(freed, 2)
        self.assertEqual([item["uid"] for item in chosen], [1])


if __name__ == "__main__":
    unittest.main()
