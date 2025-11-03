from collections import OrderedDict
from itertools import combinations
from typing import List, Counter, Optional, Dict, Tuple


def plan_pure_pinzu_suu_ankou(
        hand_tiles: List[int],
        future_draw_ids: List[int],
        deck_map: "OrderedDict[int, str]",
) -> Optional[Dict]:
    """
    规划：只追求【纯饼四暗刻】，bd 仅可当作任意饼子；0p 视为 5p，打 5p 时优先打 5p 而不是 0p。
    若无法在未来摸牌内达成，返回 None。
    返回的 discards 是【要打出去的牌 id 列表】（从当前这一步开始，直到自摸前一手）。
    """

    def face_of(i: int) -> str:
        return deck_map[i]

    def is_pinzu(face: str) -> bool:
        # 仅 p 花色；0p 当 5p；bd 单独处理
        return (len(face) == 2 and face.endswith("p")) or face == "0p"

    def pin_rank(face: str) -> Optional[int]:
        # 返回 1..9；0p → 5；非饼/癞子返回 None
        if face == "0p":
            return 5
        if len(face) == 2 and face[1] == "p" and face[0].isdigit():
            d = int(face[0])
            if 1 <= d <= 9:
                return d
        return None

    def normalize_pin(face: str) -> Optional[str]:
        # 统一饼子牌面：'0p' 统一为 '5p'
        r = pin_rank(face)
        return f"{r}p" if r is not None else None

    def count_pin_and_bd(ids: List[int]) -> Tuple[Counter, int, Counter]:
        """
        统计：各饼子点数的自然张数（'1p'..'9p'），以及 bd 数量。
        同时返回 0p/5p 的来源计数（用于丢牌时优先打 5p 而不是 0p）。
        """
        pin_counter = Counter()
        five_sources = Counter()  # {"5p": 非红五自然5p数量, "0p": 红五数量}
        bd_cnt = 0
        for i in ids:
            f = face_of(i)
            if f == "bd":
                bd_cnt += 1
            else:
                r = pin_rank(f)
                if r is not None:
                    if f == "0p":
                        pin_counter["5p"] += 1
                        five_sources["0p"] += 1
                    else:
                        norm = normalize_pin(f)
                        pin_counter[norm] += 1
                        if norm == "5p":
                            five_sources["5p"] += 1
        return pin_counter, bd_cnt, five_sources

    def available_after_k(hand_ids: List[int], k: int) -> Tuple[Counter, int, Counter]:
        pool = hand_ids + future_draw_ids[:k]
        return count_pin_and_bd(pool)

    def exists_pure_pinzu_suuankou(pin_counter: Counter, bd_cnt: int) -> Optional[Dict]:
        """
        给定“可用池”（当前 + 前 k 摸），判断是否存在纯饼四暗刻（4刻 + 1雀头）目标。
        只从饼子里选；bd 只能补到饼子。
        返回一个 dict:
          {
            "need": {"1p":3, "3p":3, "7p":3, "9p":3, "5p":2},  # 例
            "bd_used": N,
          }
        若不可行，返回 None。
        """
        ranks = [f"{d}p" for d in range(1, 10)]

        # 选择 4 个不同的刻子点数
        for triplet_ranks in combinations(ranks, 4):
            # 雀头可以与刻子点数重复（需要 5 张该点数；bd 可补）
            for pair_r in ranks:
                need = Counter()
                for tr in triplet_ranks:
                    need[tr] += 3
                need[pair_r] += 2

                # 仅使用饼子与 bd 补足
                deficit = 0
                for r in ranks:
                    have = pin_counter.get(r, 0)
                    req = need.get(r, 0)
                    if req > have:
                        deficit += (req - have)

                if deficit <= bd_cnt:
                    return {"need": need, "bd_used": deficit}
        return None

    k_found = None
    target = None
    for k in range(0, len(future_draw_ids) + 1):
        pin_cnt, bd_cnt, _ = available_after_k(hand_tiles, k)
        tgt = exists_pure_pinzu_suuankou(pin_cnt, bd_cnt)
        if tgt is not None:
            k_found = k
            target = tgt
            break

    if target is None:
        return None

    pin_all, bd_all, five_src_all = available_after_k(hand_tiles, k_found)
    need = target["need"].copy()
    nat_need = Counter()
    for r in list(need.keys()):
        use_nat = min(need[r], pin_all.get(r, 0))
        nat_need[r] += use_nat
        need[r] -= use_nat

    discards: List[int] = []
    cur_ids = list(hand_tiles)

    def still_feasible_after_discard(discard_id: int, future_rest_ids: List[int]) -> bool:
        # 丢掉 discard_id 后，看“当前剩余 + 未来剩余”是否仍能满足 nat_need + bd 补足总目标
        tmp_ids = [x for x in cur_ids if x != discard_id]  # 移除一个该 id
        pin_c, bd_c, _ = count_pin_and_bd(tmp_ids + future_rest_ids)

        for r in nat_need:
            if pin_c.get(r, 0) < nat_need[r]:
                return False

        total_need = target["need"].copy()
        deficit = 0
        for r in [f"{d}p" for d in range(1, 10)]:
            have = pin_c.get(r, 0)
            req = total_need.get(r, 0)
            if have < req:
                deficit += (req - have)
        return deficit <= bd_c

    def discard_score(tile_id: int, future_rest_ids: List[int]) -> Tuple[int, int, int, int]:
        f = face_of(tile_id)
        # 非饼 & 非 bd：最优先丢
        if f != "bd" and not is_pinzu(f):
            return (0, 0, 0, tile_id)

        if f == "bd":
            return (3, 0, 0, tile_id)

        r = normalize_pin(f)  # '1p'..'9p'
        need_total = target["need"].get(r, 0)

        cur_pin_counts, _, cur_five_src = count_pin_and_bd(cur_ids)
        fut_pin_counts, _, _ = count_pin_and_bd(future_rest_ids)
        naturals_total = cur_pin_counts.get(r, 0) + fut_pin_counts.get(r, 0)

        over = max(0, naturals_total - need_total)
        # 基础分：越“超编”越该丢
        base = 1 if over > 0 else 2

        is_five = (r == "5p")
        is_red = (f == "0p")
        red_bias = 1 if (is_five and is_red) else 0

        # 再给一个稳定 tie-breaker
        return (base, red_bias, 0 if r else 1, tile_id)

    if k_found > 0:
        future_rest = future_draw_ids[:k_found]
        candidates = [x for x in set(cur_ids) if still_feasible_after_discard(x, future_rest)]
        if not candidates:
            # 理论上不会发生：因为整体是可行的
            candidates = list(set(cur_ids))

        best = min(candidates, key=lambda t: discard_score(t, future_rest))
        discards.append(best)
        cur_ids.remove(best)  # 移除一个 best

    for j in range(k_found):
        draw_id = future_draw_ids[j]
        cur_ids.append(draw_id)
        future_rest = future_draw_ids[j + 1:k_found]

        if j == k_found - 1:
            break

        # 选一张要丢的，保持“仍可达成目标”
        candidates = [x for x in set(cur_ids) if still_feasible_after_discard(x, future_rest)]
        if not candidates:
            # 兜底
            candidates = list(set(cur_ids))

        best = min(candidates, key=lambda t: discard_score(t, future_rest))
        discards.append(best)
        cur_ids.remove(best)

    # 生成解释性的 target14
    target_face = []
    for d in range(1, 10):
        r = f"{d}p"
        target_face += [r] * target["need"].get(r, 0)

    return {
        "draws_needed": k_found,
        "target14": target_face,
        "discards": discards,
    }


def plan_pure_pinzu_suu_ankou_v2(hand_tiles, future_draw_ids, deck_map):
    base = plan_pure_pinzu_suu_ankou(hand_tiles, future_draw_ids, deck_map)
    if base is None:
        return {
            "status": "impossible",  # 无论怎么摸都做不成“纯饼四暗刻”
            "reason": "not-enough-pinzu-or-bd"
        }
    # base = {"draws_needed": k_found, "target14": [...], "discards": [...]}
    k = base["draws_needed"]
    if k == 0:
        return {
            "status": "win_now",  # 起手即和
            "draws_needed": 0,
            "target14": base["target14"],
            "discards": []  # 这步不用打牌
        }
    else:
        return {
            "status": "plan",  # 需要再摸 k 次；下面给出每步要打的 id
            "draws_needed": k,
            "target14": base["target14"],
            "discards": base["discards"]
        }
