from typing import Dict, Tuple, List


def abc_slot_centers_16x9(
        x_left: float, x_right: float, y: float, margin_ratio: float = 0.06
) -> Dict[str, Tuple[float, float]]:
    pad = (x_right - x_left) * margin_ratio
    ix1, ix2 = x_left + pad, x_right - pad
    A = (ix1, y)
    C = (ix2, y)
    B = ((ix1 + ix2) / 2.0, y)
    return {"A": A, "B": B, "C": C}


def place_ops_to_abc_16x9(
        x_left: float, x_right: float, y: float, present_ops: List[int],
        KAN: int, TSUMO: int, SKIP: int, margin_ratio: float = 0.06
) -> Dict[int, Tuple[float, float]]:
    centers = abc_slot_centers_16x9(x_left, x_right, y, margin_ratio)
    present = set(present_ops)
    out: Dict[int, Tuple[float, float]] = {}

    if SKIP in present:
        out[SKIP] = centers["C"]

    has_kan = KAN in present
    has_tsumo = TSUMO in present

    if has_kan and has_tsumo:
        out[KAN] = centers["A"]
        out[TSUMO] = centers["B"]
    elif has_tsumo:
        out[TSUMO] = centers["B"]
    elif has_kan:
        out[KAN] = centers["B"]

    return out
