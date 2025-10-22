from collections import Counter
from math import inf
from typing import Dict, List, Tuple, Literal

# 规则与映射
RED_MAP = {"0m": "5m", "0p": "5p", "0s": "5s"}  # 红5归一到普通5
JOKER = "bd"  # 癞子（仅手牌可出现）

FIVES = {"5m", "5p", "5s"}
RED_FIVES_RAW = {"0m", "0p", "0s"}
PLAIN_FIVES_RAW = {"5m", "5p", "5s"}


def _norm_tile(t: str) -> str:
    return RED_MAP.get(t, t)


def _ids_to_tiles(ids: List[int], deck_map: Dict[int, str]) -> List[str]:
    return [_norm_tile(deck_map[i]) for i in ids]


def _distinct_pairs_singles_bd(tiles: List[str]) -> Tuple[int, int, int]:
    """
    返回 (pairs_distinct, singles_distinct, B)
    - pairs_distinct: 非癞子中 计数(v>=2) 的“不同对子牌种数”；v=3或v=4 也只算 1 个对子
    - singles_distinct: 非癞子中 计数(v==1) 的“不同单张牌种数”
    - B: 癞子张数（bd）
    """
    cnt = Counter(t for t in tiles if t != JOKER)
    pairs_distinct = sum(1 for v in cnt.values() if v >= 2)
    singles_distinct = sum(1 for v in cnt.values() if v == 1)
    B = tiles.count(JOKER)
    return pairs_distinct, singles_distinct, B


def _can_pick_14_as_chiitoi_distinct(pool: List[str]) -> bool:
    """严格“七对=七种不同对子”的可行性判定。允许 pool>=14（可从中挑14张）。"""
    if len(pool) < 14:
        return False
    P, S, B = _distinct_pairs_singles_bd(pool)
    need_pairs = max(0, 7 - P)
    use_bd_on_singles = min(need_pairs, S)  # 先把不同单张用癞子各补一张 → 成对
    remain_pairs = need_pairs - use_bd_on_singles
    bd_needed = use_bd_on_singles * 1 + remain_pairs * 2
    return B >= bd_needed


def _win_now_draw_sensitive_14(hand14: List[str]) -> bool:
    """
    14张，最后一张为新摸：
    - 若手里无癞子(B=0)：14 张本身严格七对即可立刻和
    - 若手里有癞子(B>=1)：仅当去掉最后一张的前13张为 L+6对+0单 时，才允许立刻和
    """
    if len(hand14) != 14:
        return False
    pre13 = hand14[:-1]
    P, S, B = _distinct_pairs_singles_bd(pre13)
    if B == 0:
        # 纯自然七对：14 张本身严格可胡即可
        return _can_pick_14_as_chiitoi_distinct(hand14)
    else:
        # 有癞子时，必须是“真听癞子”（pre13=L+6对+0单）才允许即胡
        return P == 6 and S == 0


def _earliest_draws_after_discard_to_chiitoi(hand13: List[str], wall_tiles_norm_no_bd: List[str]) -> int:
    for k in range(1, len(wall_tiles_norm_no_bd) + 1):
        if _can_pick_14_as_chiitoi_distinct(hand13 + wall_tiles_norm_no_bd[:k]):
            return k
    return inf


def _uke_ire_from_wall_available_first(
        hand13_norm: List[str],
        wall_ids: List[int],
        deck_map: Dict[int, str],
) -> Tuple[int, Dict[str, int]]:
    """
    逐张遍历牌山（顺序无关，仅统计张数），把“这张+hand13”能成严格七对的计入。
    - 返回: (受入总张数, {面:张数})
    - 规则：红5已归一；牌山遇到bd一律忽略；七对必须7种不同牌。
    - 快速通路：B>=1 且 非癞子“不同对子数”==6 且 “不同单张数”==0 → 任摸皆胡（除bd）
    """
    P, S, B = _distinct_pairs_singles_bd(hand13_norm)
    if B >= 1 and P == 6 and S == 0:
        faces = [_norm_tile(deck_map[i]) for i in wall_ids]
        faces = [f for f in faces if f != JOKER]  # 牌山不含癞子
        cnt = Counter(faces)
        return sum(cnt.values()), dict(cnt)

    total = 0
    face_counter = Counter()
    for iid in wall_ids:
        f = _norm_tile(deck_map[iid])
        if f == JOKER:
            continue  # 牌山不会摸到癞子：忽略
        if _can_pick_14_as_chiitoi_distinct(hand13_norm + [f]):
            total += 1
            face_counter[f] += 1
    return total, dict(face_counter)


def _first_idx(arr: List[str], target: str) -> int:
    try:
        return arr.index(target)
    except ValueError:
        return inf


def _helpfulness(tile: str, hand_cnt: Counter, wall_tiles_norm_no_bd: List[str]) -> float:
    """
    越小越该切：
    - 不切癞子
    - 冗余(>=3)优先切
    - 已成对(==2)保留
    - 单张看牌山最近出现位置（越近越不该切）
    """
    if tile == JOKER:
        return 1e9  # 癞子尽量不切
    c = hand_cnt[tile]
    if c >= 3:
        return -1000 - c
    if c == 2:
        return 100
    nxt = _first_idx(wall_tiles_norm_no_bd, tile)
    return -10 if nxt is inf else (50 - 0.1 * nxt)


