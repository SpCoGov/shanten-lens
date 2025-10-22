from typing import Dict, List, Tuple

# 花色与权重：m=万, p=筒, s=索, z=字；bd(癞子)最左且不可打出
_SUIT_WEIGHT = {"bd": -1, "m": 0, "p": 1, "s": 2, "z": 3}


def _tile_key(label: str) -> Tuple[int, int]:
    """
    把牌面字符串映射到排序键：
      - "bd"   -> 最左（不可打）
      - "1m9m" -> 1..9，红五用 '0m'，排在 4 之后 5 之前
      - "1p..9p"，'0p' 在 4 之后
      - "1s..9s"，'0s' 在 4 之后
      - "1z..7z"  字牌
    返回 (大类序, 同类内序)
    """
    if label == "bd":
        return _SUIT_WEIGHT["bd"], -999

    if len(label) != 2:
        # 容错：未知格式，放到最后
        return 999, 999

    rank_ch, suit = label[0], label[1]
    if suit not in ("m", "p", "s", "z"):
        return 999, 999

    suit_w = _SUIT_WEIGHT[suit]

    # 字牌：1z..7z
    if suit == "z":
        if rank_ch.isdigit():
            r = int(rank_ch)
            return suit_w, r
        return suit_w, 99

    # 数牌：1..9，红五 0*
    if rank_ch == "0":
        # 红五介于 4 与 5 之间，设为 45
        r = 45
    elif rank_ch.isdigit():
        r = int(rank_ch) * 10  # *10 让 45 能插在 40 和 50 之间
    else:
        r = 999

    return suit_w, r


def sort_hand_labels(hand_labels: List[str]) -> List[str]:
    """
    按你给的规则对 13 张“可排序部分”排序（不含最后一张摸牌）。
    注意：这个函数不处理“最后一张摸牌”逻辑。外层需要把最后一张保留在末尾。
    """
    return sorted(hand_labels, key=_tile_key)


def screen_slot_indices_from_hand(hand_labels_with_draw: List[str]) -> List[int]:
    """
    给出屏幕显示的槽位索引（0..N-1），对应传入的每一张牌的“屏幕位置”。
    规则：
      - 输入 hand_labels_with_draw 的最后一个元素视为“摸进来的牌”，永远放在最右侧
      - 其余 N-1 张按 sort_hand_labels 的规则在左侧从左到右排序
    返回：与输入等长的数组，元素是该牌应在的槽位索引
    """
    n = len(hand_labels_with_draw)
    if n == 0:
        return []
    if n == 1:
        return [0]

    # 拆分：左侧可排序部分 + 末尾摸牌
    left = hand_labels_with_draw[:-1]
    draw = hand_labels_with_draw[-1]

    left_sorted = sort_hand_labels(left)
    # 构造“目标屏幕顺序”
    target = left_sorted + [draw]

    # 为了处理重复牌，对每个 label 建“出现次序”索引
    def enumerate_positions(seq: List[str]):
        pos_map = {}
        out = []
        for s in seq:
            k = (s, pos_map.get(s, 0))
            pos_map[s] = pos_map.get(s, 0) + 1
            out.append(k)
        return out

    src_tags = enumerate_positions(hand_labels_with_draw)
    tgt_tags = enumerate_positions(target)

    # 建立从 src 每张到 tgt 的对应位置
    from collections import defaultdict, deque
    buckets = defaultdict(deque)
    for idx_t, tag in enumerate(tgt_tags):
        buckets[tag].append(idx_t)

    slot_idx = [0] * n
    for idx_s, tag in enumerate(src_tags):
        slot_idx[idx_s] = buckets[tag].popleft()

    return slot_idx


def choose_discard_slot(
        hand_labels_with_draw: List[str],
        target_label: str,
        allow_tsumogiri: bool = False
) -> int:
    """
    选择要丢 target_label 的“屏幕槽位索引”。
    规则：
      - 'bd'（癞子）不可打出，返回 -1
      - 若 allow_tsumogiri=False，尽量选择“最后一张以外”的同名牌；
        如果只有摸牌那一张可打且 allow_tsumogiri=False，则返回 -1
      - 若 allow_tsumogiri=True，允许丢最右边摸进的那张（tsumogiri）
      - 如果有多张同名牌，选择“最右侧的那一张”（更接近实际操作）
    返回：槽位索引（0..N-1），不可打出返回 -1
    """
    if target_label == "bd":
        return -1

    n = len(hand_labels_with_draw)
    if n == 0:
        return -1

    slots = screen_slot_indices_from_hand(hand_labels_with_draw)

    # 收集所有匹配 target_label 的索引（输入中的下标）
    idxs = [i for i, lab in enumerate(hand_labels_with_draw) if lab == target_label]
    if not idxs:
        return -1

    last_idx = n - 1  # 输入末尾是摸牌
    # 筛掉摸牌（若不允许摸切）
    cand = [i for i in idxs if i != last_idx] if not allow_tsumogiri else idxs

    if not cand:
        return -1

    # 取“屏幕位置更靠右”的那张
    best_input_idx = max(cand, key=lambda i: slots[i])
    return slots[best_input_idx]


def screen_slot_indices_from_ids(hand_ids_with_draw: List[int], id2label: Dict[int, str]) -> List[int]:
    """
    输入：手牌 id 序列（最后一个是摸进来的）、以及 id->label 的映射（例如 {22:"3p", 49:"0p", ...}）
    输出：每个 id 在屏幕上的槽位索引（考虑左侧排序 + 末尾摸牌）
    """
    labels = [id2label.get(tid, "??") for tid in hand_ids_with_draw]
    # 直接复用 label 版本的逻辑
    return screen_slot_indices_from_hand(labels)


def choose_discard_slot_by_id(
        hand_ids_with_draw: List[int],
        target_id: int,
        id2label: Dict[int, str],
        allow_tsumogiri: bool = False
) -> int:
    """
    选择“指定 tile_id”要丢的屏幕槽位。
    规则：
      - 若该 id 对应 label 为 'bd'（癞子），不可打出，返回 -1
      - 若 target_id 是最后一张且不允许摸切（allow_tsumogiri=False），返回 -1
      - 其余情况：按 id 的位置映射到屏幕槽位（基于当前手牌 + 排序规则）
    """
    if target_id not in hand_ids_with_draw:
        return -1

    label = id2label.get(target_id, "")
    if label == "bd":
        return -1

    n = len(hand_ids_with_draw)
    idx_in_input = hand_ids_with_draw.index(target_id)
    if (idx_in_input == n - 1) and (not allow_tsumogiri):
        return -1

    slots = screen_slot_indices_from_ids(hand_ids_with_draw, id2label)
    return slots[idx_in_input]
