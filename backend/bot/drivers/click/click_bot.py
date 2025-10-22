from typing import List, Dict, Optional, Callable
from backend.bot.core.interfaces import GameBot
from .pipeline import BotPipeline, BotConfig
from backend.app import GAME_STATE


class ClickBot(GameBot):
    def __init__(self, cfg: BotConfig):
        self.pipeline = BotPipeline(cfg)

    def bind(self) -> bool:
        return self.pipeline.bind_window()

    def refresh(self) -> bool:
        return self.pipeline.refresh_viewport()

    def click_op(
            self,
            present_ops: List[int],
            target_op: int,
            state_ok_pred: Optional[Callable[[], bool]] = None
    ) -> bool:
        return self.pipeline.click_op(present_ops, target_op, state_ok_pred)

    def discard_by_index(self, slot_idx: int, n_slots: Optional[int] = None) -> bool:
        return self.pipeline.click_discard_by_index(slot_idx, n_slots)

    def discard_by_tile_id(
            self,
            tile_id: int,
            allow_tsumogiri: bool = True
    ) -> bool:
        return self.pipeline.click_discard_by_tile_id(
            tile_id=tile_id,
            hand_ids_with_draw=GAME_STATE.hand_tiles,
            id2label=GAME_STATE.deck_map,
            allow_tsumogiri=allow_tsumogiri
        )
