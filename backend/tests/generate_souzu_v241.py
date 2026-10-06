"""Regenerate golden results from the unmodified v2.4.1 algorithm (stdlib only)."""
import json
import hashlib
import random
import subprocess
import sys
import types
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = "backend/autorun/util/souzu_switch_recommender.py"
REVISION = "d3408b6882a785f729fff26b97b94f372f5d17d3"
FIELDS = (
    "status", "reason", "mode", "draws_needed", "switch_discards", "switch_in",
    "switch_batch_sizes", "wall_draws", "post_draw_discards", "waits", "quad_faces",
    "target13", "target14", "remaining_changes", "plan_signature",
)


def main():
    source = subprocess.check_output(["git", "show", f"{REVISION}:{SOURCE}"], cwd=ROOT).decode("utf-8-sig")
    legacy = types.ModuleType("souzu_v241")
    sys.modules[legacy.__name__] = legacy
    exec(compile(source, SOURCE, "exec"), legacy.__dict__)
    cases = []

    def add(name, hand, changes, wall, rounds=0, buff=None, used=0, wall_limit=36, changed=0):
        tiles = hand + changes + wall
        state = {
            "deck_map": {str(i + 1): face for i, face in enumerate(tiles)},
            "hand_tiles": list(range(1, len(hand) + 1)),
            "replacement_tiles": list(range(len(hand) + 1, len(hand) + len(changes) + 1)),
            "wall_tiles": list(range(len(hand) + len(changes) + 1, len(tiles) + 1)),
            "switch_used_tiles": list(range(used)), "total_change_tile_count": rounds + changed,
            "change_tile_count": changed, "boss_buff": buff or [],
        }
        wall_ids = state["wall_tiles"] if wall_limit >= 36 else state["wall_tiles"][:wall_limit]
        result = legacy.recommend_souzu_tenpai_switch(
            {int(k): v for k, v in state["deck_map"].items()}, state["hand_tiles"],
            state["replacement_tiles"], wall_ids, state["switch_used_tiles"],
            state["total_change_tile_count"], changed, state["boss_buff"],
        )
        per_change = 3 if 901 in state["boss_buff"] else 13
        considered_changes = changes[used:used + rounds * per_change]
        considered_wall = wall if wall_limit >= 36 else wall[:wall_limit]
        normalize = lambda values: [legacy._norm(t) for t in values]
        expected = {key: result[key] for key in FIELDS if key in result}
        if result["status"] == "plan":
            _, logs = legacy._tes_simulate_change(normalize(hand), normalize(considered_changes),
                normalize(considered_wall[:result["draws_needed"]]), result["target14"], rounds, per_change)
            expected["switch_rounds"] = [{key: log[key] for key in ("hand", "keep", "replace")} for log in logs]
        case = {"name": name, "state": state, "wall_limit": wall_limit, "expected": expected}
        if name in ("first-prefix", "red-five", "replacement-needed", "bamboo-2", "bamboo-3", "shuffled-0"):
            targets = legacy._tes_generate_targets(normalize(hand + considered_changes + considered_wall), rounds, per_change, len(considered_wall))
            digest = hashlib.sha256()
            for target in targets:
                digest.update((json.dumps(target, separators=(",", ":")) + "\n").encode())
            case["generation"] = {"count": len(targets), "sha256": digest.hexdigest()}
        cases.append(case)

    hand = ["1m"] * 4 + ["2p"] * 4 + ["1s", "2s", "3s", "4s", "5s"]
    add("first-prefix", hand, [], ["6s", "7s", "8s", "9s"])
    add("empty-wall", hand, [], [])
    add("one-wall-tile", hand, [], ["6s"])
    add("invalid-hand-size", hand[:-1], [], ["6s", "7s"])
    add("no-quads", ["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p"], [], ["6s", "7s"])
    add("empty-rounds", hand, ["9p"] * 3, ["6s", "7s"], rounds=2)
    add("replacement-needed", hand[:-2] + ["8p", "9p"], ["4s", "5s"], ["6s", "7s"], rounds=1)
    add("used-replacements", hand[:-2] + ["8p", "9p"], ["9z", "9z", "4s", "5s"], ["6s", "7s"], rounds=1, used=2, changed=2)
    red_hand = ["0m", "5m", "5m", "5m"] + hand[4:]
    add("red-five", red_hand, [], ["6s", "7s"])
    add("wall-limit-two", hand, [], ["1z", "2z", "6s", "7s"], wall_limit=2)
    add("later-prefix", hand, [], ["1z", "2z", "6s", "7s"], wall_limit=4)
    # More than 13 rounds are not needed; include both normal and 901 exchange limits.
    for seed in range(36):
        rng = random.Random(seed)
        deck = [f"{rank}{suit}" for suit in "mps" for rank in range(1, 10) for _ in range(4)]
        deck += [f"{rank}z" for rank in range(1, 8) for _ in range(4)]
        rng.shuffle(deck)
        add(f"shuffled-{seed}", deck[:13], deck[13:52], deck[52:52 + (36 if seed < 4 else 8)],
            rounds=1 + seed % 3, buff=[901] if seed % 2 else [], used=seed % 4, changed=seed % 3)
    # Concentrated hands exercise exclusions, duplicate targets, and wall-first matching.
    for seed in range(24):
        rng = random.Random(100 + seed)
        deck = ["1m"] * 4 + ["2p"] * 4 + [f"{rank}s" for rank in range(1, 10) for _ in range(4)]
        rng.shuffle(deck)
        add(f"bamboo-{seed}", deck[:13], deck[13:31], deck[31:], rounds=seed % 4,
            buff=[901] if seed % 2 else [])
    path = ROOT / "backend/tests/fixtures/souzu_v241.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text('{"revision":' + json.dumps(REVISION) + ',"cases":[\n'
        + ',\n'.join(json.dumps(case, ensure_ascii=False, separators=(",", ":")) for case in cases) + '\n]}\n', encoding="utf-8")
    print(f"Generated {len(cases)} cases ({sum(c['expected']['status'] == 'plan' for c in cases)} plans)", flush=True)


if __name__ == "__main__":
    main()
