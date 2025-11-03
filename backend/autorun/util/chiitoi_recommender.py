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
    """“七对=七种不同对子”的可行性判定。允许 pool>=14（可从中挑14张）。"""
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
    - 若手里无癞子(B=0)：14 张本身严七对即可立刻和
    - 若手里有癞子(B>=1)：仅当去掉最后一张的前13张为 L+6对+0单 时，才允许立刻和
    """
    if len(hand14) != 14:
        return False
    pre13 = hand14[:-1]
    P, S, B = _distinct_pairs_singles_bd(pre13)
    if B == 0:
        # 纯自然七对：14 张本身可胡即可
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
):
    """
    返回：
      - win_now:     {"status":"win_now","draws_needed":0,"target14":[...],"discards":[]}
      - plan:        {"status":"plan","draws_needed":k,"target14":[...],"discards":[...]}
      - impossible:  {"status":"impossible","reason":"..."}
    规则：
      - 0m/0p/0s 归一到 5m/5p/5s 做判定；丢牌时仍按 id 输出，且需要打五时优先打“5x”而不是“0x”
      - 牌山中遇到 bd 直接忽略；bd 只会在手牌出现，可补任意牌成对子
    """
    from collections import Counter
    from math import inf

    def _ids_norm(ids: List[int]) -> List[str]:
        return [_norm_tile(deck_map[i]) for i in ids]

    def _wall_norm_no_bd(ids: List[int]) -> List[str]:
        arr = [_norm_tile(deck_map[i]) for i in ids]
        return [t for t in arr if t != JOKER]

    def _build_target14_from_pool(pool_norm_with_bd: List[str]) -> List[str]:
        """从可行池里构造一种严格七对14张（仅用于展示；0x已归一为5x；bd 保留为 'bd'）。"""
        cnt = Counter(t for t in pool_norm_with_bd if t != JOKER)
        B = pool_norm_with_bd.count(JOKER)

        pairs = [t for t, v in cnt.items() if v >= 2]   # 自然对子种
        singles = [t for t, v in cnt.items() if v == 1] # 单张种
        res: List[str] = []

        # 先放自然对子
        used_pairs = 0
        for t in pairs:
            if used_pairs == 7:
                break
            res += [t, t]
            used_pairs += 1

        # 再用“单张 + bd”补对子
        i = 0
        while used_pairs < 7 and i < len(singles) and B >= 1:
            res += [singles[i], JOKER]
            B -= 1
            used_pairs += 1
            i += 1

        # 不够再用纯 bd
        while used_pairs < 7 and B >= 2:
            res += [JOKER, JOKER]
            B -= 2
            used_pairs += 1

        return res if used_pairs == 7 and len(res) == 14 else []

    # 规范化
    if len(hand_ids) != 14:
        return {"status": "impossible", "reason": "hand-must-be-14"}

    H_raw = [deck_map[i] for i in hand_ids]
    H_norm = [_norm_tile(x) for x in H_raw]         # 判定用（0x -> 5x）
    W_norm_no_bd = _wall_norm_no_bd(wall_ids)       # 牌山无 bd

    # 起手可胡？（新摸在末位）
    if _win_now_draw_sensitive_14(H_norm):
        target14 = _build_target14_from_pool(H_norm)
        return {
            "status": "win_now",
            "draws_needed": 0,
            "target14": target14,
            "discards": []
        }

    # 起手应打哪张以达到“最早自摸”
    hand_cnt = Counter(H_norm)
    candidates = []  # (idx, k, tie_score, prefer_penalty)

    for idx in range(14):
        hand13 = H_norm[:idx] + H_norm[idx+1:]
        k = _earliest_draws_after_discard_to_chiitoi(hand13, W_norm_no_bd)  # 最早几摸
        tie_score = _helpfulness(H_norm[idx], hand_cnt, W_norm_no_bd)       # 次级排序
        prefer_penalty = 0
        if H_norm[idx] in FIVES:
            has_red = any(r in RED_FIVES_RAW and _norm_tile(r) == H_norm[idx] for r in H_raw)
            has_plain = any(r == H_norm[idx] for r in H_raw)
            if has_red and has_plain and H_raw[idx] in RED_FIVES_RAW:
                # 同面同时持有 0x 与 5x 时，丢 0x 施加惩罚（优先丢 5x）
                prefer_penalty = 1
        candidates.append((idx, k, tie_score, prefer_penalty))

    # 速度优先：k 最小 → tie_score → prefer_penalty（普通五优先丢）
    candidates.sort(key=lambda x: (x[1], x[2], x[3]))
    best_idx, k_found, _, _ = candidates[0]

    if k_found is inf:
        return {"status": "impossible", "reason": "no-path-to-chiitoi"}

    # 逐巡执行到自摸：本步先丢一张，其后每摸一张再丢一张，直到自摸前一手
    discards: List[int] = []
    cur_ids = list(hand_ids)
    cur_norm = list(H_norm)

    # 五优先规则修正：如果是 5x 面且同时有 0x 与 5x，优先丢 5x（非红）
    best_idx = _prefer_plain_five_over_red(best_idx, H_raw, H_norm)

    if k_found > 0:
        discards.append(hand_ids[best_idx])
        del cur_ids[best_idx]
        del cur_norm[best_idx]

    # 按计划摸 k_found 次；第 k_found 次即自摸，不再丢
    for j in range(k_found):
        draw_id = wall_ids[j]
        draw_norm = _norm_tile(deck_map[draw_id])
        cur_ids.append(draw_id)
        cur_norm.append(draw_norm)

        if j == k_found - 1:
            break  # 这一摸自摸

        rest_norm_no_bd = _wall_norm_no_bd(wall_ids[j+1:k_found])

        # 丟一張，保证仍能在剩余步数内完成（保持全局最早 k）
        best2_idx = None
        best2_key = None
        for idx2 in range(len(cur_norm)):
            nh = cur_norm[:idx2] + cur_norm[idx2+1:]
            k2 = _earliest_draws_after_discard_to_chiitoi(nh, rest_norm_no_bd)
            if k2 <= (k_found - (j+1)):
                tie = _helpfulness(cur_norm[idx2], Counter(cur_norm), rest_norm_no_bd)
                red_penalty = 0
                if cur_norm[idx2] in FIVES:
                    raw_set = [deck_map[_id] for _id in cur_ids]
                    has_red = any(r in RED_FIVES_RAW and _norm_tile(r) == cur_norm[idx2] for r in raw_set)
                    has_plain = any(r == cur_norm[idx2] for r in raw_set)
                    if has_red and has_plain and deck_map[cur_ids[idx2]] in RED_FIVES_RAW:
                        red_penalty = 1
                key = (k2, tie, red_penalty)
                if best2_key is None or key < best2_key:
                    best2_key = key
                    best2_idx = idx2

        if best2_idx is None:
            # 兜底（极少）：选一个对后续帮助最小的
            best2_idx = min(range(len(cur_norm)),
                            key=lambda i: _helpfulness(cur_norm[i], Counter(cur_norm), rest_norm_no_bd))

        discards.append(cur_ids[best2_idx])
        del cur_ids[best2_idx]
        del cur_norm[best2_idx]

    pool_norm = _ids_norm(hand_ids + wall_ids[:k_found])
    target14 = _build_target14_from_pool(pool_norm)

    return {
        "status": "plan",
        "draws_needed": k_found,
        "target14": target14,
        "discards": discards
    }
