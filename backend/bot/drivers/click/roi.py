from typing import List, Tuple


def slot_centers_by_bbox(
        bbox: Tuple[int, int, int, int],
        n_slots: int,
        margin_ratio: float = 0.02,
        left_comp_px: int = 0,
        right_comp_px: int = 0
) -> List[Tuple[int, int]]:
    x1, y1, x2, y2 = bbox
    w = x2 - x1
    pad = int(round(w * margin_ratio))
    ix1 = x1 + pad + left_comp_px
    ix2 = x2 - pad - right_comp_px
    step = (ix2 - ix1) / max(1, n_slots)
    cy = (y1 + y2) // 2
    return [(int(round(ix1 + (i + 0.5) * step)), cy) for i in range(n_slots)]