def _prefer_plain_five_over_red(
        picked_idx: int,
        hand_raw: List[str],
        hand_norm: List[str],
) -> int:
    norm_face = hand_norm[picked_idx]
    raw_face = hand_raw[picked_idx]
    if norm_face not in FIVES:
        return picked_idx
    has_red = any(r in RED_FIVES_RAW and _norm_tile(r) == norm_face for r in hand_raw)
    has_plain = any(r == norm_face for r in hand_raw)
    if has_red and has_plain:
        for i, r in enumerate(hand_raw):
            if r == norm_face:  # 非红5
                return i
    return picked_idx


def chiitoi_recommendation_json(
        deck_map: Dict[int, str],
        hand_ids: List[int],  # 14张（最后一张为新摸）
        wall_ids: List[int],
        policy: Literal["speed", "count"] = "speed",
):
    # 非14张：七对不适用（鸣牌等）
    if len(hand_ids) != 14:
        return {
            "type": "chiitoi_recommendation",
            "data": {
                "policy": policy,
                "applicable": False,
                "could_win_now": False,
                "sacrifice_for_count": False,
                "win_now": False,
                "discard_id": None,
                "discard_tile": None,
                "discard_tile_raw": None,
                "draws_needed": None,
                "uke_ire_total": None,
                "uke_detail": None
            }
        }

    # 规范化
    H_raw = [deck_map[i] for i in hand_ids]
    H = [_norm_tile(x) for x in H_raw]
    W_norm = _ids_to_tiles(wall_ids, deck_map)
    W_norm_no_bd = [t for t in W_norm if t != JOKER]  # 牌山不含癞子

    could_win_now = _win_now_draw_sensitive_14(H)
    if could_win_now:
        return {
            "type": "chiitoi_recommendation",
            "data": {
                "policy": policy,
                "applicable": True,
                "could_win_now": True,
                "sacrifice_for_count": False,
                "win_now": True,
                "discard_id": None,
                "discard_tile": None,
                "discard_tile_raw": None,
                "draws_needed": 0,
                "uke_ire_total": None,
                "uke_detail": None
            }
        }

    # 进入候选比较（当下不能立刻和）
    hand_cnt = Counter(H)
    candidates = []  # (idx, k, tie_score, uke_total, uke_detail, prefer_penalty)

    # 标记同面是否同时持有红/非红 5x
    both_variant_5x = {}
    for f in FIVES:
        has_red = any(r in RED_FIVES_RAW and _norm_tile(r) == f for r in H_raw)
        has_plain = any(r == f for r in H_raw)
        both_variant_5x[f] = has_red and has_plain

    for idx, _ in enumerate(hand_ids):
        tile_raw = H_raw[idx]
        tile_norm = H[idx]
        hand13 = H[:idx] + H[idx + 1:]

        # 速度：最早几摸
        k = _earliest_draws_after_discard_to_chiitoi(hand13, W_norm_no_bd)

        # 次数：available-first 受入统计（忽略bd）
        if policy == "count":
            uke_total, uke_detail = _uke_ire_from_wall_available_first(hand13, wall_ids, deck_map)
        else:
            uke_total, uke_detail = 0, None

        # 启发式
        tie_score = _helpfulness(tile_norm, hand_cnt, W_norm_no_bd)

        # 红5偏好：同面有0x与5x时，红5惩罚1
        prefer_penalty = 0
        if tile_norm in FIVES and both_variant_5x.get(tile_norm, False):
            if tile_raw in RED_FIVES_RAW:
                prefer_penalty = 1

        candidates.append((idx, k, tie_score, uke_total, uke_detail, prefer_penalty))

    # 排序/选择
    if policy == "count":
        # 受入多 → 更快 → 更该切 → 普通5优先
        candidates.sort(key=lambda x: (-x[3], x[1], x[2], x[5]))
    else:
        # 更快 → 更该切 → 受入多 → 普通5优先
        candidates.sort(key=lambda x: (x[1], x[2], -x[3], x[5]))

    idx, k, _, uke_total, uke_detail, _ = candidates[0]

    # 红5优先纠偏
    final_idx = _prefer_plain_five_over_red(idx, H_raw, H)

    discard_id = hand_ids[final_idx]
    discard_tile_raw = H_raw[final_idx]
    discard_tile_norm = H[final_idx]

    if policy == "count":
        data = {
            "policy": policy,
            "applicable": True,
            "could_win_now": False,
            "sacrifice_for_count": False,
            "win_now": False,
            "discard_id": discard_id,
            "discard_tile": discard_tile_norm,  # 归一面（0x→5x）
            "discard_tile_raw": discard_tile_raw,
            "draws_needed": None if k is inf else k,
            "uke_ire_total": uke_total,
            "uke_detail": uke_detail  # {面:张数}
        }
    else:  # speed 且当前未成和
        data = {
            "policy": policy,
            "applicable": True,
            "could_win_now": False,
            "sacrifice_for_count": False,
            "win_now": False,
            "discard_id": discard_id,
            "discard_tile": discard_tile_norm,
            "discard_tile_raw": discard_tile_raw,
            "draws_needed": None if k is inf else k,
            "uke_ire_total": None,
            "uke_detail": None
        }

    return {"type": "chiitoi_recommendation", "data": data}
